//! What one recomposed scope costs: `COUNT` scopes in one `FlowRow`, each
//! reading a state that changes every frame. `MODE=bare` reads it and calls
//! nothing, `same` calls `Text` with equal arguments, `change` calls `Text`
//! with a new string of the same length. `SLICES=1` reads every node's
//! modifier slices after each frame, as a renderer does, so a changed text
//! updates slices that hold its layout. Run it under callgrind with
//! `FRAMES=0` and `FRAMES=100` and divide the difference in instructions by
//! `FRAMES * COUNT`.

use std::{any::Any, hint::black_box};

use cranpose_core::{MemoryApplier, MutableState, location_key};
use cranpose_ui::{
    AppContext, Color, Composition, LayoutNode, Modifier, ParagraphStyle, SpanStyle, Text,
    TextStyle, composable,
    text::TextUnit,
    widgets::{FlowRow, FlowRowSpec},
};

#[derive(Clone, Copy, PartialEq)]
enum Mode {
    Bare,
    Same,
    Change,
}

fn style() -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(Color::from_rgb_u8(0x37, 0x41, 0x51)),
            font_size: TextUnit::Sp(12.0),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Em(1.4),
            ..Default::default()
        },
    }
}

#[composable]
fn Scene(frame: MutableState<u32>, count: usize, mode: Mode) {
    FlowRow(
        Modifier::empty().fill_max_width().padding(4.0),
        FlowRowSpec::new()
            .main_axis_spacing(4.0)
            .cross_axis_spacing(4.0),
        move || {
            for index in 0..count {
                Cell(index, frame, mode);
            }
        },
    );
}

#[composable]
fn Cell(index: usize, frame: MutableState<u32>, mode: Mode) {
    let cents = match mode {
        Mode::Bare => {
            black_box(frame.get());
            return;
        }
        Mode::Same => {
            let _ = frame.get();
            10_000 + index as u32
        }
        Mode::Change => 10_000 + ((frame.get() * 7 + index as u32 * 13) % 89_999),
    };
    let text = format!("{}.{:02}", cents / 100, cents % 100);
    Text(text, Modifier::empty(), style());
}

fn env_or<T: std::str::FromStr>(name: &str, default: T) -> T {
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}

/// Reads every node's modifier slices, as a renderer does after a frame.
fn read_slices(composition: &mut Composition<MemoryApplier>) {
    composition.applier_mut().for_each_node_mut(|node| {
        if let Some(layout) = (node as &mut dyn Any).downcast_mut::<LayoutNode>() {
            black_box(layout.modifier_slices_snapshot());
        }
    });
}

fn main() {
    let count: usize = env_or("COUNT", 300);
    let frames: u32 = env_or("FRAMES", 100);
    let mode = match std::env::var("MODE").as_deref() {
        Ok("bare") => Mode::Bare,
        Ok("same") => Mode::Same,
        _ => Mode::Change,
    };
    let slices = env_or("SLICES", 0) == 1;
    let app_context = AppContext::new();
    app_context.enter(|| {
        let mut composition = Composition::new(MemoryApplier::new());
        let frame = MutableState::with_runtime(0u32, composition.runtime_handle());
        composition
            .render(location_key(file!(), line!(), column!()), || {
                Scene(frame, count, mode);
            })
            .expect("initial composition");
        if slices {
            read_slices(&mut composition);
        }
        for index in 1..=frames {
            frame.set(index);
            composition
                .process_invalid_scopes()
                .expect("scope recomposition");
            if slices {
                read_slices(&mut composition);
            }
        }
    });
    println!("count {count} frames {frames}");
}
