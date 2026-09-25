use cranpose_core::run_in_mutable_snapshot;
use cranpose_render_common::Renderer;

use crate::AppShell;

impl<R: Renderer> AppShell<R>
where
    R::Error: std::fmt::Debug,
{
    pub(crate) fn update_modal_focus(&mut self) {
        if !self.app.semantics_enabled
            && self
                .surfaces
                .iter()
                .all(|surface| surface.modal_focus.is_empty())
            && !cranpose_ui::dismissable_popup_open()
        {
            return;
        }
        for surface in &mut self.surfaces {
            let modal = surface.top_modal(&mut self.app);
            if modal == surface.modal_focus.last().map(|entry| entry.0) {
                continue;
            }
            let current = cranpose_ui::active_focus_target();
            let order = surface
                .layout_tree_in_context(&mut self.app)
                .map(cranpose_ui::collect_focus_order)
                .unwrap_or_default();
            let previous = previous_modal_focus(&mut surface.modal_focus, modal, current);
            let target = previous
                .filter(|previous| order.iter().any(|entry| entry.node_id == *previous))
                .or_else(|| {
                    order
                        .iter()
                        .find(|entry| Some(entry.node_id) != modal)
                        .map(|entry| entry.node_id)
                })
                .or_else(|| order.first().map(|entry| entry.node_id));
            if let Some(target) = target {
                let _ =
                    run_in_mutable_snapshot(|| cranpose_ui::request_focus_from_platform(target));
            } else {
                let _ = run_in_mutable_snapshot(|| cranpose_ui::FocusManager.clear_focus());
            }
        }
    }
}

fn previous_modal_focus(
    history: &mut Vec<(cranpose_core::NodeId, Option<cranpose_core::NodeId>)>,
    modal: Option<cranpose_core::NodeId>,
    current: Option<cranpose_core::NodeId>,
) -> Option<cranpose_core::NodeId> {
    let Some(modal) = modal else {
        let previous = history.first().and_then(|entry| entry.1);
        history.clear();
        return previous;
    };
    if let Some(index) = history.iter().position(|entry| entry.0 == modal) {
        let previous = history.get(index + 1).and_then(|entry| entry.1);
        history.truncate(index + 1);
        previous
    } else {
        history.push((modal, current));
        current
    }
}
