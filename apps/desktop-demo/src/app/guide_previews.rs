use cranpose_ui::{composable, Box, BoxSpec, Color, Modifier};

struct Preview {
    id: &'static str,
    source: &'static str,
    render: fn(),
}

include!(concat!(env!("OUT_DIR"), "/guide_previews.rs"));

pub(super) fn find(fence: &str, source: &str) -> Option<usize> {
    let id = fence
        .split_whitespace()
        .find_map(|part| part.strip_prefix("preview="))?;
    PREVIEWS
        .iter()
        .position(|preview| preview.id == id && preview.source.trim_end() == source.trim_end())
}

#[composable]
pub(super) fn GuidePreview(index: usize) {
    let preview = &PREVIEWS[index];
    Box(
        Modifier::empty()
            .fill_max_width()
            .height(240.0)
            .clip_to_bounds()
            .background(Color(0.10, 0.14, 0.18, 1.0))
            .content_description(format!("Interactive example: {}", preview.id))
            .padding(16.0),
        BoxSpec::default(),
        preview.render,
    );
}
