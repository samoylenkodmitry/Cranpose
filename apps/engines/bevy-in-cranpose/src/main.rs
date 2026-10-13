use std::{cell::Cell, rc::Rc};

use bevy::log::error;
use cranpose::{prelude::*, *};
use cranpose_bevy::BevyView;
use demo_scene::SceneControls;
use demo_ui::{Controls, EngineInfo, EngineWindow, TargetSize, rememberWindowStates};

const BEVY: EngineInfo = EngineInfo {
    name: "Bevy",
    description: "Bevy 0.20 renders this scene on its own thread with Cranpose's wgpu device. Cranpose shows each finished frame as an image, clips it and draws glass over it. No pixels are copied, and the interface never waits for Bevy.",
    features: "Earth atmosphere, cascaded shadows, fog, bloom",
};

fn main() -> Result<(), LaunchError> {
    AppLauncher::new()
        .with_title("Bevy inside Cranpose")
        .with_size(1320, 820)
        .with_gpu_requirements(cranpose_bevy::gpu_requirements())
        .with_developer_inspector(false)
        .try_run(Root)
}

#[composable]
fn Root() {
    let states = rememberWindowStates();
    let target_size = remember(TargetSize::default).with(TargetSize::clone);
    let gpu = gpu_context();
    let view = remember(move || {
        let view = BevyView::new(gpu.as_ref()?, demo_scene::build);
        view.inspect_err(|error| error!("Bevy view: {error}"))
            .ok()
            .map(Rc::new)
    })
    .with(Option::clone);
    let sent = remember(|| Rc::new(Cell::new(None::<Controls>))).with(Rc::clone);
    let counter = remember(|| Rc::new(Cell::new((0u64, 0u32)))).with(Rc::clone);
    let running = view.is_some();
    let loop_size = target_size.clone();
    FrameEffect((), running, move |now| {
        let Some(view) = view.as_ref() else {
            return;
        };
        let controls = states.controls.value();
        if sent.get() != Some(controls) {
            sent.set(Some(controls));
            view.send(move |world| world.resource_mut::<SceneControls>().0 = controls);
        }
        let (width, height) = loop_size.0.get();
        let frame = view.show(width, height);
        let shown = u32::from(frame.is_some());
        if let Some(frame) = frame {
            states.frame.set(Some(frame));
        }
        let (since, frames) = counter.get();
        let frames = frames + shown;
        let elapsed = now.saturating_sub(since);
        if since == 0 {
            counter.set((now, 0));
        } else if elapsed >= 500_000_000 {
            let per_second = (u64::from(frames) * 1_000_000_000 + elapsed / 2) / elapsed;
            states.fps.set(per_second as u32);
            counter.set((now, 0));
        } else {
            counter.set((since, frames));
        }
    });
    EngineWindow(BEVY, states, target_size);
}
