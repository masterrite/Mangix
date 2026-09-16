//! Turning a path into an ordered list of pages, and reading the bytes of one.

use anyhow::{anyhow, Context, Result};
use std::cmp::Ordering;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

const IMAGE_EXTS: &[&str] = &[
    "jpg", "jpeg", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff",
];

pub fn is_image(name: &str) -> bool {
    match name.rsplit_once('.') {
        Some((_, ext)) => IMAGE_EXTS.contains(&ext.to_ascii_lowercase().as_str()),
        None => false,
    }
}

fn is_archive(path: &Path) -> Option<&'static str> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "cbz" | "zip" => Some("zip"),
        "cbr" | "rar" | "cb7" | "7z" => Some("external"),
        "pdf" => Some("pdf"),
        _ => None,
    }
}

/// Where a single page lives.
#[derive(Clone, Debug)]
pub enum Entry {
    Zip {
        index: usize,
        name: String,
    },
    File(PathBuf),
    /// A page of a PDF, rendered on demand rather than read as bytes.
    Pdf {
        index: usize,
    },
}

impl Entry {
    pub fn sort_key(&self) -> String {
        match self {
            Entry::Zip { name, .. } => name.clone(),
            Entry::File(p) => p.to_string_lossy().into_owned(),
            // Already in reading order; pad so 10 sorts after 9.
            Entry::Pdf { index } => format!("{index:08}"),
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum Kind {
    Zip,
    Loose,
    Pdf,
}

pub struct Book {
    pub title: String,
    /// The archive itself, or the directory the pages came from.
    pub path: PathBuf,
    pub entries: Vec<Entry>,
    pub kind: Kind,
    /// Kept alive so unpacked pages outlive the reader, and cleaned up on drop.
    _scratch: Option<ScratchDir>,
}

impl Book {
    /// Opens whatever the path points at. Returns the book and the page to
    /// land on (non-zero when the user picked one image out of a folder).
    pub fn open(path: &Path) -> Result<(Book, usize)> {
        if path.is_dir() {
            let book = Self::from_dir(path, path)?;
            return Ok((book, 0));
        }

        match is_archive(path) {
            Some("zip") => {
                let book = Self::from_zip(path)?;
                // Deflate covers essentially every CBZ, but zip also allows
                // zstd, lzma and xz. Read one page to find out which we have
                // before the reader is handed to a worker thread.
                match Reader::new(&book).and_then(|mut r| r.read(&book.entries[0])) {
                    Ok(_) => Ok((book, 0)),
                    Err(_) => Ok((Self::from_external(path)?, 0)),
                }
            }
            Some("external") => Ok((Self::from_external(path)?, 0)),
            Some("pdf") => Ok((Self::from_pdf(path)?, 0)),
            _ => {
                // A loose image: read the whole folder, start on that page.
                let name = path
                    .file_name()
                    .ok_or_else(|| anyhow!("that path has no file name"))?;
                if !is_image(&name.to_string_lossy()) {
                    return Err(anyhow!(
                        "{} isn't a comic archive or an image",
                        path.display()
                    ));
                }
                let dir = path.parent().unwrap_or(Path::new("."));
                let book = Self::from_dir(dir, path)?;
                let start = book
                    .entries
                    .iter()
                    .position(|e| matches!(e, Entry::File(p) if p == path))
                    .unwrap_or(0);
                Ok((book, start))
            }
        }
    }

    fn from_dir(dir: &Path, title_from: &Path) -> Result<Book> {
        let mut entries: Vec<Entry> = Vec::new();
        collect_images(dir, &mut entries, 0)?;
        if entries.is_empty() {
            return Err(anyhow!("no images in {}", dir.display()));
        }
        sort_entries(&mut entries);
        Ok(Book {
            title: display_name(if title_from.is_dir() { dir } else { title_from }),
            path: dir.to_path_buf(),
            entries,
            kind: Kind::Loose,
            _scratch: None,
        })
    }

    fn from_zip(path: &Path) -> Result<Book> {
        let file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
        let mut zip = zip::ZipArchive::new(file)
            .with_context(|| format!("{} isn't a readable zip archive", path.display()))?;

        let mut entries = Vec::new();
        for index in 0..zip.len() {
            let f = zip.by_index(index)?;
            if f.is_dir() {
                continue;
            }
            let name = f.name().to_string();
            if wanted(&name) {
                entries.push(Entry::Zip { index, name });
            }
        }
        if entries.is_empty() {
            return Err(anyhow!("no images in {}", path.display()));
        }
        sort_entries(&mut entries);

        Ok(Book {
            title: display_name(path),
            path: path.to_path_buf(),
            entries,
            kind: Kind::Zip,
            _scratch: None,
        })
    }

    /// RAR and 7z are unpacked by whatever 7-Zip is on the machine. Neither
    /// format has a usable pure-Rust decoder, and shelling out keeps a C++
    /// toolchain (and RARLAB's licence) out of this build entirely.
    fn from_external(path: &Path) -> Result<Book> {
        let scratch = ScratchDir::new()?;
        let mut tried: Vec<String> = Vec::new();

        // 7-Zip first, then unrar. A 7-Zip build without the RAR codec exits
        // non-zero with "Unsupported Method", so falling through matters.
        let unpacked = unpack_with_seven_zip(path, scratch.path(), &mut tried)
            || unpack_with_unrar(path, scratch.path(), &mut tried);

        if !unpacked {
            return Err(anyhow!(
                "couldn't unpack {}. {} Install 7-Zip from 7-zip.org (the `7zz` build), \
                 or unrar (`apt install unrar`, `brew install carlocab/personal/unrar`).",
                path.display(),
                if tried.is_empty() {
                    "No 7-Zip or unrar was found on this machine.".to_string()
                } else {
                    format!("Tried: {}.", tried.join("; "))
                }
            ));
        }

        let mut entries = Vec::new();
        collect_images(scratch.path(), &mut entries, 0)?;
        if entries.is_empty() {
            return Err(anyhow!("no images in {}", path.display()));
        }
        sort_entries(&mut entries);

        Ok(Book {
            title: display_name(path),
            path: path.to_path_buf(),
            entries,
            kind: Kind::Loose,
            _scratch: Some(scratch),
        })
    }

    /// PDFs carry no page images to extract, so the book is just a page count
    /// and each page is rendered when it is wanted.
    fn from_pdf(path: &Path) -> Result<Book> {
        let count = crate::pdf::page_count(path)?;
        if count == 0 {
            return Err(anyhow!("{} has no pages", path.display()));
        }

        Ok(Book {
            title: display_name(path),
            path: path.to_path_buf(),
            entries: (0..count).map(|index| Entry::Pdf { index }).collect(),
            kind: Kind::Pdf,
            _scratch: None,
        })
    }

    #[allow(clippy::len_without_is_empty)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Skips the junk that ends up inside comic archives.
fn wanted(name: &str) -> bool {
    let leaf = name.rsplit('/').next().unwrap_or(name);
    is_image(name) && !leaf.starts_with('.') && !name.contains("__MACOSX")
}

fn collect_images(dir: &Path, out: &mut Vec<Entry>, depth: usize) -> Result<()> {
    if depth > 6 {
        return Ok(());
    }
    let read = std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?;
    for item in read.flatten() {
        let path = item.path();
        if path.is_dir() {
            collect_images(&path, out, depth + 1)?;
        } else if wanted(&path.to_string_lossy()) {
            out.push(Entry::File(path));
        }
    }
    Ok(())
}

fn sort_entries(entries: &mut Vec<Entry>) {
    // Build each key once rather than twice per comparison.
    let mut keyed: Vec<(String, Entry)> = entries
        .drain(..)
        .map(|e| (e.sort_key().to_lowercase(), e))
        .collect();
    keyed.sort_by(|a, b| natural_cmp(&a.0, &b.0));
    entries.extend(keyed.into_iter().map(|(_, e)| e));
}

fn display_name(path: &Path) -> String {
    path.file_stem()
        .or_else(|| path.file_name())
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string_lossy().into_owned())
}

/// Orders names the way a person would: page9 before page10, and "1-2" after
/// "1-1". Falls back to a case-insensitive comparison for the rest.
/// Orders names the way a person would: page9 before page10, and "1-2" after
/// "1-1". Compares bytes, so callers fold case themselves if they want it.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    let (mut i, mut j) = (0usize, 0usize);

    while i < a.len() && j < b.len() {
        if a[i].is_ascii_digit() && b[j].is_ascii_digit() {
            let (left, ni) = take_number(a, i);
            let (right, nj) = take_number(b, j);
            if left != right {
                return left.cmp(&right);
            }
            i = ni;
            j = nj;
        } else {
            if a[i] != b[j] {
                return a[i].cmp(&b[j]);
            }
            i += 1;
            j += 1;
        }
    }
    (a.len() - i).cmp(&(b.len() - j))
}

fn take_number(s: &[u8], mut i: usize) -> (u128, usize) {
    let mut n: u128 = 0;
    while i < s.len() && s[i].is_ascii_digit() {
        // Saturate rather than overflow on absurdly long digit runs.
        n = n.saturating_mul(10).saturating_add((s[i] - b'0') as u128);
        i += 1;
    }
    (n, i)
}

/// Pulls the bytes of one page. Holds the archive open between reads.
pub struct Reader {
    zip: Option<zip::ZipArchive<File>>,
    /// Which PDF to ask the pdfium thread about.
    pdf: Option<PathBuf>,
}

impl Reader {
    pub fn new(book: &Book) -> Result<Reader> {
        let mut reader = Reader {
            zip: None,
            pdf: None,
        };
        match book.kind {
            Kind::Zip => reader.zip = Some(zip::ZipArchive::new(File::open(&book.path)?)?),
            Kind::Loose => {}
            Kind::Pdf => reader.pdf = Some(book.path.clone()),
        }
        Ok(reader)
    }

    /// Produces the page image, whatever it takes: decoding a file, or asking
    /// pdfium to draw one. `cap` is the longest edge wanted, which for a PDF
    /// decides the rendering resolution rather than trimming afterwards.
    pub fn page_image(&mut self, entry: &Entry, cap: u32) -> Result<image::DynamicImage> {
        if let Entry::Pdf { index } = entry {
            let path = self.pdf.as_ref().ok_or_else(|| anyhow!("no PDF open"))?;
            return crate::pdf::render(path, *index, cap);
        }

        let bytes = self.read(entry)?;
        Ok(image::load_from_memory(&bytes)?)
    }

    pub fn read(&mut self, entry: &Entry) -> Result<Vec<u8>> {
        match entry {
            Entry::File(path) => {
                std::fs::read(path).with_context(|| format!("reading {}", path.display()))
            }
            Entry::Pdf { .. } => Err(anyhow!("a PDF page has no bytes to read")),
            Entry::Zip { index, name } => {
                let zip = self
                    .zip
                    .as_mut()
                    .ok_or_else(|| anyhow!("no archive open for {name}"))?;
                let mut f = zip.by_index(*index)?;
                let mut buf = Vec::with_capacity(f.size() as usize);
                f.read_to_end(&mut buf)
                    .with_context(|| format!("reading {name}"))?;
                Ok(buf)
            }
        }
    }
}

/// Runs 7-Zip if one can be found. Returns false so the caller can try the
/// next tool, recording why this one didn't work.
fn unpack_with_seven_zip(archive: &Path, into: &Path, tried: &mut Vec<String>) -> bool {
    let Some(exe) = seven_zip() else {
        return false;
    };
    let output = quiet_command(&exe)
        .arg("x")
        .arg("-y")
        .arg("-bso0")
        .arg("-bsp0")
        .arg(format!("-o{}", into.display()))
        .arg(archive)
        .output();

    match output {
        Ok(out) if out.status.success() => true,
        Ok(out) => {
            let why = String::from_utf8_lossy(&out.stderr);
            let why = why.lines().last().unwrap_or("failed").trim();
            // Almost always a 7-Zip built without the RAR codec: on Debian and
            // Ubuntu that codec lives in the separate p7zip-rar package.
            let hint = if why.contains("Unsupported Method") {
                " (this 7-Zip has no RAR codec - try p7zip-rar, or the 7zz build)"
            } else {
                ""
            };
            tried.push(format!("{}{hint}", exe.display()));
            false
        }
        Err(e) => {
            tried.push(format!("{} ({e})", exe.display()));
            false
        }
    }
}

/// RARLAB's own extractor, which handles every RAR there is.
fn unpack_with_unrar(archive: &Path, into: &Path, tried: &mut Vec<String>) -> bool {
    for exe in ["unrar", "unrar-free"] {
        let output = quiet_command(Path::new(exe))
            .arg("x")
            .arg("-o+")
            .arg("-idq")
            .arg(archive)
            // unrar wants a trailing separator on the destination.
            .arg(format!("{}{}", into.display(), std::path::MAIN_SEPARATOR))
            .output();
        match output {
            Ok(out) if out.status.success() => return true,
            Ok(out) => tried.push(format!(
                "{exe} ({})",
                String::from_utf8_lossy(&out.stderr)
                    .lines()
                    .last()
                    .unwrap_or("failed")
                    .trim()
            )),
            Err(_) => {} // not installed; not worth reporting
        }
    }
    false
}

/// Binds pdfium once per thread. The library is looked for beside the
/// executable first, then wherever the system keeps it; a leaked box gives the
/// `'static` lifetime that holding an open document needs.
/// Builds a child process that stays invisible. Mangix has no console of its
/// own on Windows, so spawning a console program like 7-Zip makes Windows
/// allocate one — a black window that flashes up on every archive we open.
fn quiet_command(exe: &Path) -> std::process::Command {
    #[allow(unused_mut)] // only the Windows branch below touches it
    let mut command = std::process::Command::new(exe);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

/// Finds a usable 7-Zip. `7z i` prints its capabilities and exits cleanly, so
/// it doubles as a "does this actually run" probe.
fn seven_zip() -> Option<PathBuf> {
    // 7zz and 7zzs are the current official builds (dynamic and static), and
    // 7-Zip ZS is a recent fork with extra codecs. p7zip's "7z"/"7za" are a
    // 16.02 fork that cannot read RAR5, so they come last.
    #[allow(unused_mut)] // only the Windows branch below adds to it
    let mut candidates: Vec<PathBuf> = ["7zz", "7zzs", "7z", "7za"]
        .iter()
        .map(PathBuf::from)
        .collect();
    #[cfg(windows)]
    for var in ["ProgramFiles", "ProgramFiles(x86)"] {
        if let Some(base) = std::env::var_os(var) {
            let base = Path::new(&base);
            for dir in ["7-Zip", "7-Zip-Zstandard", "NanaZip"] {
                candidates.push(base.join(dir).join("7z.exe"));
            }
        }
    }
    candidates.into_iter().find(|exe| {
        quiet_command(exe)
            .arg("i")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .map(|s| s.success())
            .unwrap_or(false)
    })
}

/// A temporary directory that deletes itself. Saves a dependency for the one
/// thing we need one for.
pub struct ScratchDir(PathBuf);

impl ScratchDir {
    fn new() -> Result<ScratchDir> {
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let dir = std::env::temp_dir().join(format!("mangix-{}-{stamp}", std::process::id()));
        std::fs::create_dir_all(&dir)?;
        Ok(ScratchDir(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn numbers_sort_by_value() {
        let mut v = vec!["p10.jpg", "p9.jpg", "p1.jpg", "p02.jpg"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["p1.jpg", "p02.jpg", "p9.jpg", "p10.jpg"]);
    }

    #[test]
    fn chapters_before_pages() {
        let mut v = vec!["ch2/p1.png", "ch10/p1.png", "ch1/p2.png"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, vec!["ch1/p2.png", "ch2/p1.png", "ch10/p1.png"]);
    }

    #[test]
    fn extensions_are_recognised() {
        assert!(is_image("a/b/PAGE01.JPG"));
        assert!(!is_image("ComicInfo.xml"));
        assert!(!wanted("__MACOSX/._page1.jpg"));
    }
}
