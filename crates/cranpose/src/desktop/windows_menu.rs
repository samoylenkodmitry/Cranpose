//! The menu bar of a window on Windows. muda attaches it to the window's
//! `HWND` and handles its clicks; the shortcuts it shows reach it through
//! the event loop's message hook, which hands each message to
//! `TranslateAcceleratorW` first.
#![expect(unsafe_code, reason = "calls into the Win32 API")]

use std::{
    ffi::c_void,
    sync::atomic::{AtomicIsize, Ordering},
};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use windows_sys::Win32::UI::WindowsAndMessaging::{MSG, TranslateAcceleratorW};
use winit::window::Window;

/// The accelerator table of the menu bar shown, `0` while there is none.
static ACCELERATORS: AtomicIsize = AtomicIsize::new(0);
/// The window that shows the menu bar.
static MENU_WINDOW: AtomicIsize = AtomicIsize::new(0);

/// Shows `menu` as the menu bar of `window`, and answers the window's
/// `HWND` for [`detach`].
pub(crate) fn attach(menu: &muda::Menu, window: &dyn Window) -> Option<isize> {
    let handle = window.window_handle().ok()?;
    let RawWindowHandle::Win32(handle) = handle.as_raw() else {
        return None;
    };
    let hwnd = handle.hwnd.get();
    // SAFETY: the handle is the live window winit gave the shell.
    if let Err(error) = unsafe { menu.init_for_hwnd(hwnd) } {
        log::warn!("cranpose: the menu bar could not show: {error}");
        return None;
    }
    ACCELERATORS.store(menu.haccel(), Ordering::Relaxed);
    MENU_WINDOW.store(hwnd, Ordering::Relaxed);
    Some(hwnd)
}

/// Takes `menu` off the window `hwnd` that [`attach`] gave it.
pub(crate) fn detach(menu: &muda::Menu, hwnd: isize) {
    ACCELERATORS.store(0, Ordering::Relaxed);
    MENU_WINDOW.store(0, Ordering::Relaxed);
    // SAFETY: `hwnd` is the window the menu was attached to, which the shell
    // still holds.
    if let Err(error) = unsafe { menu.remove_for_hwnd(hwnd) } {
        log::warn!("cranpose: the menu bar could not go: {error}");
    }
}

/// winit's message hook: whether a shortcut of the menu bar took `message`,
/// which then goes no further.
pub(crate) fn translate_shortcut(message: *const c_void) -> bool {
    let accelerators = ACCELERATORS.load(Ordering::Relaxed);
    if accelerators == 0 || message.is_null() {
        return false;
    }
    let window = MENU_WINDOW.load(Ordering::Relaxed);
    // SAFETY: winit hands the hook the message it is about to dispatch, and
    // the table lives as long as the menu bar that set it.
    unsafe {
        TranslateAcceleratorW(
            window as *mut c_void,
            accelerators as *mut c_void,
            message.cast::<MSG>(),
        ) != 0
    }
}

/// Chooses the menu bar item that `labels` name, as a click does: the
/// window gets the item's `WM_COMMAND`.
#[cfg(feature = "robot")]
pub(crate) fn choose(labels: &[String]) -> Result<(), String> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMenu, GetMenuItemID, GetSubMenu, SendMessageW, WM_COMMAND,
    };

    let window = MENU_WINDOW.load(Ordering::Relaxed);
    if window == 0 {
        return Err("no menu bar shows".to_owned());
    }
    let (last, parents) = labels.split_last().ok_or("no menu item named")?;
    // SAFETY: the window shows the menu bar, so its menus are live.
    let mut menu = unsafe { GetMenu(window as *mut c_void) };
    for title in parents {
        let position = position_of(menu, title).ok_or_else(|| format!("no menu {title:?}"))?;
        // SAFETY: `position` is an item of `menu`.
        menu = unsafe { GetSubMenu(menu, position) };
        if menu.is_null() {
            return Err(format!("{title:?} opens no menu"));
        }
    }
    let position = position_of(menu, last).ok_or_else(|| format!("no menu item {last:?}"))?;
    // SAFETY: as above.
    let id = unsafe { GetMenuItemID(menu, position) };
    if id == u32::MAX {
        return Err(format!("{last:?} opens a menu"));
    }
    // SAFETY: the window is live; muda's subclass answers the command.
    unsafe { SendMessageW(window as *mut c_void, WM_COMMAND, id as usize, 0) };
    Ok(())
}

/// Where the item labelled `label` sits in `menu`: its text before the tab
/// that leads its shortcut.
#[cfg(feature = "robot")]
fn position_of(
    menu: windows_sys::Win32::UI::WindowsAndMessaging::HMENU,
    label: &str,
) -> Option<i32> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        GetMenuItemCount, GetMenuStringW, MF_BYPOSITION,
    };

    if menu.is_null() {
        return None;
    }
    // SAFETY: `menu` is live; each call reads one item's text into `text`.
    let count = unsafe { GetMenuItemCount(menu) };
    let mut text = [0u16; 256];
    (0..count).find(|&position| {
        let length = unsafe {
            GetMenuStringW(
                menu,
                position as u32,
                text.as_mut_ptr(),
                text.len() as i32,
                MF_BYPOSITION,
            )
        };
        let shown = String::from_utf16_lossy(&text[..length.max(0) as usize]);
        shown.split('\t').next() == Some(label)
    })
}
