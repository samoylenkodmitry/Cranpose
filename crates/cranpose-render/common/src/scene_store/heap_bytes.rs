// Estimates of the heap memory a scene store holds, for memory reports.

use std::mem::size_of;

use cranpose_ui::text::AnnotatedString;
use cranpose_ui_graphics::{DrawPrimitive, ShadowPrimitive};

use super::{GeoRecord, SceneStore, TreeRecord, WarmRecord};

pub(super) fn store_heap_bytes(store: &SceneStore) -> usize {
    let rows = store.tree.capacity() * size_of::<TreeRecord>()
        + store.geo.capacity() * size_of::<GeoRecord>()
        + store.warm.capacity() * size_of::<WarmRecord>()
        + store.generations.capacity() * size_of::<u32>()
        + store.free_rows.capacity() * size_of::<u32>();
    let props = store.props.heap_bytes() + store.graphics_layers.heap_bytes();
    let draws = store.draws.heap_bytes()
        + store
            .draws
            .values()
            .map(|draw| draw_primitive_heap_bytes(&draw.primitive))
            .sum::<usize>();
    let texts = store.texts.heap_bytes()
        + store
            .texts
            .values()
            .map(|text| annotated_string_heap_bytes(&text.text))
            .sum::<usize>();
    let runs = store.runs.heap_bytes()
        + store
            .runs
            .values()
            .map(|run| {
                run.recording.pod_heap_bytes()
                    + size_of_val(run.recording.others())
                    + run
                        .recording
                        .others()
                        .iter()
                        .map(draw_primitive_heap_bytes)
                        .sum::<usize>()
            })
            .sum::<usize>();
    let marks = store.marks.heap_bytes() + size_of_val(store.mark_scratch.as_slice());
    rows + store.hashes.heap_bytes()
        + store.facts.heap_bytes()
        + store.lists.heap_bytes()
        + store.items.heap_bytes()
        + marks
        + store.transforms.heap_bytes()
        + props
        + draws
        + texts
        + runs
}

fn draw_primitive_heap_bytes(primitive: &DrawPrimitive) -> usize {
    match primitive {
        DrawPrimitive::Content
        | DrawPrimitive::Group(_)
        | DrawPrimitive::Rect { .. }
        | DrawPrimitive::RoundRect { .. }
        | DrawPrimitive::Arc { .. }
        | DrawPrimitive::Line { .. }
        | DrawPrimitive::Trapezoid { .. } => 0,
        DrawPrimitive::Blend { primitive, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(primitive)
        }
        DrawPrimitive::Image { .. } => 0,
        DrawPrimitive::Text(text) => {
            size_of::<cranpose_ui_graphics::TextPrimitive>()
                + text.text.len()
                + text.style.font_family.as_ref().map_or(0, String::capacity)
        }
        DrawPrimitive::Shadow(shadow) => shadow_primitive_heap_bytes(shadow),
    }
}

fn shadow_primitive_heap_bytes(shadow: &ShadowPrimitive) -> usize {
    match shadow {
        ShadowPrimitive::Drop { shape, .. } => {
            size_of::<DrawPrimitive>() + draw_primitive_heap_bytes(shape)
        }
        ShadowPrimitive::Inner { fill, cutout, .. } => {
            size_of::<DrawPrimitive>() * 2
                + draw_primitive_heap_bytes(fill)
                + draw_primitive_heap_bytes(cutout)
        }
    }
}

fn annotated_string_heap_bytes(text: &AnnotatedString) -> usize {
    let content = text.render_string();
    let ranges = content.span_styles().len()
        + content.paragraph_styles().len()
        + content.string_annotations().len()
        + content.links().len();
    content.len()
        + ranges * size_of::<usize>() * 2
        + content
            .string_annotations()
            .iter()
            .map(|annotation| annotation.item.tag.len() + annotation.item.annotation.len())
            .sum::<usize>()
        + content
            .links()
            .iter()
            .map(|link| match &link.item {
                cranpose_ui::text::LinkKey::Url(url) => url.len(),
                cranpose_ui::text::LinkKey::Clickable(tag) => tag.len(),
            })
            .sum::<usize>()
}
