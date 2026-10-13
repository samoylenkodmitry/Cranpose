mod hud;

use bevy::{
    camera::RenderTarget,
    input::mouse::AccumulatedMouseMotion,
    prelude::*,
    window::{PrimaryWindow, WindowResolution},
};
use cranpose_bevy::hud::{CranposeHudPlugin, HudPointer};
use demo_scene::{Controls, SceneStats};
use hud::{HudBridge, HudCommand, Stats};

fn main() -> AppExit {
    let mut app = App::new();
    app.add_plugins(DefaultPlugins.set(WindowPlugin {
        primary_window: Some(Window {
            title: "Cranpose HUD in Bevy".into(),
            resolution: WindowResolution::new(1320, 820),
            ..default()
        }),
        ..default()
    }))
    .add_plugins(CranposeHudPlugin::new(|world: &mut World| {
        let bridge = HudBridge::default();
        world.insert_non_send(bridge.clone());
        Box::new(move || hud::Hud(bridge.clone()))
    }))
    .add_systems(Update, (orbit_with_mouse, sync_hud, run_commands));
    demo_scene::build(&mut app, RenderTarget::default());
    app.run()
}

fn orbit_with_mouse(
    bridge: Option<NonSend<HudBridge>>,
    buttons: Res<ButtonInput<MouseButton>>,
    motion: Res<AccumulatedMouseMotion>,
    pointer: Res<HudPointer>,
) {
    let Some(states) = bridge
        .as_ref()
        .and_then(|bridge| bridge.states.get().copied())
    else {
        return;
    };
    if !buttons.pressed(MouseButton::Left) || pointer.captured || motion.delta == Vec2::ZERO {
        return;
    }
    states.controls.update(|controls| {
        controls.yaw += motion.delta.x * 0.008;
        controls.pitch = (controls.pitch + motion.delta.y * 0.006).clamp(0.05, 1.5);
    });
}

fn sync_hud(
    bridge: Option<NonSend<HudBridge>>,
    mut controls: ResMut<Controls>,
    scene: Res<SceneStats>,
    time: Res<Time>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut since_report: Local<f32>,
    mut frames: Local<u32>,
) {
    let Some(states) = bridge
        .as_ref()
        .and_then(|bridge| bridge.states.get().copied())
    else {
        return;
    };
    let wanted = states.controls.value();
    if *controls != wanted {
        *controls = wanted;
    }
    *since_report += time.delta_secs();
    *frames += 1;
    if *since_report < 0.5 {
        return;
    }
    let size = windows
        .single()
        .map(|window| (window.physical_width(), window.physical_height()))
        .unwrap_or_default();
    states.stats.set(Stats {
        fps: (*frames as f32 / *since_report).round() as u32,
        shapes: scene.shapes,
        energy: 0.5 + 0.45 * (time.elapsed_secs() * 0.7).sin(),
        size,
    });
    *since_report = 0.0;
    *frames = 0;
}

fn run_commands(bridge: Option<NonSend<HudBridge>>, mut commands: Commands) {
    let Some(bridge) = bridge else {
        return;
    };
    for command in bridge.commands.borrow_mut().drain(..) {
        match command {
            HudCommand::DropShape => {
                commands.run_system_cached(demo_scene::drop_shape);
            }
        }
    }
}
