use cranpose_ui_graphics::BlendMode;

use crate::render::{RunTier, SUPPORTED_BLEND_MODES, ShapePipelineKey, supported_blend_mode};

fn prewarmed() -> Vec<ShapePipelineKey> {
    let mut keys = Vec::new();
    for blend_mode in SUPPORTED_BLEND_MODES {
        for tier in [RunTier::Store, RunTier::Arena] {
            keys.push(ShapePipelineKey::general_for(blend_mode, tier));
        }
    }
    keys
}

#[test]
fn every_blend_mode_a_scene_can_ask_for_is_prewarmed() {
    let ready = prewarmed();
    for mode in BlendMode::ALL {
        for tier in [RunTier::Store, RunTier::Arena] {
            let key = ShapePipelineKey::general_for(supported_blend_mode(mode), tier);
            assert!(
                ready.contains(&key),
                "{mode:?} resolves to a general pipeline no one built at startup, so the \
                 first draw using it compiles inside its own frame"
            );
        }
    }
}

#[test]
fn the_prewarmed_set_is_the_whole_general_space() {
    let ready = prewarmed();
    assert_eq!(ready.len(), SUPPORTED_BLEND_MODES.len() * 2);
    assert!(ready.iter().all(|key| key.is_general()));
    let mut unique = ready.clone();
    unique.dedup();
    assert_eq!(unique.len(), ready.len(), "a pipeline built twice");
}

#[test]
fn an_unsupported_mode_folds_onto_one_that_is_built() {
    assert_eq!(
        supported_blend_mode(BlendMode::Multiply),
        BlendMode::SrcOver
    );
    assert_eq!(supported_blend_mode(BlendMode::Clear), BlendMode::SrcOver);
    assert_eq!(supported_blend_mode(BlendMode::Src), BlendMode::Src);
    assert_eq!(supported_blend_mode(BlendMode::DstOut), BlendMode::DstOut);
}
