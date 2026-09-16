//! One thread owns pdfium, and everything else asks it for pages.
//!
//! pdfium expects to be initialised exactly once per process, and
//! `pdfium_render::Pdfium` is not `Sync`, so it cannot be shared behind a
//! static either. Binding it per thread satisfies the compiler and then kills
//! the process the second time the library initialises. The way out is to give
//! pdfium a thread of its own: it is created once, never moves, and the page
//! and thumbnail workers send requests over a channel.

use anyhow::{anyhow, Result};
use image::DynamicImage;
use pdfium_render::prelude::{Pdfium, PdfRenderConfig};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Mutex, OnceLock};

const MISSING: &str = "reading PDFs needs the pdfium library. Put pdfium.dll (pdfium.so on Linux, \
                       libpdfium.dylib on macOS) next to the Mangix executable. Prebuilt copies \
                       are at github.com/bblanchon/pdfium-binaries.";

enum Job {
    Count {
        path: PathBuf,
        reply: Sender<Result<usize, String>>,
    },
    Render {
        path: PathBuf,
        index: usize,
        cap: u32,
        reply: Sender<Result<DynamicImage, String>>,
    },
}

/// How many pages the document has.
pub fn page_count(path: &Path) -> Result<usize> {
    let (reply, answer) = channel();
    submit(Job::Count {
        path: path.to_path_buf(),
        reply,
    })?;
    answer
        .recv()
        .map_err(|_| anyhow!("the pdfium thread stopped"))?
        .map_err(|e| anyhow!(e))
}

/// Draws one page, sized so its longest edge is about `cap`.
pub fn render(path: &Path, index: usize, cap: u32) -> Result<DynamicImage> {
    let (reply, answer) = channel();
    submit(Job::Render {
        path: path.to_path_buf(),
        index,
        cap,
        reply,
    })?;
    answer
        .recv()
        .map_err(|_| anyhow!("the pdfium thread stopped"))?
        .map_err(|e| anyhow!(e))
}

fn submit(job: Job) -> Result<()> {
    static SERVICE: OnceLock<Mutex<Sender<Job>>> = OnceLock::new();

    let service = SERVICE.get_or_init(|| {
        let (tx, rx) = channel::<Job>();
        std::thread::Builder::new()
            .name("pdfium".into())
            .spawn(move || run(rx))
            .expect("failed to start the pdfium thread");
        Mutex::new(tx)
    });

    service
        .lock()
        .map_err(|_| anyhow!("the pdfium thread panicked"))?
        .send(job)
        .map_err(|_| anyhow!("the pdfium thread stopped"))
}

fn run(rx: Receiver<Job>) {
    // Bound once, here, and never anywhere else in the process.
    let Some(pdfium) = bind() else {
        // Answer every request rather than leaving callers waiting forever.
        while let Ok(job) = rx.recv() {
            match job {
                Job::Count { reply, .. } => {
                    let _ = reply.send(Err(MISSING.to_string()));
                }
                Job::Render { reply, .. } => {
                    let _ = reply.send(Err(MISSING.to_string()));
                }
            }
        }
        return;
    };

    // Documents stay open: reopening per page would be wasteful, and the
    // reader only ever looks at one book at a time.
    let mut open: HashMap<PathBuf, pdfium_render::prelude::PdfDocument<'static>> = HashMap::new();

    while let Ok(job) = rx.recv() {
        match job {
            Job::Count { path, reply } => {
                let answer = document(pdfium, &mut open, &path).map(|d| d.pages().len() as usize);
                let _ = reply.send(answer);
            }
            Job::Render {
                path,
                index,
                cap,
                reply,
            } => {
                let answer = document(pdfium, &mut open, &path).and_then(|doc| {
                    let page = doc
                        .pages()
                        .get(index as u16)
                        .map_err(|e| format!("page {}: {e}", index + 1))?;
                    // Comic pages are taller than they are wide, so fitting the
                    // height is what decides the useful resolution.
                    let config = PdfRenderConfig::new()
                        .set_target_height(cap as i32)
                        .set_maximum_width(cap as i32);
                    page.render_with_config(&config)
                        .map(|bitmap| bitmap.as_image())
                        .map_err(|e| format!("rendering page {}: {e}", index + 1))
                });
                let _ = reply.send(answer);
            }
        }
    }
}

fn document<'a>(
    pdfium: &'static Pdfium,
    open: &'a mut HashMap<PathBuf, pdfium_render::prelude::PdfDocument<'static>>,
    path: &Path,
) -> Result<&'a pdfium_render::prelude::PdfDocument<'static>, String> {
    if !open.contains_key(path) {
        // Only one book is ever read at a time; don't accumulate documents.
        if open.len() > 2 {
            open.clear();
        }
        let loaded = pdfium
            .load_pdf_from_file(path, None)
            .map_err(|e| format!("opening {}: {e}", path.display()))?;
        open.insert(path.to_path_buf(), loaded);
    }
    open.get(path)
        .ok_or_else(|| "the document went missing".to_string())
}

/// Looks beside the executable, then in the working directory, then wherever
/// the system keeps its libraries. Leaked so open documents can borrow it for
/// as long as the thread lives.
fn bind() -> Option<&'static Pdfium> {
    let mut places: Vec<PathBuf> = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            places.push(dir.to_path_buf());
        }
    }
    // `cargo run` puts the executable in target/debug, which is not where
    // anyone thinks to drop a DLL.
    if let Ok(cwd) = std::env::current_dir() {
        places.push(cwd);
    }

    let bindings = places
        .iter()
        .find_map(|dir| Pdfium::bind_to_library(Pdfium::pdfium_platform_library_name_at_path(dir)).ok())
        .or_else(|| Pdfium::bind_to_system_library().ok())?;

    Some(&*Box::leak(Box::new(Pdfium::new(bindings))))
}
