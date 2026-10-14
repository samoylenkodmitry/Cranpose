//! A tray icon and a native menu bar for desktop apps: Compose Desktop's
//! `Tray` and `MenuBar`. A composition describes them with [`Tray`] and
//! [`MenuBar`]; the desktop shell builds them on its main thread before the
//! event loop waits, and runs an item's callback as it runs an event handler.
//!
//! The tray icon shows on macOS, Windows and Linux; on Linux it is a
//! StatusNotifierItem over D-Bus, which GNOME shows with the AppIndicator
//! extension. The menu bar is the macOS application menu bar, after the
//! application menu; on Windows and Linux [`MenuBar`] shows nothing yet.

use std::{
    rc::Rc,
    sync::{Mutex, Once, PoisonError},
};

use cranpose_app_shell::AppShell;
use cranpose_core::{DisposableEffect, SideEffect, remember};
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::{ImageBitmap, KeyCode, Modifiers, composable};
use muda::{
    CheckMenuItem, IsMenuItem, MenuId, PredefinedMenuItem, Submenu,
    accelerator::{Accelerator, CMD_OR_CTRL, Code, Modifiers as AcceleratorModifiers},
};
use tray_icon::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use winit::event_loop::EventLoopProxy;

/// The keys that run a menu item without opening its menu: Compose
/// Desktop's `KeyShortcut`. `modifiers.ctrl` is Command on macOS and Control
/// elsewhere, as in key events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct KeyShortcut {
    /// The key pressed with the modifiers.
    pub key: KeyCode,
    /// The modifiers held with the key.
    pub modifiers: Modifiers,
}

impl KeyShortcut {
    /// `key` with `modifiers` held.
    pub fn new(key: KeyCode, modifiers: Modifiers) -> Self {
        Self { key, modifiers }
    }
}

/// A menu item that runs `on_click` when chosen: Compose Desktop's `Item`.
#[derive(Clone)]
pub struct MenuItem {
    /// The text the menu shows.
    pub label: String,
    /// Whether the item can be chosen; a disabled item shows dimmed.
    pub enabled: bool,
    /// The keys that choose the item without opening its menu.
    pub shortcut: Option<KeyShortcut>,
    /// Runs when the item is chosen, as an event handler runs.
    pub on_click: Rc<dyn Fn()>,
}

impl MenuItem {
    /// An enabled item `label` without a shortcut.
    pub fn new(label: impl Into<String>, on_click: impl Fn() + 'static) -> Self {
        Self {
            label: label.into(),
            enabled: true,
            shortcut: None,
            on_click: Rc::new(on_click),
        }
    }

    /// The item with `enabled` set.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }

    /// The item with `shortcut`.
    pub fn shortcut(mut self, shortcut: KeyShortcut) -> Self {
        self.shortcut = Some(shortcut);
        self
    }
}

/// A menu item with a check mark: Compose Desktop's `CheckboxItem`.
#[derive(Clone)]
pub struct CheckItem {
    /// The text the menu shows.
    pub label: String,
    /// Whether the check mark shows.
    pub checked: bool,
    /// Whether the item can be chosen.
    pub enabled: bool,
    /// Runs with the state a click asks for. The mark shows `checked` again
    /// at the next composition, so it stays where the app keeps it.
    pub on_change: Rc<dyn Fn(bool)>,
}

impl CheckItem {
    /// An enabled item `label`, checked when `checked` is set.
    pub fn new(
        label: impl Into<String>,
        checked: bool,
        on_change: impl Fn(bool) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            checked,
            enabled: true,
            on_change: Rc::new(on_change),
        }
    }

    /// The item with `enabled` set.
    pub fn enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// One line of a menu.
#[derive(Clone)]
pub enum MenuEntry {
    /// An item that runs a callback.
    Item(MenuItem),
    /// An item with a check mark.
    Check(CheckItem),
    /// A line between groups of items.
    Separator,
    /// An item that opens a menu of its own.
    Submenu(Menu),
}

/// A titled list of entries: a menu of the menu bar or a submenu. Compose
/// Desktop's `Menu`.
#[derive(Clone)]
pub struct Menu {
    /// The title the menu bar or the parent menu shows.
    pub title: String,
    /// The entries, top to bottom.
    pub entries: Vec<MenuEntry>,
}

impl Menu {
    /// An empty menu `title`.
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            entries: Vec::new(),
        }
    }

    /// The menu with `item` added at the end.
    pub fn item(mut self, item: MenuItem) -> Self {
        self.entries.push(MenuEntry::Item(item));
        self
    }

    /// The menu with `item` added at the end.
    pub fn check(mut self, item: CheckItem) -> Self {
        self.entries.push(MenuEntry::Check(item));
        self
    }

    /// The menu with a separator added at the end.
    pub fn separator(mut self) -> Self {
        self.entries.push(MenuEntry::Separator);
        self
    }

    /// The menu with `menu` added at the end as a submenu.
    pub fn submenu(mut self, menu: Menu) -> Self {
        self.entries.push(MenuEntry::Submenu(menu));
        self
    }
}

/// What a tray icon shows and does: Compose Desktop's `Tray` arguments.
#[derive(Clone)]
pub struct TraySpec {
    /// The icon, in its pixel size; 32 by 32 or 64 by 64 suits most trays.
    pub icon: ImageBitmap,
    /// The text that shows while the pointer rests on the icon.
    pub tooltip: String,
    /// The menu a click on the icon opens: a right click when `on_action` is
    /// set, else any click.
    pub menu: Vec<MenuEntry>,
    /// Runs on a left click of the icon. Linux trays may not report clicks.
    pub on_action: Option<Rc<dyn Fn()>>,
}

impl TraySpec {
    /// A tray icon with `tooltip` and no menu.
    pub fn new(icon: ImageBitmap, tooltip: impl Into<String>) -> Self {
        Self {
            icon,
            tooltip: tooltip.into(),
            menu: Vec::new(),
            on_action: None,
        }
    }

    /// The tray with `menu` as its menu.
    pub fn menu(mut self, menu: Vec<MenuEntry>) -> Self {
        self.menu = menu;
        self
    }

    /// The tray with `on_action` run on a left click.
    pub fn on_action(mut self, on_action: impl Fn() + 'static) -> Self {
        self.on_action = Some(Rc::new(on_action));
        self
    }
}

/// Shows a tray icon while this composable is in the composition. The last
/// `Tray` composed wins while two are.
#[composable(no_skip)]
pub fn Tray(spec: TraySpec) {
    let owner = remember(next_owner).with(|owner| *owner);
    SideEffect(move || with_requests(|requests| requests.set_tray(owner, spec)));
    DisposableEffect(owner, move |scope| {
        scope.on_dispose(move || with_requests(|requests| requests.clear_tray(owner)))
    });
}

/// Shows `menus` in the menu bar while this composable is in the
/// composition: on macOS after the application menu, which keeps About,
/// Hide and Quit. The last `MenuBar` composed wins while two are.
#[composable(no_skip)]
pub fn MenuBar(menus: Vec<Menu>) {
    let owner = remember(next_owner).with(|owner| *owner);
    SideEffect(move || with_requests(|requests| requests.set_menu_bar(owner, menus)));
    DisposableEffect(owner, move |scope| {
        scope.on_dispose(move || with_requests(|requests| requests.clear_menu_bar(owner)))
    });
}

fn next_owner() -> u64 {
    crate::thread_state::with(|state| {
        let owner = state.native_menus_next_owner.get() + 1;
        state.native_menus_next_owner.set(owner);
        owner
    })
}

fn with_requests<R>(f: impl FnOnce(&mut MenuRequests) -> R) -> R {
    crate::thread_state::with(|state| f(&mut state.native_menus.borrow_mut()))
}

/// What the composition asks the shell to show, and whether that changed
/// since the shell last built it.
#[derive(Default)]
pub(crate) struct MenuRequests {
    tray: Option<(u64, TraySpec)>,
    menu_bar: Option<(u64, Vec<Menu>)>,
    revision: u64,
}

impl MenuRequests {
    fn set_tray(&mut self, owner: u64, spec: TraySpec) {
        let same = self
            .tray
            .as_ref()
            .is_some_and(|(held, current)| *held == owner && same_tray(current, &spec));
        self.tray = Some((owner, spec));
        if !same {
            self.revision += 1;
        }
    }

    fn clear_tray(&mut self, owner: u64) {
        if self.tray.as_ref().is_some_and(|(held, _)| *held == owner) {
            self.tray = None;
            self.revision += 1;
        }
    }

    fn set_menu_bar(&mut self, owner: u64, menus: Vec<Menu>) {
        let same = self.menu_bar.as_ref().is_some_and(|(held, current)| {
            *held == owner
                && current.len() == menus.len()
                && current.iter().zip(&menus).all(|(a, b)| same_menu(a, b))
        });
        self.menu_bar = Some((owner, menus));
        if !same {
            self.revision += 1;
        }
    }

    /// The id of the item that `labels` name: the menu's title, then the
    /// labels down to the item. The tray menu has no title.
    #[cfg(feature = "robot")]
    fn id_of(&self, tray: bool, labels: &[String]) -> Option<String> {
        use std::fmt::Write;

        let (mut path, mut entries, rest) = if tray {
            ("tray".to_owned(), &self.tray.as_ref()?.1.menu[..], labels)
        } else {
            let (title, rest) = labels.split_first()?;
            let menus = &self.menu_bar.as_ref()?.1;
            let index = menus.iter().position(|menu| &menu.title == title)?;
            (format!("bar/{index}"), &menus[index].entries[..], rest)
        };
        for label in rest {
            let index = entries
                .iter()
                .position(|entry| entry_label(entry) == Some(label.as_str()))?;
            let _ = write!(path, "/{index}");
            if let MenuEntry::Submenu(menu) = &entries[index] {
                entries = &menu.entries;
            }
        }
        Some(path)
    }

    /// The entry at `path`: `tray/2` or `bar/1/0`, an index per menu level.
    fn entry(&self, path: &str) -> Option<&MenuEntry> {
        let mut parts = path.split('/');
        let mut entries: &[MenuEntry] = match parts.next()? {
            "tray" => &self.tray.as_ref()?.1.menu,
            "bar" => {
                &self
                    .menu_bar
                    .as_ref()?
                    .1
                    .get(parts.next()?.parse::<usize>().ok()?)?
                    .entries
            }
            _ => return None,
        };
        let mut entry = None;
        for part in parts {
            let found = entries.get(part.parse::<usize>().ok()?)?;
            if let MenuEntry::Submenu(menu) = found {
                entries = &menu.entries;
            }
            entry = Some(found);
        }
        entry
    }

    fn clear_menu_bar(&mut self, owner: u64) {
        if self
            .menu_bar
            .as_ref()
            .is_some_and(|(held, _)| *held == owner)
        {
            self.menu_bar = None;
            self.revision += 1;
        }
    }
}

#[cfg(feature = "robot")]
fn entry_label(entry: &MenuEntry) -> Option<&str> {
    match entry {
        MenuEntry::Item(item) => Some(&item.label),
        MenuEntry::Check(item) => Some(&item.label),
        MenuEntry::Submenu(menu) => Some(&menu.title),
        MenuEntry::Separator => None,
    }
}

/// Whether the native tray built for `a` shows `b` too: everything but the
/// callbacks, which a click reads from the requests.
fn same_tray(a: &TraySpec, b: &TraySpec) -> bool {
    a.icon == b.icon
        && a.tooltip == b.tooltip
        && a.on_action.is_some() == b.on_action.is_some()
        && same_entries(&a.menu, &b.menu)
}

fn same_menu(a: &Menu, b: &Menu) -> bool {
    a.title == b.title && same_entries(&a.entries, &b.entries)
}

fn same_entries(a: &[MenuEntry], b: &[MenuEntry]) -> bool {
    a.len() == b.len()
        && a.iter().zip(b).all(|pair| match pair {
            (MenuEntry::Item(a), MenuEntry::Item(b)) => {
                a.label == b.label && a.enabled == b.enabled && a.shortcut == b.shortcut
            }
            (MenuEntry::Check(a), MenuEntry::Check(b)) => {
                a.label == b.label && a.enabled == b.enabled && a.checked == b.checked
            }
            (MenuEntry::Separator, MenuEntry::Separator) => true,
            (MenuEntry::Submenu(a), MenuEntry::Submenu(b)) => same_menu(a, b),
            _ => false,
        })
}

/// A native menu or tray event, queued by the platform's handler until the
/// shell's main thread takes it.
enum NativeEvent {
    Menu(MenuId),
    TrayAction,
}

/// Events the platform reported, from any thread.
static EVENTS: Mutex<Vec<NativeEvent>> = Mutex::new(Vec::new());
/// Wakes the running event loop when an event is queued.
static WAKER: Mutex<Option<EventLoopProxy>> = Mutex::new(None);
static HANDLERS: Once = Once::new();

fn queue(event: NativeEvent) {
    EVENTS
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .push(event);
    if let Some(waker) = &*WAKER.lock().unwrap_or_else(PoisonError::into_inner) {
        waker.wake_up();
    }
}

/// The tray icon and menu bar the shell built, and the actions their items
/// run.
#[derive(Default)]
pub(crate) struct NativeMenus {
    built_revision: u64,
    /// A click flipped a check mark natively: build again to show the state
    /// the app keeps.
    stale: bool,
    tray: Option<tray_icon::TrayIcon>,
    #[cfg(target_os = "macos")]
    menu_bar: Option<muda::Menu>,
}

impl NativeMenus {
    /// Builds what the composition asks for, if it changed, and runs the
    /// actions of the items chosen since the last call. `menu_bar_due`
    /// holds the menu bar back until the application menu is installed.
    pub(crate) fn serve(
        &mut self,
        app: &mut AppShell<WgpuRenderer>,
        proxy: &EventLoopProxy,
        menu_bar_due: bool,
    ) {
        *WAKER.lock().unwrap_or_else(PoisonError::into_inner) = Some(proxy.clone());
        HANDLERS.call_once(|| {
            muda::MenuEvent::set_event_handler(Some(|event: muda::MenuEvent| {
                queue(NativeEvent::Menu(event.id));
            }));
            TrayIconEvent::set_event_handler(Some(|event: TrayIconEvent| {
                if let TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                } = event
                {
                    queue(NativeEvent::TrayAction);
                }
            }));
        });
        let revision = with_requests(|requests| requests.revision);
        if (revision != self.built_revision || self.stale) && menu_bar_due {
            self.build();
            self.built_revision = revision;
            self.stale = false;
        }
        let events = std::mem::take(&mut *EVENTS.lock().unwrap_or_else(PoisonError::into_inner));
        for event in events {
            self.run(event, app, proxy);
        }
    }

    fn run(
        &mut self,
        event: NativeEvent,
        app: &mut AppShell<WgpuRenderer>,
        proxy: &EventLoopProxy,
    ) {
        let callback: Option<Rc<dyn Fn()>> = with_requests(|requests| match &event {
            NativeEvent::TrayAction => requests.tray.as_ref()?.1.on_action.clone(),
            NativeEvent::Menu(id) => match requests.entry(id.as_ref())? {
                MenuEntry::Item(item) => Some(Rc::clone(&item.on_click)),
                MenuEntry::Check(item) => {
                    // The native mark flipped with the click.
                    self.stale = true;
                    let on_change = Rc::clone(&item.on_change);
                    let asked = !item.checked;
                    let callback: Rc<dyn Fn()> = Rc::new(move || on_change(asked));
                    Some(callback)
                }
                MenuEntry::Separator | MenuEntry::Submenu(_) => None,
            },
        });
        let Some(callback) = callback else {
            return;
        };
        if let Err(error) =
            crate::desktop_integration::run_as_event_handler(app, proxy, || callback())
        {
            log::warn!("cranpose: a menu action did not apply: {error}");
        }
    }

    /// Chooses the item that `labels` name, as a click on it does: through
    /// the native menu bar on macOS, else through the event a click queues.
    #[cfg(feature = "robot")]
    pub(crate) fn choose(&self, tray: bool, labels: &[String]) -> Result<(), String> {
        #[cfg(target_os = "macos")]
        if !tray && self.menu_bar.is_some() {
            return choose_in_menu_bar(labels);
        }
        let id = with_requests(|requests| requests.id_of(tray, labels))
            .ok_or_else(|| format!("no menu item {labels:?}"))?;
        queue(NativeEvent::Menu(MenuId::new(id)));
        Ok(())
    }

    fn build(&mut self) {
        let (tray, menu_bar) = with_requests(|requests| {
            (
                requests.tray.as_ref().map(|(_, spec)| spec.clone()),
                requests.menu_bar.as_ref().map(|(_, menus)| menus.clone()),
            )
        });
        self.build_tray(tray);
        self.build_menu_bar(menu_bar);
    }

    fn build_tray(&mut self, spec: Option<TraySpec>) {
        let Some(spec) = spec else {
            self.tray = None;
            return;
        };
        let icon = match tray_icon::Icon::from_rgba(
            spec.icon.rgba8_pixels().into_owned(),
            spec.icon.width(),
            spec.icon.height(),
        ) {
            Ok(icon) => icon,
            Err(error) => {
                log::warn!("cranpose: the tray icon is unusable: {error}");
                return;
            }
        };
        let menu = muda::Menu::new();
        if let Err(error) = menu.append_items(&item_refs(&build_items(&spec.menu, "tray"))) {
            log::warn!("cranpose: the tray menu could not be built: {error}");
        }
        let menu_on_left_click = spec.on_action.is_none();
        match &self.tray {
            Some(tray) => {
                if let Err(error) = tray.set_icon(Some(icon)) {
                    log::warn!("cranpose: the tray icon could not change: {error}");
                }
                if let Err(error) = tray.set_tooltip(Some(&spec.tooltip)) {
                    log::warn!("cranpose: the tray tooltip could not change: {error}");
                }
                tray.set_menu(Some(Box::new(menu)));
                tray.set_show_menu_on_left_click(menu_on_left_click);
            }
            None => {
                match TrayIconBuilder::new()
                    .with_icon(icon)
                    .with_tooltip(&spec.tooltip)
                    .with_menu(Box::new(menu))
                    .with_menu_on_left_click(menu_on_left_click)
                    .build()
                {
                    Ok(tray) => self.tray = Some(tray),
                    Err(error) => log::warn!("cranpose: the tray icon could not show: {error}"),
                }
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn build_menu_bar(&mut self, menus: Option<Vec<Menu>>) {
        let Some(menus) = menus else {
            if let Some(menu_bar) = self.menu_bar.take() {
                menu_bar.remove_for_nsapp();
                crate::macos_menu::install_application_menu();
            }
            return;
        };
        let menu_bar = muda::Menu::new();
        let application = Submenu::with_items(
            "",
            true,
            &[
                &PredefinedMenuItem::about(None, None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::services(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::hide(None),
                &PredefinedMenuItem::hide_others(None),
                &PredefinedMenuItem::show_all(None),
                &PredefinedMenuItem::separator(),
                &PredefinedMenuItem::quit(None),
            ],
        );
        let mut items: Vec<Box<dyn IsMenuItem>> = Vec::with_capacity(menus.len() + 1);
        match application {
            Ok(application) => items.push(Box::new(application)),
            Err(error) => log::warn!("cranpose: the application menu could not be built: {error}"),
        }
        for (index, menu) in menus.iter().enumerate() {
            if let Some(submenu) = build_submenu(menu, &format!("bar/{index}")) {
                items.push(Box::new(submenu));
            }
        }
        if let Err(error) = menu_bar.append_items(&item_refs(&items)) {
            log::warn!("cranpose: the menu bar could not be built: {error}");
        }
        menu_bar.init_for_nsapp();
        self.menu_bar = Some(menu_bar);
    }

    #[cfg(not(target_os = "macos"))]
    fn build_menu_bar(&mut self, _menus: Option<Vec<Menu>>) {}
}

/// The native items of `entries`, each with its path under `path` as its id.
fn build_items(entries: &[MenuEntry], path: &str) -> Vec<Box<dyn IsMenuItem>> {
    let mut items: Vec<Box<dyn IsMenuItem>> = Vec::with_capacity(entries.len());
    for (index, entry) in entries.iter().enumerate() {
        let id = MenuId::new(format!("{path}/{index}"));
        match entry {
            MenuEntry::Item(item) => {
                items.push(Box::new(muda::MenuItem::with_id(
                    id,
                    &item.label,
                    item.enabled,
                    item.shortcut.and_then(accelerator),
                )));
            }
            MenuEntry::Check(item) => {
                items.push(Box::new(CheckMenuItem::with_id(
                    id,
                    &item.label,
                    item.enabled,
                    item.checked,
                    None,
                )));
            }
            MenuEntry::Separator => items.push(Box::new(PredefinedMenuItem::separator())),
            MenuEntry::Submenu(menu) => {
                if let Some(submenu) = build_submenu(menu, &format!("{path}/{index}")) {
                    items.push(Box::new(submenu));
                }
            }
        }
    }
    items
}

fn build_submenu(menu: &Menu, path: &str) -> Option<Submenu> {
    let items = build_items(&menu.entries, path);
    match Submenu::with_id_and_items(MenuId::new(path), &menu.title, true, &item_refs(&items)) {
        Ok(submenu) => Some(submenu),
        Err(error) => {
            log::warn!(
                "cranpose: the menu {:?} could not be built: {error}",
                menu.title
            );
            None
        }
    }
}

fn item_refs(items: &[Box<dyn IsMenuItem>]) -> Vec<&dyn IsMenuItem> {
    items.iter().map(Box::as_ref).collect()
}

/// The native accelerator of `shortcut`, or `None` for a key a menu cannot
/// take.
fn accelerator(shortcut: KeyShortcut) -> Option<Accelerator> {
    let modifiers = shortcut.modifiers;
    let mut native = AcceleratorModifiers::empty();
    for (held, flag) in [
        (modifiers.ctrl, CMD_OR_CTRL),
        (modifiers.shift, AcceleratorModifiers::SHIFT),
        (modifiers.alt, AcceleratorModifiers::ALT),
        (modifiers.meta, AcceleratorModifiers::META),
    ] {
        if held {
            native |= flag;
        }
    }
    Some(Accelerator::new(native, key_code(shortcut.key)?))
}

fn key_code(key: KeyCode) -> Option<Code> {
    use KeyCode as K;
    Some(match key {
        K::A => Code::KeyA,
        K::B => Code::KeyB,
        K::C => Code::KeyC,
        K::D => Code::KeyD,
        K::E => Code::KeyE,
        K::F => Code::KeyF,
        K::G => Code::KeyG,
        K::H => Code::KeyH,
        K::I => Code::KeyI,
        K::J => Code::KeyJ,
        K::K => Code::KeyK,
        K::L => Code::KeyL,
        K::M => Code::KeyM,
        K::N => Code::KeyN,
        K::O => Code::KeyO,
        K::P => Code::KeyP,
        K::Q => Code::KeyQ,
        K::R => Code::KeyR,
        K::S => Code::KeyS,
        K::T => Code::KeyT,
        K::U => Code::KeyU,
        K::V => Code::KeyV,
        K::W => Code::KeyW,
        K::X => Code::KeyX,
        K::Y => Code::KeyY,
        K::Z => Code::KeyZ,
        K::Digit0 => Code::Digit0,
        K::Digit1 => Code::Digit1,
        K::Digit2 => Code::Digit2,
        K::Digit3 => Code::Digit3,
        K::Digit4 => Code::Digit4,
        K::Digit5 => Code::Digit5,
        K::Digit6 => Code::Digit6,
        K::Digit7 => Code::Digit7,
        K::Digit8 => Code::Digit8,
        K::Digit9 => Code::Digit9,
        K::F1 => Code::F1,
        K::F2 => Code::F2,
        K::F3 => Code::F3,
        K::F4 => Code::F4,
        K::F5 => Code::F5,
        K::F6 => Code::F6,
        K::F7 => Code::F7,
        K::F8 => Code::F8,
        K::F9 => Code::F9,
        K::F10 => Code::F10,
        K::F11 => Code::F11,
        K::F12 => Code::F12,
        K::ArrowUp => Code::ArrowUp,
        K::ArrowDown => Code::ArrowDown,
        K::ArrowLeft => Code::ArrowLeft,
        K::ArrowRight => Code::ArrowRight,
        K::Home => Code::Home,
        K::End => Code::End,
        K::PageUp => Code::PageUp,
        K::PageDown => Code::PageDown,
        K::Backspace => Code::Backspace,
        K::Delete => Code::Delete,
        K::Enter => Code::Enter,
        K::Tab => Code::Tab,
        K::Space => Code::Space,
        K::Escape => Code::Escape,
        K::Minus => Code::Minus,
        K::Equal => Code::Equal,
        K::BracketLeft => Code::BracketLeft,
        K::BracketRight => Code::BracketRight,
        K::Backslash => Code::Backslash,
        K::Semicolon => Code::Semicolon,
        K::Quote => Code::Quote,
        K::Comma => Code::Comma,
        K::Period => Code::Period,
        K::Slash => Code::Slash,
        K::Backquote => Code::Backquote,
        _ => return None,
    })
}

/// Performs the action of the macOS menu bar item that `labels` name, as
/// AppKit does when the user chooses it.
#[cfg(all(feature = "robot", target_os = "macos"))]
fn choose_in_menu_bar(labels: &[String]) -> Result<(), String> {
    use objc2::MainThreadMarker;
    use objc2_app_kit::NSApplication;
    use objc2_foundation::NSString;

    let mtm = MainThreadMarker::new().ok_or("the menu bar answers on the main thread")?;
    let mut menu = NSApplication::sharedApplication(mtm)
        .mainMenu()
        .ok_or("no menu bar")?;
    let (last, parents) = labels.split_last().ok_or("no menu item named")?;
    for title in parents {
        menu = menu
            .itemWithTitle(&NSString::from_str(title))
            .and_then(|item| item.submenu())
            .ok_or_else(|| format!("no menu {title:?}"))?;
    }
    let index = menu.indexOfItemWithTitle(&NSString::from_str(last));
    if index < 0 {
        return Err(format!("no menu item {last:?}"));
    }
    menu.performActionForItemAtIndex(index);
    Ok(())
}
