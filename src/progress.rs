//! Remembers where you stopped reading. One line per comic, newest first,
//! written as "page<TAB>path" so there is no need for a serialisation crate.

use crate::settings::data_dir;
use std::path::{Path, PathBuf};

const CAP: usize = 500;

pub fn store_path() -> Option<PathBuf> {
    let dir = data_dir()?.join("mangix");
    std::fs::create_dir_all(&dir).ok()?;
    Some(dir.join("progress.tsv"))
}

fn key(path: &Path) -> String {
    std::fs::canonicalize(path)
        .unwrap_or_else(|_| path.to_path_buf())
        .to_string_lossy()
        .replace(['\t', '\n'], " ")
}

pub fn last_page(path: &Path) -> Option<usize> {
    let wanted = key(path);
    let text = std::fs::read_to_string(store_path()?).ok()?;
    text.lines()
        .filter_map(|line| line.split_once('\t'))
        .find(|(_, p)| *p == wanted)
        .and_then(|(page, _)| page.parse().ok())
}

/// A single thread owns the bookmark file. Spawning a writer per page turn
/// meant several threads doing read-modify-write on the same file at once,
/// which loses updates; this also collapses a burst of turns into one write.
pub fn writer() -> std::sync::mpsc::Sender<(PathBuf, usize)> {
    let (tx, rx) = std::sync::mpsc::channel::<(PathBuf, usize)>();
    std::thread::spawn(move || {
        while let Ok(mut latest) = rx.recv() {
            std::thread::sleep(std::time::Duration::from_millis(400));
            while let Ok(newer) = rx.try_recv() {
                latest = newer;
            }
            remember(&latest.0, latest.1);
        }
    });
    tx
}

fn remember(path: &Path, page: usize) {
    let Some(store) = store_path() else { return };
    let wanted = key(path);
    let previous = std::fs::read_to_string(&store).unwrap_or_default();

    let mut out = format!("{page}\t{wanted}\n");
    let mut kept = 1;
    for line in previous.lines() {
        if kept >= CAP {
            break;
        }
        match line.split_once('\t') {
            Some((_, p)) if p == wanted => continue, // the stale entry
            Some(_) => {}
            None => continue,
        }
        out.push_str(line);
        out.push('\n');
        kept += 1;
    }
    let _ = std::fs::write(store, out);
}
