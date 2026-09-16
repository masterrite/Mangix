// No console window on Windows, debug builds included. This is a GUI app and
// the console only ever showed backend chatter.
#![windows_subsystem = "windows"]

mod book;
mod pdf;
mod progress;
mod settings;
mod worker;

use anyhow::Result;
use book::Book;
use slint::{ComponentHandle, ModelRc, Rgba8Pixel, SharedPixelBuffer, VecModel};
use std::cell::RefCell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use std::sync::Arc;
use worker::{PageCmd, ThumbCmd};

slint::include_modules!();

#[derive(Default)]
struct State {
    book: Option<Arc<Book>>,
    index: usize,
}

fn main() -> Result<()> {
    let ui = AppWindow::new()?;

    let thumbs: Rc<VecModel<Thumb>> = Rc::new(VecModel::default());
    ui.set_thumbnails(ModelRc::from(thumbs.clone()));

    let (page_tx, page_rx) = channel::<PageCmd>();
    let (thumb_tx, thumb_rx) = channel::<ThumbCmd>();
    {
        let handle = ui.as_weak();
        std::thread::spawn(move || worker::page_worker(page_rx, handle));
    }
    {
        let handle = ui.as_weak();
        std::thread::spawn(move || worker::thumb_worker(thumb_rx, handle));
    }

    let saved = settings::load();
    ui.global::<Ink>().set_dark(saved.dark);
    ui.set_fit_mode(saved.fit);
    ui.set_spread(saved.spread);
    ui.set_rtl(saved.rtl);
    ui.set_rail(saved.rail);
    ui.set_resume(saved.resume);

    let bookmarks = progress::writer();
    let state = Rc::new(RefCell::new(State::default()));

    // ---- show a page ----------------------------------------------------
    let show: Rc<dyn Fn(usize)> = {
        let ui = ui.as_weak();
        let state = state.clone();
        let page_tx = page_tx.clone();
        let bookmarks = bookmarks.clone();
        Rc::new(move |requested: usize| {
            let Some(ui) = ui.upgrade() else { return };
            let mut state = state.borrow_mut();
            let Some(book) = state.book.clone() else { return };

            let index = requested.min(book.len().saturating_sub(1));
            state.index = index;
            ui.set_current_page(index as i32);
            // A cached page arrives in about a millisecond. Only admit to
            // loading if it genuinely hasn't turned up yet.
            ui.set_page_pending(true);
            let waiting = ui.as_weak();
            slint::Timer::single_shot(Duration::from_millis(180), move || {
                if let Some(ui) = waiting.upgrade() {
                    if ui.get_page_pending() {
                        ui.set_loading(true);
                    }
                }
            });
            let _ = page_tx.send(PageCmd::Show {
                index,
                spread: ui.get_spread(),
                rtl: ui.get_rtl(),
                dark: ui.global::<Ink>().get_dark(),
            });
            let _ = bookmarks.send((book.path.clone(), index));
        })
    };

    // ---- open a comic ---------------------------------------------------
    // Book::open can take seconds: a CBR is unpacked by 7-Zip, a big folder is
    // walked. That must not happen on the UI thread, so a worker does it and a
    // timer collects the result back here, where the Rc state lives.
    let (book_tx, book_rx) = channel::<anyhow::Result<(Book, usize)>>();

    let open: Rc<dyn Fn(PathBuf)> = {
        let ui = ui.as_weak();
        let book_tx = book_tx.clone();
        Rc::new(move |path: PathBuf| {
            if let Some(ui) = ui.upgrade() {
                ui.set_status("Opening\u{2026}".into());
            }
            let book_tx = book_tx.clone();
            std::thread::spawn(move || {
                let _ = book_tx.send(Book::open(&path));
            });
        })
    };

    let collect = {
        let ui = ui.as_weak();
        let state = state.clone();
        let show = show.clone();
        let thumbs = thumbs.clone();
        let page_tx = page_tx.clone();
        let thumb_tx = thumb_tx.clone();

        move |rx: &Receiver<anyhow::Result<(Book, usize)>>| {
            let Ok(result) = rx.try_recv() else { return };
            let Some(ui) = ui.upgrade() else { return };

            let (book, start) = match result {
                Ok(opened) => opened,
                Err(e) => {
                    ui.set_status(format!("{e}").into());
                    return;
                }
            };

            let book = Arc::new(book);
            let count = book.len();
            let resume = if ui.get_resume() && start == 0 {
                progress::last_page(&book.path)
                    .filter(|p| *p < count)
                    .unwrap_or(0)
            } else {
                start
            };

            ui.set_book_title(book.title.clone().into());
            ui.set_window_title(format!("{} \u{2014} Mangix", book.title).into());
            ui.set_page_count(count as i32);
            ui.set_span(1);
            ui.set_zoom(1.0);
            ui.set_status(slint::SharedString::new());

            thumbs.set_vec(
                (0..count)
                    .map(|i| Thumb {
                        idx: i as i32,
                        label: format!("{}", i + 1).into(),
                        preview: blank(),
                        ready: false,
                    })
                    .collect::<Vec<_>>(),
            );

            state.borrow_mut().book = Some(book.clone());
            let _ = page_tx.send(PageCmd::Open(book.clone()));
            let _ = thumb_tx.send(ThumbCmd::Open(book));
            show(resume);
        }
    };

    let poll = slint::Timer::default();
    poll.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(40),
        move || collect(&book_rx),
    );

    // ---- callbacks ------------------------------------------------------
    ui.on_open_file({
        let open = open.clone();
        move || {
            let picked = rfd::FileDialog::new()
                .set_title("Open a comic")
                .add_filter("Comics", &["cbz", "cbr", "cb7", "zip", "rar", "7z", "pdf"])
                .add_filter("Images", &["jpg", "jpeg", "png", "gif", "webp", "bmp"])
                .add_filter("All files", &["*"])
                .pick_file();
            if let Some(path) = picked {
                open(path);
            }
        }
    });

    ui.on_open_folder({
        let open = open.clone();
        move || {
            if let Some(path) = rfd::FileDialog::new()
                .set_title("Open a folder of pages")
                .pick_folder()
            {
                open(path);
            }
        }
    });

    ui.on_next_page({
        let ui = ui.as_weak();
        let state = state.clone();
        let show = show.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let index = state.borrow().index;
            let step = (ui.get_span().max(1)) as usize;
            let last = state
                .borrow()
                .book
                .as_ref()
                .map(|b| b.len().saturating_sub(1))
                .unwrap_or(0);
            if index < last {
                show(index + step);
            }
        }
    });

    ui.on_prev_page({
        let ui = ui.as_weak();
        let state = state.clone();
        let show = show.clone();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            let index = state.borrow().index;
            if index == 0 {
                return;
            }
            // Facing pages are grouped cover-alone, so stepping back lands on
            // the start of the previous pair.
            let step = if ui.get_spread() && index >= 3 { 2 } else { 1 };
            show(index - step);
        }
    });

    ui.on_first_page({
        let show = show.clone();
        move || show(0)
    });

    ui.on_last_page({
        let state = state.clone();
        let show = show.clone();
        move || {
            let last = state
                .borrow()
                .book
                .as_ref()
                .map(|b| b.len().saturating_sub(1))
                .unwrap_or(0);
            show(last);
        }
    });

    ui.on_goto_page({
        let show = show.clone();
        move |page: f32| show(page.max(0.0) as usize)
    });

    ui.on_refresh({
        let state = state.clone();
        let show = show.clone();
        move || {
            let index = state.borrow().index;
            show(index);
        }
    });

    ui.on_save_settings({
        let ui = ui.as_weak();
        move || {
            let Some(ui) = ui.upgrade() else { return };
            settings::save(&settings::Settings {
                dark: ui.global::<Ink>().get_dark(),
                fit: ui.get_fit_mode(),
                spread: ui.get_spread(),
                rtl: ui.get_rtl(),
                rail: ui.get_rail(),
                resume: ui.get_resume(),
            });
        }
    });

    // ---- open whatever was passed on the command line -------------------
    if let Some(arg) = std::env::args_os().nth(1) {
        let path = PathBuf::from(arg);
        if path.exists() {
            open(path);
        }
    }

    ui.run()?;

    let _ = page_tx.send(PageCmd::Quit);
    let _ = thumb_tx.send(ThumbCmd::Quit);
    Ok(())
}

/// A 1x1 transparent stand-in for thumbnails that haven't been made yet.
fn blank() -> slint::Image {
    slint::Image::from_rgba8(SharedPixelBuffer::<Rgba8Pixel>::new(1, 1))
}
