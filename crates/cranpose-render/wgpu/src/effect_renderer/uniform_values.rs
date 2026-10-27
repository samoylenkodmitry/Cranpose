use cranpose_ui_graphics::TileMode;

use super::CompositeSampleMode;

pub(super) fn tile_mode_uniform_value(tile_mode: TileMode) -> f32 {
    match tile_mode {
        TileMode::Clamp => 0.0,
        TileMode::Repeated => 1.0,
        TileMode::Mirror => 2.0,
        TileMode::Decal => 3.0,
    }
}

pub(super) fn composite_sampling_mode_value(sample_mode: CompositeSampleMode) -> f32 {
    match sample_mode {
        CompositeSampleMode::Linear => 0.0,
        CompositeSampleMode::Nearest => 1.0,
        CompositeSampleMode::Texels => 2.0,
    }
}
