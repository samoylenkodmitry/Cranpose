use cranpose_services::LifecycleState;
use winit::{dpi::PhysicalSize, event::WindowEvent};

use super::WindowPresence;

#[test]
fn focus_occlusion_and_minimizing_move_the_window_lifecycle() {
    let mut presence = WindowPresence::shown();
    let steps = [
        WindowEvent::Focused(false),
        WindowEvent::Focused(false),
        WindowEvent::Occluded(true),
        WindowEvent::Occluded(false),
        WindowEvent::Focused(true),
        WindowEvent::SurfaceResized(PhysicalSize::new(0, 0)),
        WindowEvent::SurfaceResized(PhysicalSize::new(800, 600)),
        WindowEvent::SurfaceResized(PhysicalSize::new(900, 600)),
        WindowEvent::CloseRequested,
    ];
    let published: Vec<Option<LifecycleState>> =
        steps.iter().map(|event| presence.observe(event)).collect();
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
