//! The pass that composes a subcomposition's slot where the subcomposition
//! runs.

use std::rc::Rc;

use super::{
    CapturedCompositionContext, Composer, ComposerRuntimeState, SubcomposeFrame, SubcomposeSite,
    SubcomposeStackGuard, take_subcompose_frame,
};
use crate::{NodeError, NodeId, RecomposeScope, SlotsHost, runtime};

/// Subcomposes `f` into `slots` at `site`, with the runtime state the host
/// holds or, for a host without one, `fallback_state`'s. See
/// [`Composer::subcompose_slot_with_context`].
pub(super) fn subcompose_slot_at<R>(
    site: SubcomposeSite<'_>,
    fallback_state: impl FnOnce() -> Rc<ComposerRuntimeState>,
    slots: &Rc<SlotsHost>,
    root: Option<NodeId>,
    context: &CapturedCompositionContext,
    f: impl FnOnce(&Composer) -> R,
) -> Result<(R, Vec<RecomposeScope>), NodeError> {
    let _teardown = runtime::enter_state_teardown_scope();
    let shared_state = slots.runtime_state().unwrap_or_else(fallback_state);
    let core = site.spin_up_core(shared_state, slots, root, context.locals.clone());
    *core.subcomposition_owner_scope.borrow_mut() = context
        .owner_scope
        .as_ref()
        .and_then(RecomposeScope::upgrade);
    let composer = Composer::from_core(core);
    composer.subcompose_stack().push(SubcomposeFrame::default());
    let mut guard = SubcomposeStackGuard {
        core: composer.clone_core(),
        leaked: false,
    };
    let root_group_key = crate::location_key!();
    let (result, commands, side_effects, compact_applier) = composer.install(|composer| {
        let (output, outcome) = composer.try_with_slot_host_pass(
            Rc::clone(slots),
            crate::slot::SlotPassMode::Compose,
            |composer| {
                let output = composer.with_group(root_group_key, |composer| f(composer));
                if root.is_some() {
                    composer.pop_parent();
                }
                output
            },
        )?;
        let commands = composer.take_commands();
        let side_effects = composer.take_side_effects();
        Ok((output, commands, side_effects, outcome.compacted))
    })?;
    let frame = {
        let frame = take_subcompose_frame(&guard.core, "subcompose_slot");
        guard.leaked = true;
        frame
    };
    site.flush(commands, compact_applier, side_effects)?;
    Ok((result, frame.scopes))
}
