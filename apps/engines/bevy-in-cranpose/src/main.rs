use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};

use bevy::log::{error, warn};
use cranpose::{prelude::*, *};
use cranpose_bevy::BevyView;
use demo_scene::SceneControls;
use demo_ui::{EngineInfo, EngineWindow, TargetSize, rememberWindowStates};

const BEVY: EngineInfo = EngineInfo {
    name: "Bevy",
    description: "Bevy 0.20 renders this scene on Cranpose's wgpu device. Cranpose shows the frame as an image, clips it and draws glass over it. No pixels are copied.",
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
    let view = remember(|| Rc::new(RefCell::new(None::<BevyView>))).with(Rc::clone);
    let counter = remember(|| Rc::new(Cell::new((0u64, 0u32)))).with(Rc::clone);
    let gpu = gpu_context();
    let running = gpu.is_some();
    let loop_size = target_size.clone();
    FrameEffect((), running, move |now| {
        let Some(gpu) = gpu.as_ref() else {
            return;
        };
        let (width, height) = loop_size.0.get();
        if width == 0 || height == 0 {
            return;
        }
        let mut slot = view.borrow_mut();
        let bevy = match slot.as_mut() {
            Some(bevy) => {
                if let Err(error) = bevy.resize(width, height) {
                    warn!("Bevy view resize: {error}");
                }
                bevy
            }
            None => match BevyView::new(gpu, width, height, demo_scene::build) {
                Ok(bevy) => slot.insert(bevy),
                Err(error) => {
                    error!("Bevy view: {error}");
                    return;
                }
            },
        };
        bevy.world_mut().resource_mut::<SceneControls>().0 = states.controls.value();
        if let Some(frame) = bevy.update() {
            states.frame.set(Some(frame));
        }
        let (since, frames) = counter.get();
        if now.saturating_sub(since) >= 500_000_000 {
            if since != 0 {
                states
                    .fps
                    .set((u64::from(frames) * 1_000_000_000 / now.saturating_sub(since)) as u32);
            }
            counter.set((now, 0));
        } else {
            counter.set((since, frames + 1));
        }
    });
    EngineWindow(BEVY, states, target_size);
}
