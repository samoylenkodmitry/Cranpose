use super::{DrawCacheBuilder, DrawCommand, Modifier};
use crate::modifier_nodes::DrawCommandElement;
use cranpose_ui_graphics::{DrawScope, DrawScopeDefault};
use std::rc::Rc;

impl Modifier {
    /// Draw around content.
    ///
    /// `draw_content()` splits drawing into behind (before) and overlay (after)
    /// phases. If `draw_content()` is never called, primitives are treated as
    /// overlay content.
    ///
    /// Example: `Modifier::empty().draw_with_content(|scope| { ... })`
    pub fn draw_with_content(self, f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |scope: &mut DrawScopeDefault| f(scope));
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::WithContent(func)));
        self.then(modifier)
    }

    /// Draw content behind.
    ///
    /// Example: `Modifier::empty().draw_behind(|scope| { ... })`
    pub fn draw_behind(self, f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |scope: &mut DrawScopeDefault| f(scope));
        let modifier = Self::with_element(DrawCommandElement::new(DrawCommand::Behind(func)));
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
