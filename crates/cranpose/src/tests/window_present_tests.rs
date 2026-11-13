use std::time::Instant;

use cranpose_render_wgpu::{CancelReason, PresentOutcome, PresentTimings};

use super::{FrameInFlight, FramesInFlight, PresentReturns, WindowPresent};

// A window whose present thread holds the frames `ids`, oldest first.
fn holding(ids: &[u64]) -> WindowPresent {
    let now = Instant::now();
    WindowPresent::OnPresentThread {
        frames: FramesInFlight {
            frames: ids
                .iter()
                .map(|&id| FrameInFlight {
                    id,
                    started_at: now,
                    after_update: now,
                })
                .collect(),
            clock_origin: now,
        },
        occluded: false,
        owes_frame: false,
    }
}

// What the loop makes of the present thread handing back `outcomes`, in
// order, to `present`.
fn hand_back(present: &mut WindowPresent, outcomes: &[(u64, PresentOutcome)]) -> PresentReturns {
    present.drain_from(|on_return| {
        for &(id, outcome) in outcomes {
            on_return(id, outcome, PresentTimings::default());
        }
    })
}

fn set_occluded(present: &mut WindowPresent, now: bool) {
    if let WindowPresent::OnPresentThread { occluded, .. } = present {
        *occluded = now;
    }
}

#[test]
fn a_refused_frame_with_no_newer_one_on_its_way_owes_the_window_a_frame() {
    let mut present = holding(&[1]);
    let returns = hand_back(
        &mut present,
        &[(1, PresentOutcome::Cancelled(CancelReason::NeedsWhole))],
    );
    assert!(returns.redraw);
    assert!(present.owes_frame());
}

#[test]
fn a_newer_frame_on_its_way_stands_in_for_a_refused_one() {
    let mut present = holding(&[1, 2]);
    let returns = hand_back(
        &mut present,
        &[(1, PresentOutcome::Cancelled(CancelReason::SurfaceEpoch))],
    );
    assert!(!returns.redraw);
    assert!(!present.owes_frame());
    let returns = hand_back(&mut present, &[(2, PresentOutcome::Presented)]);
    assert!(returns.presented.is_some());
    assert!(!present.owes_frame());
}

#[test]
fn the_owed_frame_waits_while_the_window_server_shows_none_of_the_window() {
    let mut present = holding(&[1]);
    set_occluded(&mut present, true);
    hand_back(
        &mut present,
        &[(1, PresentOutcome::Cancelled(CancelReason::DeviceLost))],
    );
    assert!(!present.owes_frame());
    set_occluded(&mut present, false);
    assert!(present.owes_frame());
}

#[test]
fn a_frame_the_surface_showed_already_counts_as_shown() {
    let mut present = holding(&[1]);
    let returns = hand_back(&mut present, &[(1, PresentOutcome::Unchanged)]);
    assert!(returns.shown);
    assert!(!returns.redraw);
    assert!(!present.owes_frame());
}

#[test]
fn a_frame_with_no_image_to_draw_into_asks_for_no_redraw() {
    let mut present = holding(&[1]);
    let returns = hand_back(
        &mut present,
        &[(
            1,
            PresentOutcome::Cancelled(CancelReason::SurfaceUnavailable),
        )],
    );
    assert!(returns.unavailable);
    assert!(!returns.redraw);
    assert!(!present.owes_frame());
}
