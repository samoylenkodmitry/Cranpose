use cranpose::prelude::*;
use cranpose_ui::text::{FontWeight, ParagraphStyle, SpanStyle, TextUnit};
use cranpose_ui_graphics::{Color, RoundedCornerShape};
use cranpose_ui_layout::{Alignment, LinearArrangement, VerticalAlignment};

pub(super) const BG_CANVAS: Color = Color::from_rgb_u8(0xF1, 0xF5, 0xF9);
pub(super) const BG_CARD: Color = Color::from_rgb_u8(0xFF, 0xFF, 0xFF);
pub(super) const BG_SUBTLE: Color = Color::from_rgb_u8(0xF8, 0xFA, 0xFC);
pub(super) const BG_TRACK: Color = Color::from_rgb_u8(0xE2, 0xE8, 0xF0);
pub(super) const BORDER_CARD: Color = Color::from_rgb_u8(0xCB, 0xD5, 0xE1);
pub(super) const BORDER_STRONG: Color = Color::from_rgb_u8(0x94, 0xA3, 0xB8);
pub(super) const INK: Color = Color::from_rgb_u8(0x0F, 0x17, 0x2A);
pub(super) const MUTED: Color = Color::from_rgb_u8(0x47, 0x55, 0x69);
pub(super) const HEADER_BG: Color = Color::from_rgb_u8(0x0F, 0x17, 0x2A);
pub(super) const FOOTER_BG: Color = Color::from_rgb_u8(0x1E, 0x29, 0x3B);

pub(super) const BLUE: Color = Color::from_rgb_u8(0x25, 0x63, 0xEB);
pub(super) const BLUE_SOFT: Color = Color::from_rgb_u8(0xDB, 0xEA, 0xFE);
pub(super) const INDIGO: Color = Color::from_rgb_u8(0x4F, 0x46, 0xE5);
pub(super) const INDIGO_SOFT: Color = Color::from_rgb_u8(0xE0, 0xE7, 0xFF);
pub(super) const VIOLET: Color = Color::from_rgb_u8(0x7C, 0x3A, 0xED);
pub(super) const VIOLET_SOFT: Color = Color::from_rgb_u8(0xED, 0xE9, 0xFE);
pub(super) const ROSE: Color = Color::from_rgb_u8(0xE1, 0x1D, 0x48);
pub(super) const ROSE_SOFT: Color = Color::from_rgb_u8(0xFF, 0xE4, 0xE6);
pub(super) const RED: Color = Color::from_rgb_u8(0xDC, 0x26, 0x26);
pub(super) const RED_SOFT: Color = Color::from_rgb_u8(0xFE, 0xE2, 0xE2);
pub(super) const AMBER: Color = Color::from_rgb_u8(0xD9, 0x77, 0x06);
pub(super) const AMBER_SOFT: Color = Color::from_rgb_u8(0xFE, 0xF3, 0xC7);
pub(super) const EMERALD: Color = Color::from_rgb_u8(0x05, 0x96, 0x69);
pub(super) const EMERALD_SOFT: Color = Color::from_rgb_u8(0xD1, 0xFA, 0xE5);
pub(super) const TEAL: Color = Color::from_rgb_u8(0x0D, 0x94, 0x88);
pub(super) const TEAL_SOFT: Color = Color::from_rgb_u8(0xCC, 0xFB, 0xF1);
pub(super) const CYAN: Color = Color::from_rgb_u8(0x02, 0x84, 0xC7);
pub(super) const CYAN_SOFT: Color = Color::from_rgb_u8(0xE0, 0xF2, 0xFE);
pub(super) const SLATE: Color = Color::from_rgb_u8(0x64, 0x74, 0x8B);

pub(super) const ACCENTS: [Color; 9] =
    [BLUE, EMERALD, AMBER, ROSE, VIOLET, CYAN, TEAL, INDIGO, RED];

pub(super) fn rect_shape() -> RoundedCornerShape {
    RoundedCornerShape::uniform(0.0)
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct ChipSpec {
    pub(super) width: f32,
    pub(super) height: f32,
    pub(super) bg: Color,
    pub(super) radius: f32,
    pub(super) font_size: f32,
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct RowChipSpec {
    pub(super) height: f32,
    pub(super) bg: Color,
    pub(super) radius: f32,
    pub(super) font_size: f32,
    pub(super) alignment: Alignment,
    pub(super) horizontal_padding: f32,
}

pub(super) fn atlas_text_style(size_sp: f32, color: Color, bold: bool) -> TextStyle {
    TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: TextUnit::Sp(size_sp),
            font_weight: bold.then_some(FontWeight::BOLD),
            ..Default::default()
        },
        paragraph_style: ParagraphStyle {
            line_height: TextUnit::Em(1.4),
            ..Default::default()
        },
    }
}

#[composable(no_skip)]
pub(super) fn ZoneCard<F: FnMut() + 'static>(
    tag: &'static str,
    title: &'static str,
    accent: Color,
    modifier: Modifier,
    mut content: F,
) {
    Column(
        modifier
            .test_tag(tag)
            .background(BG_CARD)
            .rounded_corners(6.0)
            .border(1.0, BORDER_CARD, RoundedCornerShape::uniform(6.0))
            .padding(6.0),
        ColumnSpec::default(),
        move || {
            Row(
                Modifier::empty().fill_max_width().height(15.0),
                RowSpec::new()
                    .horizontal_arrangement(LinearArrangement::spaced_by(5.0))
                    .vertical_alignment(VerticalAlignment::CenterVertically),
                move || {
                    Box(
                        Modifier::empty()
                            .size_points(4.0, 11.0)
                            .background(accent)
                            .rounded_corners(2.0),
                        BoxSpec::default(),
                        || {},
                    );
                    Text(title, Modifier::empty(), atlas_text_style(9.0, INK, true));
                },
            );
            Spacer(Modifier::empty().height(4.0));
            content();
        },
    );
}

#[composable]
pub(super) fn LabeledChip(tag: String, label: String, spec: ChipSpec) {
    let mut modifier = Modifier::empty()
        .test_tag(tag)
        .size_points(spec.width, spec.height)
        .background(spec.bg);
    if spec.radius > 0.0 {
        modifier = modifier.rounded_corners(spec.radius);
    }
    Box(
        modifier,
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                atlas_text_style(spec.font_size, Color::WHITE, true),
            );
        },
    );
}

#[composable]
pub(super) fn FullWidthChip(tag: String, label: String, spec: RowChipSpec) {
    let mut modifier = Modifier::empty()
        .test_tag(tag)
        .fill_max_width()
        .height(spec.height)
        .background(spec.bg);
    if spec.radius > 0.0 {
        modifier = modifier.rounded_corners(spec.radius);
    }
    if spec.horizontal_padding > 0.0 {
        modifier = modifier.padding_horizontal(spec.horizontal_padding);
    }
    Box(
        modifier,
        BoxSpec::new().content_alignment(spec.alignment),
        move || {
            Text(
                label.clone(),
                Modifier::empty(),
                atlas_text_style(spec.font_size, Color::WHITE, true),
            );
        },
    );
}
