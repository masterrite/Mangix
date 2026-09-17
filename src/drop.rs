//! Accepting files dragged from Explorer.
//!
//! Slint can drag and drop within an application, but taking a drop from
//! another one is still being built upstream in winit. Until that lands the
//! window is hooked directly: Windows is told to accept files, and WM_DROPFILES
//! is intercepted by a subclass that hands the path back to the UI thread.
//!
//! The catch is that winit has already claimed the window. It calls
//! RegisterDragDrop on everything it creates (`with_drag_and_drop` defaults to
//! true, and Slint exposes no way to turn it off), and a registered OLE drop
//! target owns the drop outright: WS_EX_ACCEPTFILES is never consulted and
//! WM_DROPFILES is never posted. winit's own target then discards the file,
//! because Slint does not forward winit's DroppedFile event. So the OLE target
//! is revoked first, which hands the window back to the shell's default
//! handler and puts WM_DROPFILES back in play.
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
    use windows_sys::Win32::System::Ole::RevokeDragDrop;
    use windows_sys::Win32::UI::Shell::{
        DefSubclassProc, DragAcceptFiles, DragFinish, DragQueryFileW, SetWindowSubclass, HDROP,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        ChangeWindowMessageFilterEx, MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES,
    };

    /// Not in windows-sys: the private message the shell uses to hand the
    /// HDROP block across processes. UIPI filters it by name like any other.
    const WM_COPYGLOBALDATA: u32 = 0x0049;

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
            // Ask for the length first rather than parking 64 KB on the stack
            // of a window procedure.
            let needed = DragQueryFileW(handle, 0, std::ptr::null_mut(), 0);
            if needed > 0 {
                let mut buffer = vec![0u16; needed as usize + 1];
                let written =
                    DragQueryFileW(handle, 0, buffer.as_mut_ptr(), buffer.len() as u32);
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
            // Subclass first, so nothing is missed between the two calls.
            SetWindowSubclass(window, Some(wndproc), SUBCLASS_ID, 0);

            // Drop winit's OLE target. Returns DRAGDROP_E_NOTREGISTERED if it
            // was never there, which is fine and means nothing to undo.
            let _ = RevokeDragDrop(window);

            DragAcceptFiles(window, 1);

            // If Mangix runs elevated and Explorer does not, UIPI discards
            // these three before the subclass ever sees them.
            for message in [WM_DROPFILES, WM_COPYGLOBALDATA, WM_COPYDATA] {
                ChangeWindowMessageFilterEx(window, message, MSGFLT_ALLOW, std::ptr::null_mut());
            }
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
