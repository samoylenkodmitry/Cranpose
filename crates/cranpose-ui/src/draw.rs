use std::rc::Rc;

use cranpose_ui_graphics::{CommandRecording, DrawPrimitive, DrawScope, DrawScopeDefault};

use crate::{modifier::Size, text::AppContextTextMeasurer};

/// A draw command records into a scope the CONSUMER provides, instead of
/// returning a freshly allocated primitive vector. This is the boundary that
/// lets recording storage live with the command's stable identity and be
/// reused frame over frame (the retained draw-command recorder), rather than
/// being reallocated, moved, and dropped on every execution.
pub type DrawCommandFn = Rc<dyn Fn(&mut DrawScopeDefault)>;

/// A draw command and the phase of the node's drawing in which it runs.
#[derive(Clone)]
pub enum DrawCommand {
    /// Records drawing that appears behind the node's content.
    Behind(DrawCommandFn),
    /// Records drawing around the node's content. A `draw_content` call in it marks where
    /// the content draws, as in Compose's `drawWithContent`.
    WithContent(DrawCommandFn),
    /// Records drawing that appears in front of the node's content.
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
pub fn command_draw_scope_reusing(size: Size, storage: CommandRecording) -> DrawScopeDefault {
    DrawScopeDefault::with_text_measurer_reusing(size, AppContextTextMeasurer::shared(), storage)
}

/// Collects the draw commands that a `Modifier::draw_with_cache` block records.
#[derive(Default, Clone)]
pub struct DrawCacheBuilder {
    behind: Vec<DrawCommandFn>,
    with_content: Vec<DrawCommandFn>,
    overlay: Vec<DrawCommandFn>,
}

impl DrawCacheBuilder {
    /// Records drawing that appears behind the node's content.
    pub fn on_draw_behind(&mut self, f: impl Fn(&mut dyn DrawScope) + 'static) {
        self.behind
            .push(Rc::new(move |scope: &mut DrawScopeDefault| f(scope)));
    }

    /// Returns the recorded commands in the order behind, with-content, overlay.
    pub fn finish(self) -> Vec<DrawCommand> {
        let mut commands = Vec::new();
        commands.extend(self.behind.into_iter().map(DrawCommand::Behind));
        commands.extend(self.with_content.into_iter().map(DrawCommand::WithContent));
        commands.extend(self.overlay.into_iter().map(DrawCommand::Overlay));
        commands
    }
}

/// Runs every command at `size` and returns the primitives they record, in command order.
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
