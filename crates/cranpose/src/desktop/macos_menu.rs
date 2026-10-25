//! The application menu on macOS, installed once the first frame is on screen.
//!
//! winit builds this menu when the application finishes launching, before it
//! lets the application create its window: on an Apple M5 that held the
//! gauntlet's window back 14 ms (median of six launches). The menu is the one
//! winit builds -- About, Services, Hide, Hide Others, Show All and Quit -- so
//! the application behaves as before once its first frame is presented.
#![expect(unsafe_code, reason = "Objective-C messages to AppKit")]

use objc2::{MainThreadMarker, MainThreadOnly, rc::Retained, sel};
use objc2_app_kit::{NSApplication, NSEventModifierFlags, NSMenu, NSMenuItem};
use objc2_foundation::{NSBundle, NSProcessInfo, NSString, ns_string};

/// Installs the application menu, unless the application has one already.
pub(crate) fn install_application_menu() {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let app = NSApplication::sharedApplication(mtm);
    if app.mainMenu().is_some() {
        return;
    }
    let process_name = NSBundle::mainBundle()
        .name()
        .unwrap_or_else(|| NSProcessInfo::processInfo().processName());
    let services = NSMenu::new(mtm);
    let services_item = item(mtm, ns_string!("Services"), None, ns_string!(""));
    services_item.setSubmenu(Some(&services));
    let hide_others = item(
        mtm,
        ns_string!("Hide Others"),
        Some(sel!(hideOtherApplications:)),
        ns_string!("h"),
    );
    hide_others
        .setKeyEquivalentModifierMask(NSEventModifierFlags::Option | NSEventModifierFlags::Command);
    let app_menu = NSMenu::new(mtm);
    for entry in [
        item(
            mtm,
            &ns_string!("About ").stringByAppendingString(&process_name),
            Some(sel!(orderFrontStandardAboutPanel:)),
            ns_string!(""),
        ),
        NSMenuItem::separatorItem(mtm),
        services_item,
        item(
            mtm,
            &ns_string!("Hide ").stringByAppendingString(&process_name),
            Some(sel!(hide:)),
            ns_string!("h"),
        ),
        hide_others,
        item(
            mtm,
            ns_string!("Show All"),
            Some(sel!(unhideAllApplications:)),
            ns_string!(""),
        ),
        NSMenuItem::separatorItem(mtm),
        item(
            mtm,
            &ns_string!("Quit ").stringByAppendingString(&process_name),
            Some(sel!(terminate:)),
            ns_string!("q"),
        ),
    ] {
        app_menu.addItem(&entry);
    }
    let app_item = NSMenuItem::new(mtm);
    app_item.setSubmenu(Some(&app_menu));
    let menubar = NSMenu::new(mtm);
    menubar.addItem(&app_item);
    app.setServicesMenu(Some(&services));
    app.setMainMenu(Some(&menubar));
}

fn item(
    mtm: MainThreadMarker,
    title: &NSString,
    action: Option<objc2::runtime::Sel>,
    key: &NSString,
) -> Retained<NSMenuItem> {
    // SAFETY: each action is a selector NSApplication responds to.
    unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(NSMenuItem::alloc(mtm), title, action, key)
    }
}
