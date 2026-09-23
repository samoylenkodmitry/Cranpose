use cranpose_services::LifecycleState;
use winit::{dpi::PhysicalSize, event::WindowEvent, window::WindowId};

use super::WindowPresence;

const PRIMARY: WindowId = WindowId::from_raw(1);
const SECONDARY: WindowId = WindowId::from_raw(2);

fn settle(
    presence: &mut WindowPresence,
    events: &[(WindowId, WindowEvent)],
) -> Option<LifecycleState> {
    for (window, event) in events {
        presence.observe(*window, event);
    }
    presence.settled()
}

#[test]
fn focus_occlusion_and_minimizing_move_the_window_lifecycle() {
    let mut presence = WindowPresence::shown(PRIMARY);
    let batches = [
        vec![(PRIMARY, WindowEvent::Focused(false))],
        vec![(PRIMARY, WindowEvent::Focused(false))],
        vec![(PRIMARY, WindowEvent::Occluded(true))],
        vec![(PRIMARY, WindowEvent::Occluded(false))],
        vec![(PRIMARY, WindowEvent::Focused(true))],
        vec![(
            PRIMARY,
            WindowEvent::SurfaceResized(PhysicalSize::new(0, 0)),
        )],
        vec![(
            PRIMARY,
            WindowEvent::SurfaceResized(PhysicalSize::new(800, 600)),
        )],
        vec![(
            PRIMARY,
            WindowEvent::SurfaceResized(PhysicalSize::new(900, 600)),
        )],
        vec![(PRIMARY, WindowEvent::CloseRequested)],
    ];
    let published: Vec<Option<LifecycleState>> = batches
        .iter()
        .map(|batch| settle(&mut presence, batch))
        .collect();
    assert_eq!(
        published,
        vec![
            Some(LifecycleState::Paused),
            None,
            Some(LifecycleState::Stopped),
            Some(LifecycleState::Paused),
            Some(LifecycleState::Resumed),
            Some(LifecycleState::Stopped),
            Some(LifecycleState::Resumed),
            None,
            None,
        ]
    );
}

#[test]
fn moving_focus_between_the_apps_own_windows_keeps_it_resumed() {
    let mut presence = WindowPresence::shown(PRIMARY);
    let handoff = [
        (PRIMARY, WindowEvent::Focused(false)),
        (SECONDARY, WindowEvent::Focused(true)),
    ];
    assert_eq!(settle(&mut presence, &handoff), None);
    let hidden_secondary = [
        (SECONDARY, WindowEvent::Occluded(true)),
        (
            SECONDARY,
            WindowEvent::SurfaceResized(PhysicalSize::new(0, 0)),
        ),
    ];
    assert_eq!(
        settle(&mut presence, &hidden_secondary),
        None,
        "only the primary window's visibility counts"
    );
    assert_eq!(
        settle(&mut presence, &[(SECONDARY, WindowEvent::Destroyed)]),
        Some(LifecycleState::Paused),
        "a closed window no longer holds focus"
    );
}
