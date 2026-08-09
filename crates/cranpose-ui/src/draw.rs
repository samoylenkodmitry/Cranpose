use std::rc::Rc;

use crate::modifier::Size;
use crate::text::AppContextTextMeasurer;
use cranpose_ui_graphics::{DrawPrimitive, DrawScope, DrawScopeDefault};

/// A draw command records into a scope the CONSUMER provides, instead of
/// returning a freshly allocated primitive vector. This is the boundary that
/// lets recording storage live with the command's stable identity and be
/// reused frame over frame (the retained draw-command recorder), rather than
/// being reallocated, moved, and dropped on every execution.
pub type DrawCommandFn = Rc<dyn Fn(&mut DrawScopeDefault)>;

#[derive(Clone)]
pub enum DrawCommand {
    Behind(DrawCommandFn),
    WithContent(DrawCommandFn),
    Overlay(DrawCommandFn),
}

/// The scope every framework-run draw command records into: sized by the
/// consumer, measuring text with the app's fonts.
pub fn command_draw_scope(size: Size) -> DrawScopeDefault {
    DrawScopeDefault::with_text_measurer(size, AppContextTextMeasurer::shared())
}

/// [`command_draw_scope`] recording into storage the consumer already owns —
/// the retained-recorder path where a command's buffer keeps its capacity
/// across frames.
pub fn command_draw_scope_reusing(size: Size, storage: Vec<DrawPrimitive>) -> DrawScopeDefault {
    DrawScopeDefault::with_text_measurer_reusing(size, AppContextTextMeasurer::shared(), storage)
}

#[derive(Default, Clone)]
pub struct DrawCacheBuilder {
    behind: Vec<DrawCommandFn>,
    with_content: Vec<DrawCommandFn>,
    overlay: Vec<DrawCommandFn>,
}

impl DrawCacheBuilder {
    pub fn on_draw_behind(&mut self, f: impl Fn(&mut dyn DrawScope) + 'static) {
        self.behind
            .push(Rc::new(move |scope: &mut DrawScopeDefault| f(scope)));
    }

    pub fn on_draw_with_content(&mut self, f: impl Fn(&mut dyn DrawScope) + 'static) {
        self.with_content
            .push(Rc::new(move |scope: &mut DrawScopeDefault| f(scope)));
    }

    pub fn finish(self) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        commands.extend(self.behind.into_iter().map(DrawCommand::Behind));
        commands.extend(self.with_content.into_iter().map(DrawCommand::WithContent));
        commands.extend(self.overlay.into_iter().map(DrawCommand::Overlay));
        commands
    }
}

pub fn execute_draw_commands(commands: &[DrawCommand], size: Size) -> Vec<DrawPrimitive> {
    let mut primitives = Vec::new();
    for command in commands {
        match command {
            DrawCommand::Behind(f) | DrawCommand::WithContent(f) | DrawCommand::Overlay(f) => {
                let mut scope = command_draw_scope(size);
                f(&mut scope);
                primitives.extend(scope.into_primitives());
            }
        }
    }
    primitives
}
