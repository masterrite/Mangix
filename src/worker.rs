//! Two background threads. One decodes the page you're looking at (and reads
//! ahead), the other grinds through thumbnails. Neither ever touches the UI
//! directly — finished pixels are posted to the event loop as plain bytes.

use crate::book::{Book, Reader};
use crate::AppWindow;
use crate::Thumb;

use image::imageops::FilterType;
use image::RgbImage;
use slint::{Image, Model, Rgb8Pixel, SharedPixelBuffer, SharedString, Weak};
use std::collections::{HashMap, VecDeque};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;

/// Fallback cap on a page's longest edge; the quality setting replaces it.
const MAX_DIM: u32 = 2400;
/// Decoded pages held in memory.
const CACHE_PAGES: usize = 6;
/// Gutter drawn between facing pages.
const GUTTER: u32 = 8;

const THUMB_W: u32 = 140;
const THUMB_H: u32 = 200;

pub enum PageCmd {
    Open(Arc<Book>),
    /// One page of the continuous view, rendered at the width it is displayed.
    Strip {
        index: usize,
        width: u32,
    },
    Show {
        index: usize,
        spread: bool,
        rtl: bool,
        dark: bool,
    },
    Quit,
}

pub enum ThumbCmd {
    Open(Arc<Book>),
    Quit,
}

// ---------------------------------------------------------------- page thread

pub fn page_worker(rx: Receiver<PageCmd>, ui: Weak<AppWindow>) {
    let mut book: Option<Arc<Book>> = None;
    let mut reader: Option<Reader> = None;
    let mut cache: Cache = Cache::new(CACHE_PAGES, MAX_DIM);
    // A command that arrived while we were reading ahead.
    let mut stashed: Option<PageCmd> = None;

    loop {
        // Take everything that's queued and keep only the newest request:
        // holding down the arrow key shouldn't decode every page on the way.
        let mut batch = match stashed.take() {
            Some(cmd) => vec![cmd],
            None => match rx.recv() {
                Ok(cmd) => vec![cmd],
                Err(_) => return,
            },
        };
        while let Ok(cmd) = rx.try_recv() {
            batch.push(cmd);
        }

        let mut wanted: Option<(usize, bool, bool, bool)> = None;
        let mut strips: Vec<(usize, u32)> = Vec::new();
        for cmd in batch {
            match cmd {
                PageCmd::Quit => return,
                PageCmd::Open(b) => {
                    cache.clear();
                    match Reader::new(&b) {
                        Ok(r) => reader = Some(r),
                        Err(e) => {
                            set_status(&ui, format!("Couldn't open that comic: {e}"));
                            reader = None;
                        }
                    }
                    book = Some(b);
                    wanted = None;
                }
                PageCmd::Strip { index, width } => strips.push((index, width)),
                PageCmd::Show {
                    index,
                    spread,
                    rtl,
                    dark,
                } => wanted = Some((index, spread, rtl, dark)),
            }
        }

        if let (Some(b), Some(rd)) = (book.clone(), reader.as_mut()) {
            for (index, width) in strips.drain(..) {
                let Ok(page) = cache.get_sized(rd, &b, index, width) else {
                    continue;
                };
                send_strip(&ui, page, index);
            }
        }

        let (Some((index, spread, rtl, dark)), Some(b), Some(rd)) =
            (wanted, book.clone(), reader.as_mut())
        else {
            continue;
        };

        let span = render(
            &ui,
            rd,
            &b,
            &mut cache,
            View {
                index,
                spread,
                rtl,
                dark,
            },
        );

        // Read ahead, but drop it the moment a new request lands.
        for ahead in [
            index + span,
            index + span + 1,
            index + span + 2,
            index.saturating_sub(1),
        ] {
            match rx.try_recv() {
                Ok(next) => {
                    stashed = Some(next);
                    break;
                }
                Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => {}
            }
            if ahead >= b.len() {
                continue;
            }
            let _ = cache.get(rd, &b, ahead);
        }
    }
}

/// Decodes and posts the page (or pair of pages) at `index`, returning how
/// many pages ended up on screen.
/// What the reader has been asked to show.
#[derive(Clone, Copy)]
struct View {
    index: usize,
    spread: bool,
    rtl: bool,
    dark: bool,
}

fn render(
    ui: &Weak<AppWindow>,
    rd: &mut Reader,
    book: &Arc<Book>,
    cache: &mut Cache,
    view: View,
) -> usize {
    let View {
        index,
        spread,
        rtl,
        dark,
    } = view;

    let first = match cache.get(rd, book, index) {
        Ok(img) => img,
        Err(e) => {
            set_status(ui, format!("Page {}: {e}", index + 1));
            post(ui, |ui| ui.set_loading(false));
            return 1;
        }
    };

    // The cover stays on its own, and a double-width splash page is already a
    // spread — pairing either one would be wrong.
    let mut second = None;
    if spread && index > 0 && index + 1 < book.len() && first.height() > first.width() {
        if let Ok(next) = cache.get(rd, book, index + 1) {
            if next.height() > next.width() {
                second = Some(next);
            }
        }
    }

    let (canvas, span) = match second {
        Some(next) => {
            let key = (index, rtl, dark);
            match cache.spread_for(key) {
                Some(ready) => (ready, 2),
                None => {
                    let (l, r) = if rtl {
                        (next.as_ref(), first.as_ref())
                    } else {
                        (first.as_ref(), next.as_ref())
                    };
                    let joined = Arc::new(join(l, r, dark));
                    cache.keep_spread(key, joined.clone());
                    (joined, 2)
                }
            }
        }
        // Single pages ride along as the cached Arc: no copy at all here.
        None => (first, 1),
    };

    send(ui, canvas, index, span);
    span
}

/// Scales a page to an exact display width. Pages arrive close to this
/// already, so this is usually a small adjustment rather than real work.
fn fit_width(img: image::DynamicImage, width: u32) -> RgbImage {
    let width = width.max(1);
    if img.width() == width {
        return img.to_rgb8();
    }
    let height = ((img.height() as f32) * (width as f32) / (img.width() as f32)).round() as u32;
    img.resize_exact(width, height.max(1), FilterType::Triangle)
        .to_rgb8()
}

/// Posts one scrolling-view page into its slot in the model.
fn send_strip(ui: &Weak<AppWindow>, canvas: Arc<RgbImage>, index: usize) {
    let (w, h) = (canvas.width(), canvas.height());
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui.upgrade() else { return };
        let model = ui.get_strip_pages();
        if index >= model.row_count() {
            return;
        }
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
        buffer.make_mut_bytes().copy_from_slice(canvas.as_raw());
        let mut row = model.row_data(index).unwrap_or_default();
        row.image = Image::from_rgb8(buffer);
        row.ready = true;
        model.set_row_data(index, row);
    });
}

/// Hands a finished canvas to the UI thread, where it becomes a slint image.
fn send(ui: &Weak<AppWindow>, canvas: Arc<RgbImage>, index: usize, span: usize) {
    let (w, h) = (canvas.width(), canvas.height());
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        let Some(ui) = ui.upgrade() else { return };
        let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
        buffer.make_mut_bytes().copy_from_slice(canvas.as_raw());
        ui.set_page_image(Image::from_rgb8(buffer));
        ui.set_page_w(w as i32);
        ui.set_page_h(h as i32);
        ui.set_span(span as i32);
        ui.set_loading(false);
        ui.set_page_pending(false);
        ui.set_status(SharedString::new());

        let total = ui.get_page_count();
        let label = if span > 1 {
            format!("{}\u{2013}{} of {}", index + 1, index + span, total)
        } else {
            format!("{} of {}", index + 1, total)
        };
        ui.set_page_label(label.into());
    });
}

/// Puts two pages side by side on a flat field.
///
/// Nothing is resampled. Facing pages in a scan are rarely the exact same
/// height — a thirty pixel difference is normal — and rescaling a whole page
/// to reconcile that costs hundreds of milliseconds for a difference nobody
/// can see. Instead the canvas is as tall as the taller page, each page is
/// centred in its half, and the leftover shows the gutter colour.
///
/// Every write here is a row-sized `copy_from_slice`. The `imageops` helpers
/// are generic, so they compile into this crate at this crate's optimisation
/// level and degenerate into per-pixel loops over twenty-odd megabytes.
fn join(left: &RgbImage, right: &RgbImage, dark: bool) -> RgbImage {
    let height = left.height().max(right.height()) as usize;
    let (lw, rw) = (left.width() as usize, right.width() as usize);
    let total = lw + GUTTER as usize + rw;
    let colour = if dark {
        [12u8, 11, 16]
    } else {
        [207u8, 202, 192]
    };

    // Pre-built runs of the background colour, copied in a row at a time.
    let blank = |width: usize| -> Vec<u8> {
        let mut row = vec![0u8; width * 3];
        for px in row.chunks_exact_mut(3) {
            px.copy_from_slice(&colour);
        }
        row
    };
    let (blank_left, blank_mid, blank_right) = (blank(lw), blank(GUTTER as usize), blank(rw));

    let (l_row, g_row, r_row, out_row) = (lw * 3, GUTTER as usize * 3, rw * 3, total * 3);
    let mut out = vec![0u8; out_row * height];
    let (lb, rb) = (left.as_raw(), right.as_raw());
    let (l_top, r_top) = (
        (height - left.height() as usize) / 2,
        (height - right.height() as usize) / 2,
    );

    for y in 0..height {
        let row = &mut out[y * out_row..(y + 1) * out_row];

        match y.checked_sub(l_top).filter(|v| *v < left.height() as usize) {
            Some(src) => row[..l_row].copy_from_slice(&lb[src * l_row..(src + 1) * l_row]),
            None => row[..l_row].copy_from_slice(&blank_left),
        }

        row[l_row..l_row + g_row].copy_from_slice(&blank_mid);

        match y
            .checked_sub(r_top)
            .filter(|v| *v < right.height() as usize)
        {
            Some(src) => row[l_row + g_row..].copy_from_slice(&rb[src * r_row..(src + 1) * r_row]),
            None => row[l_row + g_row..].copy_from_slice(&blank_right),
        }
    }

    RgbImage::from_raw(total as u32, height as u32, out).expect("buffer is sized for the image")
}

// ------------------------------------------------------------- thumb thread

pub fn thumb_worker(rx: Receiver<ThumbCmd>, ui: Weak<AppWindow>) {
    let mut queued: Option<ThumbCmd> = None;

    loop {
        let cmd = match queued.take() {
            Some(c) => c,
            None => match rx.recv() {
                Ok(c) => c,
                Err(_) => return,
            },
        };

        let book = match cmd {
            ThumbCmd::Quit => return,
            ThumbCmd::Open(b) => b,
        };

        let Ok(mut rd) = Reader::new(&book) else {
            continue;
        };

        // The page you're looking at matters more than the rail. Give the page
        // worker a clear run at the first page before starting.
        std::thread::sleep(std::time::Duration::from_millis(250));

        // Measure first. Reading image headers is far cheaper than decoding,
        // and until this finishes the continuous view cannot size its rows.
        for (index, entry) in book.entries.iter().enumerate() {
            let Ok((w, h)) = rd.page_size(entry) else {
                continue;
            };
            let aspect = if h > 0 { w as f32 / h as f32 } else { 0.7 };
            let ui = ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui.upgrade() else { return };
                let model = ui.get_strip_pages();
                if index < model.row_count() {
                    let mut row = model.row_data(index).unwrap_or_default();
                    row.aspect = aspect;
                    model.set_row_data(index, row);
                }
            });
        }

        for (index, entry) in book.entries.iter().enumerate() {
            // A newer book (or a quit) preempts the rest of this one.
            match rx.try_recv() {
                Ok(next) => {
                    queued = Some(next);
                    break;
                }
                Err(TryRecvError::Disconnected) => return,
                Err(TryRecvError::Empty) => {}
            }

            // Yield between pages so thumbnailing never starves page turns.
            std::thread::sleep(std::time::Duration::from_millis(3));

            let Ok(rendered) = rd.page_image(entry, THUMB_H * 2) else {
                continue;
            };
            let small = rendered.thumbnail(THUMB_W, THUMB_H).into_rgb8();
            let (w, h) = (small.width(), small.height());
            if w == 0 || h == 0 {
                continue;
            }
            let raw = small.into_raw();

            let ui = ui.clone();
            let _ = slint::invoke_from_event_loop(move || {
                let Some(ui) = ui.upgrade() else { return };
                let model = ui.get_thumbnails();
                if index >= model.row_count() {
                    return;
                }
                let mut buffer = SharedPixelBuffer::<Rgb8Pixel>::new(w, h);
                buffer.make_mut_bytes().copy_from_slice(&raw);
                model.set_row_data(
                    index,
                    Thumb {
                        idx: index as i32,
                        label: format!("{}", index + 1).into(),
                        preview: Image::from_rgb8(buffer),
                        ready: true,
                    },
                );
            });
        }
    }
}

// ------------------------------------------------------------------ plumbing

struct Cache {
    spread: Option<((usize, bool, bool), Arc<RgbImage>)>,
    map: HashMap<usize, Arc<RgbImage>>,
    order: VecDeque<usize>,
    limit: usize,
    cap: u32,
}

impl Cache {
    fn new(limit: usize, cap: u32) -> Cache {
        Cache {
            spread: None,
            map: HashMap::new(),
            order: VecDeque::new(),
            limit,
            cap,
        }
    }

    fn spread_for(&self, key: (usize, bool, bool)) -> Option<Arc<RgbImage>> {
        match &self.spread {
            Some((k, img)) if *k == key => Some(img.clone()),
            _ => None,
        }
    }

    fn keep_spread(&mut self, key: (usize, bool, bool), img: Arc<RgbImage>) {
        self.spread = Some((key, img));
    }

    fn clear(&mut self) {
        self.spread = None;
        self.map.clear();
        self.order.clear();
    }

    /// Renders a page at a given width for the scrolling view. Deliberately
    /// not cached here: the UI holds these, and they are sized for the screen
    /// rather than for the page cache.
    fn get_sized(
        &mut self,
        rd: &mut Reader,
        book: &Arc<Book>,
        index: usize,
        width: u32,
    ) -> anyhow::Result<Arc<RgbImage>> {
        let entry = book
            .entries
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("no such page"))?;
        let rendered = rd.page_image(entry, width.clamp(200, MAX_DIM))?;
        Ok(Arc::new(fit_width(rendered, width)))
    }

    fn get(
        &mut self,
        rd: &mut Reader,
        book: &Arc<Book>,
        index: usize,
    ) -> anyhow::Result<Arc<RgbImage>> {
        if let Some(hit) = self.map.get(&index) {
            return Ok(hit.clone());
        }
        let entry = book
            .entries
            .get(index)
            .ok_or_else(|| anyhow::anyhow!("no such page"))?;
        let page = Arc::new(cap_size(rd.page_image(entry, self.cap)?, self.cap));

        self.map.insert(index, page.clone());
        self.order.push_back(index);
        while self.order.len() > self.limit {
            if let Some(old) = self.order.pop_front() {
                self.map.remove(&old);
            }
        }
        Ok(page)
    }
}

fn cap_size(img: image::DynamicImage, cap: u32) -> RgbImage {
    let (w, h) = (img.width(), img.height());
    if w.max(h) > cap {
        let factor = cap as f32 / w.max(h) as f32;
        let nw = ((w as f32 * factor).round() as u32).max(1);
        let nh = ((h as f32 * factor).round() as u32).max(1);
        img.resize_exact(nw, nh, FilterType::Triangle).into_rgb8()
    } else {
        img.into_rgb8()
    }
}

fn post(ui: &Weak<AppWindow>, f: impl FnOnce(AppWindow) + Send + 'static) {
    let ui = ui.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(ui) = ui.upgrade() {
            f(ui);
        }
    });
}

fn set_status(ui: &Weak<AppWindow>, message: String) {
    post(ui, move |ui| ui.set_status(message.into()));
}
