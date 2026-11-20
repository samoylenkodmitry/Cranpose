//! Which window's text field takes the platform's keyboard: the routes from a root to the handler
//! that its platform gave it.

use std::{cell::RefCell, rc::Rc};

use cranpose_ui::PlatformTextInputHandler;

use crate::RootId;

#[derive(Default)]
pub(crate) struct TextInputRoutes {
    active: Option<RootId>,
    shown: Option<RootId>,
    handlers: Vec<(RootId, Rc<dyn PlatformTextInputHandler>)>,
}

impl TextInputRoutes {
    pub(crate) fn active(&self) -> RootId {
        self.active.unwrap_or(RootId::Primary)
    }

    pub(crate) fn set_active(&mut self, root: RootId) {
        self.active = Some(root);
    }

    pub(crate) fn set_handler(&mut self, root: RootId, handler: Rc<dyn PlatformTextInputHandler>) {
        self.remove(root);
        self.handlers.push((root, handler));
    }

    pub(crate) fn remove(&mut self, root: RootId) {
        self.handlers.retain(|(id, _)| *id != root);
        if self.shown == Some(root) {
            self.shown = None;
        }
    }

    fn handler(&self, root: RootId) -> Option<Rc<dyn PlatformTextInputHandler>> {
        self.handlers
            .iter()
            .find(|(id, _)| *id == root)
            .map(|(_, handler)| Rc::clone(handler))
    }

    fn take_show_target(&mut self) -> Option<Rc<dyn PlatformTextInputHandler>> {
        let active = self.active();
        let handler = self.handler(active);
        if handler.is_some() {
            self.shown = Some(active);
        }
        handler
    }

    fn take_hide_target(&mut self) -> Option<Rc<dyn PlatformTextInputHandler>> {
        let target = self.shown.take().unwrap_or_else(|| self.active());
        self.handler(target)
    }
}

pub(crate) struct TextInputRouter {
    pub(crate) routes: Rc<RefCell<TextInputRoutes>>,
}

impl PlatformTextInputHandler for TextInputRouter {
    fn show_keyboard(&self) {
        let handler = self.routes.borrow_mut().take_show_target();
        if let Some(handler) = handler {
            handler.show_keyboard();
        }
    }

    fn hide_keyboard(&self) {
        let handler = self.routes.borrow_mut().take_hide_target();
        if let Some(handler) = handler {
            handler.hide_keyboard();
        }
    }
}
