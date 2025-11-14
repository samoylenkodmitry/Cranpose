use super::{DrawCacheBuilder, DrawCommand, Modifier, Size};
use crate::modifier_nodes::DrawCommandElement;
use compose_ui_graphics::{DrawScope, DrawScopeDefault};
use std::rc::Rc;

impl Modifier {
    /// Draw content with overlay.
    ///
    /// Example: `Modifier::empty().draw_with_content(|scope| { ... })`
    pub fn draw_with_content(self, f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Overlay(func.clone())));
        self.then(modifier)
    }

    /// Draw content behind.
    ///
    /// Example: `Modifier::empty().draw_behind(|scope| { ... })`
    pub fn draw_behind(self, f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Behind(func.clone())));
        self.then(modifier)
    }

    /// Draw with cache.
    ///
    /// Example: `Modifier::empty().draw_with_cache(|builder| { ... })`
    pub fn draw_with_cache(self, build: impl FnOnce(&mut DrawCacheBuilder)) -> Self {
        let mut builder = DrawCacheBuilder::default();
        build(&mut builder);
        let commands = builder.finish();
        let modifier = Self::with_element(DrawCommandElement::from_commands(commands));
        self.then(modifier)
    }
}
