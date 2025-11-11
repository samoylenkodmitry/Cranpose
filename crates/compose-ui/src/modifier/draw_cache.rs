use super::{DrawCacheBuilder, DrawCommand, Modifier, Size};
use crate::modifier_nodes::DrawCommandElement;
use compose_ui_graphics::{DrawScope, DrawScopeDefault};
use std::rc::Rc;

impl Modifier {
    pub fn draw_with_content(f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_element(DrawCommandElement::new(DrawCommand::Overlay(func.clone())))
    }

    pub fn draw_behind(f: impl Fn(&mut dyn DrawScope) + 'static) -> Self {
        let func = Rc::new(move |size: Size| {
            let mut scope = DrawScopeDefault::new(size);
            f(&mut scope);
            scope.into_primitives()
        });
        Self::with_element(DrawCommandElement::new(DrawCommand::Behind(func.clone())))
    }

    pub fn draw_with_cache(build: impl FnOnce(&mut DrawCacheBuilder)) -> Self {
        let mut builder = DrawCacheBuilder::default();
        build(&mut builder);
        let commands = builder.finish();
        Self::with_element(DrawCommandElement::from_commands(commands))
    }
}
