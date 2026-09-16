//! Accepting files dragged from Explorer.
//!
//! Slint 1.17 can drag and drop within an application, but taking a drop from
//! another one is still being built upstream in winit. Until that lands the
//! window is hooked directly: Windows is told to accept files, and WM_DROPFILES
//! is intercepted by a subclass that hands the path back to the UI thread.
//!
//! This uses `windows-sys` rather than `windows`: the same calls, declared as
//! plain FFI instead of generated wrapper types, which is a fraction of the
//! code to compile and link for the five functions needed here.
//!
//! Everything here is a no-op on other platforms.

use std::path::PathBuf;
use std::sync::mpsc::Sender;

#[cfg(windows)]
mod imp {
    use super::*;
    use std::sync::Mutex;
    use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows_sys::Win32::UI::Shell::{
        DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, SetWindowSubclass, HDROP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::WM_DROPFILES;

    /// Where dropped paths go. The window procedure runs on the UI thread but
    /// outside any closure we own, so the sender has to live here.
    static DROPS: Mutex<Option<Sender<PathBuf>>> = Mutex::new(None);

    const SUBCLASS_ID: usize = 0x4d41_4e47; // "MANG"

    unsafe extern "system" fn wndproc(
        window: HWND,
        message: u32,
        wparam: WPARAM,
        lparam: LPARAM,
        _id: usize,
        _data: usize,
    ) -> LRESULT {
        if message != WM_DROPFILES {
            return unsafe { DefSubclassProc(window, message, wparam, lparam) };
        }

        let handle = wparam as HDROP;
        unsafe {
            // Only the first file: this reader opens one book at a time.
            let mut buffer = [0u16; 32768];
            let written = DragQueryFileW(handle, 0, buffer.as_mut_ptr(), buffer.len() as u32);
            if written > 0 {
                let path = PathBuf::from(String::from_utf16_lossy(&buffer[..written as usize]));
                if let Ok(guard) = DROPS.lock() {
                    if let Some(sender) = guard.as_ref() {
                        let _ = sender.send(path);
                    }
                }
            }
            DragFinish(handle);
        }
        0
    }

    pub fn accept(hwnd: isize, sender: Sender<PathBuf>) {
        if let Ok(mut guard) = DROPS.lock() {
            *guard = Some(sender);
        }
        let window = hwnd as HWND;
        unsafe {
            DragAcceptFiles(window, 1);
            SetWindowSubclass(window, Some(wndproc), SUBCLASS_ID, 0);
        }
    }
}

/// Starts accepting dropped files. Does nothing where it isn't supported.
#[cfg(windows)]
pub fn accept(window: &slint::Window, sender: Sender<PathBuf>) {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    // Bind the slint handle: the borrow below comes out of it.
    let owner = window.window_handle();
    let Ok(handle) = owner.window_handle() else {
        return;
    };
    if let RawWindowHandle::Win32(win32) = handle.as_raw() {
        imp::accept(win32.hwnd.get(), sender);
    }
}

#[cfg(not(windows))]
pub fn accept(_window: &slint::Window, _sender: Sender<PathBuf>) {}
