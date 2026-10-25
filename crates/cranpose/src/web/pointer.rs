//! Pointer input of the web canvas. The mouse, the pen and the first finger
//! drive the primary pointer. Every later finger is a secondary pointer that
//! joins the gesture the first finger started, as on Android and iOS, so a
//! second finger never moves the first finger's gesture. The mouse's left
//! button presses the primary pointer; its other buttons press as the
//! shell's other mouse buttons.

use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use cranpose_app_shell::{AppShell, PointerSource};
use cranpose_platform_web::WebPlatform;
use cranpose_render_wgpu::WgpuRenderer;
use cranpose_ui::PointerButton;
use cranpose_ui_graphics::Point;
use wasm_bindgen::{JsCast, prelude::*};
use web_sys::{HtmlCanvasElement, PointerEvent};

type App = Rc<RefCell<AppShell<WgpuRenderer>>>;
type Handler = fn(&Pointers, &PointerEvent, &mut AppShell<WgpuRenderer>, Point);

#[derive(Clone)]
struct Pointers {
    app: App,
    platform: Rc<RefCell<WebPlatform>>,
    canvas: HtmlCanvasElement,
    request_frame: Rc<dyn Fn()>,
    // The secondary fingers that are down. Lifting the first finger releases
    // them before it ends the gesture.
    secondaries: Rc<RefCell<Vec<u64>>>,
    // The mouse buttons last reported held, as `MouseEvent.buttons` bits.
    mouse_buttons: Rc<Cell<u16>>,
}

/// The mouse buttons with their bits in `MouseEvent.buttons`.
const MOUSE_BUTTONS: [(u16, PointerButton); 5] = [
    (1, PointerButton::Primary),
    (2, PointerButton::Secondary),
    (4, PointerButton::Middle),
    (8, PointerButton::Back),
    (16, PointerButton::Forward),
];

/// Sends the canvas's pointer events to `app`.
pub(crate) fn listen(
    canvas: &HtmlCanvasElement,
    app: App,
    platform: Rc<RefCell<WebPlatform>>,
    request_frame: Rc<dyn Fn()>,
) -> Result<(), JsValue> {
    let pointers = Pointers {
        app,
        platform,
        canvas: canvas.clone(),
        request_frame,
        secondaries: Rc::default(),
        mouse_buttons: Rc::default(),
    };
    pointers.on("pointermove", Pointers::moved)?;
    pointers.on("pointerdown", Pointers::pressed)?;
    pointers.on("pointerup", Pointers::released)?;
    pointers.on("pointercancel", Pointers::cancelled)?;
    // The right button reaches the app, so the browser's own menu stays off
    // the canvas.
    let keep_menu_off =
        Closure::wrap(Box::new(|event: web_sys::Event| event.prevent_default())
            as Box<dyn FnMut(web_sys::Event)>);
    canvas
        .add_event_listener_with_callback("contextmenu", keep_menu_off.as_ref().unchecked_ref())?;
    keep_menu_off.forget();
    Ok(())
}

fn is_mouse(event: &PointerEvent) -> bool {
    event.pointer_type() == "mouse"
}

pub(crate) fn web_pointer_source(event: &PointerEvent) -> PointerSource {
    match event.pointer_type().as_str() {
        "touch" => PointerSource::Touch,
        "pen" => PointerSource::Stylus,
        "mouse" => PointerSource::Mouse,
        _ => PointerSource::Unknown,
    }
}

// The shell's id of a finger after the first one. The shell keeps id 0 for
// the primary pointer.
fn secondary_id(event: &PointerEvent) -> Option<u64> {
    (!event.is_primary()).then(|| u64::from(event.pointer_id().unsigned_abs()) + 1)
}

impl Pointers {
    fn on(&self, name: &str, handle: Handler) -> Result<(), JsValue> {
        let pointers = self.clone();
        let closure = Closure::wrap(Box::new(move |event: PointerEvent| {
            event.prevent_default();
            let at = pointers
                .platform
                .borrow()
                .pointer_position(f64::from(event.offset_x()), f64::from(event.offset_y()));
            if let Ok(mut app) = pointers.app.try_borrow_mut() {
                app.set_pointer_source(web_pointer_source(&event));
                app.set_modifiers(super::web_modifiers(&event));
                handle(&pointers, &event, &mut app, at);
                (pointers.request_frame)();
            }
        }) as Box<dyn FnMut(PointerEvent)>);
        self.canvas
            .add_event_listener_with_callback(name, closure.as_ref().unchecked_ref())?;
        closure.forget();
        Ok(())
    }

    fn moved(&self, event: &PointerEvent, app: &mut AppShell<WgpuRenderer>, at: Point) {
        match secondary_id(event) {
            Some(id) => {
                app.secondary_pointer_moved(id, at.x, at.y, None);
            }
            None => {
                app.set_cursor(at.x, at.y);
                if is_mouse(event) {
                    self.mouse_buttons_changed(event, app, at);
                }
            }
        }
    }

    /// Presses and releases the mouse buttons whose bits changed since the
    /// event before: a browser reports the second button of a chord, and the
    /// release of every button but the last, in a pointermove.
    fn mouse_buttons_changed(
        &self,
        event: &PointerEvent,
        app: &mut AppShell<WgpuRenderer>,
        at: Point,
    ) {
        let held = event.buttons();
        let before = self.mouse_buttons.replace(held);
        for (bit, button) in MOUSE_BUTTONS {
            if (before ^ held) & bit == 0 {
                continue;
            }
            let event_time = app.realtime_pointer_event_time(None);
            let pressed = held & bit != 0;
            if pressed {
                app.set_cursor_at_event_time(at.x, at.y, event_time);
            }
            match (button, pressed) {
                (PointerButton::Primary, true) => {
                    app.pointer_pressed_at_event_time(event_time);
                }
                (PointerButton::Primary, false) => {
                    app.pointer_released_at_position_event_time(at.x, at.y, event_time);
                }
                (_, true) => {
                    app.primary().mouse_button_pressed(button);
                }
                (_, false) => {
                    app.primary().mouse_button_released(button);
                }
            }
        }
    }

    fn pressed(&self, event: &PointerEvent, app: &mut AppShell<WgpuRenderer>, at: Point) {
        let _ = self.canvas.set_pointer_capture(event.pointer_id());
        if is_mouse(event) {
            self.mouse_buttons_changed(event, app, at);
            return;
        }
        match secondary_id(event) {
            Some(id) => {
                self.secondaries.borrow_mut().push(id);
                app.secondary_pointer_pressed(id, at.x, at.y, None);
            }
            None => {
                let event_time = app.realtime_pointer_event_time(None);
                app.set_cursor_at_event_time(at.x, at.y, event_time);
                app.pointer_pressed_at_event_time(event_time);
            }
        }
    }

    fn released(&self, event: &PointerEvent, app: &mut AppShell<WgpuRenderer>, at: Point) {
        match secondary_id(event) {
            Some(id) => self.release_secondary(app, id, at),
            None if is_mouse(event) => self.mouse_buttons_changed(event, app, at),
            None => {
                let lifted = std::mem::take(&mut *self.secondaries.borrow_mut());
                for id in lifted {
                    app.secondary_pointer_released(id, at.x, at.y, None);
                }
                let event_time = app.realtime_pointer_event_time(None);
                app.pointer_released_at_position_event_time(at.x, at.y, event_time);
            }
        }
        let _ = self.canvas.release_pointer_capture(event.pointer_id());
    }

    fn cancelled(&self, event: &PointerEvent, app: &mut AppShell<WgpuRenderer>, at: Point) {
        match secondary_id(event) {
            Some(id) => self.release_secondary(app, id, at),
            None => {
                self.secondaries.borrow_mut().clear();
                self.mouse_buttons.set(0);
                app.cancel_gesture();
            }
        }
        let _ = self.canvas.release_pointer_capture(event.pointer_id());
    }

    fn release_secondary(&self, app: &mut AppShell<WgpuRenderer>, id: u64, at: Point) {
        self.secondaries.borrow_mut().retain(|&finger| finger != id);
        app.secondary_pointer_released(id, at.x, at.y, None);
    }
}
