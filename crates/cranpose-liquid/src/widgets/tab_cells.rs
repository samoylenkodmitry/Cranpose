use std::rc::Rc;

use cranpose_macros::composable;
use cranpose_ui::{
    Brush, Color, CornerRadii, Modifier, Rect, SemanticsWidgetRole, Size,
    text::{FontWeight, SpanStyle, TextStyle},
    widgets::{Box, BoxSpec, ContentScale, Image, Text},
};
use cranpose_ui_layout::Alignment;

use super::{
    selection::SelectionState,
    tab_bar::{
        BAR_HEIGHT, BLOB_HEIGHT, BLOB_MARGIN, LiquidTab, LiquidTabIcon, LiquidTabIconStyle,
        TAB_ICON_SIZE, TAB_LABEL_SIZE, TabGeometry, TabSelect, normalize_icon_scale,
    },
    tab_flight_lens::{tab_lens_base_size, tab_lens_deformed_size},
};
use crate::theme::LiquidTypography;

#[derive(Clone, Copy, Debug, PartialEq)]
struct AppBadgeGeometry {
    size: Size,
    corner_radius: f32,
    stripe: Rect,
    glyph: Rect,
}

fn app_badge_geometry(optical_scale: f32) -> AppBadgeGeometry {
    let scale = normalize_icon_scale(optical_scale);
    AppBadgeGeometry {
        size: Size::new(20.0 * scale, 32.0 * scale),
        corner_radius: 5.0 * scale,
        stripe: Rect {
            x: 7.0 * scale,
            y: 4.5 * scale,
            width: 6.0 * scale,
            height: 1.5 * scale,
        },
        glyph: Rect {
            x: 3.0 * scale,
            y: 10.0 * scale,
            width: 14.0 * scale,
            height: 14.0 * scale,
        },
    }
}

#[composable]
fn TabIcon(icon: LiquidTabIcon, style: LiquidTabIconStyle, color: Color, optical_scale: f32) {
    const FRAME_HEIGHT: f32 = 32.0;
    Box(
        Modifier::empty().size(Size::new(TAB_ICON_SIZE, FRAME_HEIGHT)),
        BoxSpec::default().content_alignment(Alignment::CENTER),
        move || match style {
            LiquidTabIconStyle::Plain => {
                TabGlyph(icon.clone(), TAB_ICON_SIZE * optical_scale, color);
            }
            LiquidTabIconStyle::AppBadge => {
                let icon = icon.clone();
                let geometry = app_badge_geometry(optical_scale);
                Box(
                    Modifier::empty()
                        .size(geometry.size)
                        .draw_behind(move |scope| {
                            scope.draw_round_rect(
                                Brush::solid(color),
                                CornerRadii::uniform(geometry.corner_radius),
                            );
                            scope.draw_rect_at(geometry.stripe, Brush::solid(Color::WHITE));
                        }),
                    BoxSpec::default(),
                    move || {
                        let icon = icon.clone();
                        Box(
                            Modifier::empty()
                                .offset(geometry.glyph.x, geometry.glyph.y)
                                .size(Size::new(geometry.glyph.width, geometry.glyph.height)),
                            BoxSpec::default(),
                            move || TabGlyph(icon.clone(), geometry.glyph.width, Color::WHITE),
                        );
                    },
                );
            }
        },
    );
}

#[composable]
fn TabGlyph(icon: LiquidTabIcon, size: f32, color: Color) {
    match icon {
        LiquidTabIcon::Vector(path) => crate::icons::Icon(path, None, size, color),
        LiquidTabIcon::Painter {
            painter,
            size: intrinsic,
        } => {
            let scale = size / TAB_ICON_SIZE;
            Image(
                painter,
                None,
                Modifier::empty()
                    .size(Size::new(intrinsic.width * scale, intrinsic.height * scale)),
                Alignment::CENTER,
                ContentScale::FillBounds,
                1.0,
                Some(cranpose_ui_graphics::ColorFilter::tint(color)),
            );
        }
    }
}

pub(super) fn tab_ink_selection(
    geometry: TabGeometry,
    position: f32,
    activity: f32,
    strain: Size,
    color: Color,
) -> super::vibrancy::InkSelection {
    let (width, height) = tab_lens_base_size(geometry.cell_width, activity);
    let base = Size::new(width, height);
    let deformed = tab_lens_deformed_size(base, strain);
    let width = deformed.width;
    let height = deformed.height;
    super::vibrancy::InkSelection {
        bounds: Rect {
            x: position + geometry.cell_width * 0.5 - width * 0.5,
            y: BLOB_HEIGHT * 0.5 - height * 0.5,
            width,
            height,
        },
        color,
        content_zoom: 1.0 + 0.16 * activity.clamp(0.0, 1.0),
        activity: activity.clamp(0.0, 1.0),
        optical_scale: 1.0,
        projection: Size::new(deformed.width / base.width, deformed.height / base.height),
    }
}

#[derive(Clone, Copy, PartialEq)]
pub(super) struct TabCellsSpec {
    pub(super) geometry: TabGeometry,
    pub(super) base_color: Color,
}

// The selection as the cells show it: the committed index, which their
// semantics report, the tab under the lens, whose label is bold, and the
// callback a cell's click action calls.
#[derive(Clone, PartialEq)]
pub(super) struct TabCellSelection {
    pub(super) committed: cranpose_core::MutableState<usize>,
    pub(super) visual: cranpose_core::State<usize>,
    pub(super) select: TabSelect,
}

// A cell's label style, by whether the lens is over it.
fn tab_label_styles(typography: &LiquidTypography, color: Color) -> [TextStyle; 2] {
    [FontWeight::MEDIUM, FontWeight::SEMI_BOLD].map(|weight| TextStyle {
        span_style: SpanStyle {
            color: Some(color),
            font_size: cranpose_ui::text::TextUnit::Sp(TAB_LABEL_SIZE),
            font_weight: Some(weight),
            ..typography.caption1.span_style.clone()
        },
        paragraph_style: cranpose_ui::text::ParagraphStyle {
            line_height: cranpose_ui::text::TextUnit::Sp(12.0),
            line_height_style: Some(cranpose_ui::text::LineHeightStyle {
                alignment: cranpose_ui::text::LineHeightAlignment::Center,
                trim: cranpose_ui::text::LineHeightTrim::None,
                mode: cranpose_ui::text::LineHeightMode::Fixed,
            }),
            ..typography.caption1.paragraph_style.clone()
        },
    })
}

/// Where the lens's ink falls on the cells this frame, under the bar's lift.
fn lifted_ink(
    geometry: TabGeometry,
    count: usize,
    selection: super::vibrancy::InkSelection,
    transform: &cranpose_ui_graphics::GraphicsLayer,
) -> (super::vibrancy::InkSelection, super::vibrancy::InkGrid) {
    let size = Size::new(geometry.pill_width(), BAR_HEIGHT);
    let mut selection = selection;
    let center = selection.bounds.x + selection.bounds.width * 0.5 + BLOB_MARGIN;
    selection.bounds.width *= transform.scale_x;
    selection.bounds.height *= transform.scale_y;
    selection.optical_scale = transform.scale_x.min(transform.scale_y);
    selection.bounds.x = (center - size.width * 0.5) * transform.scale_x
        + size.width * 0.5
        + transform.translation_x
        - selection.bounds.width * 0.5;
    selection.bounds.y = (BAR_HEIGHT - selection.bounds.height) * 0.5;
    let grid = super::vibrancy::InkGrid {
        first_center: (
            (BLOB_MARGIN + geometry.cell_width * 0.5 - size.width * 0.5) * transform.scale_x
                + size.width * 0.5
                + transform.translation_x,
            BAR_HEIGHT * 0.5,
        ),
        pitch: geometry.pitch * transform.scale_x,
        count,
    };
    (selection, grid)
}

#[composable]
pub(super) fn TabCells(
    modifier: Modifier,
    tabs: Rc<Vec<LiquidTab>>,
    typography: LiquidTypography,
    spec: TabCellsSpec,
    selection: TabCellSelection,
    ink_selection: impl Fn() -> super::vibrancy::InkSelection + 'static,
    transform: impl Fn() -> cranpose_ui_graphics::GraphicsLayer + 'static,
) {
    let transform: Rc<dyn Fn() -> cranpose_ui_graphics::GraphicsLayer> = Rc::new(transform);
    let geometry = spec.geometry;
    let size = Size::new(geometry.pill_width(), BAR_HEIGHT);
    let count = tabs.len();
    let ink = {
        let transform = Rc::clone(&transform);
        move || lifted_ink(geometry, count, ink_selection(), &transform())
    };
    let label_styles = Rc::new(tab_label_styles(&typography, spec.base_color));
    super::vibrancy::VibrantContent(modifier, size, spec.base_color, ink, move || {
        let tabs = Rc::clone(&tabs);
        let label_styles = Rc::clone(&label_styles);
        let selection = selection.clone();
        let transform = Rc::clone(&transform);
        Box(
            Modifier::empty()
                .size(size)
                .selectable_group()
                .role(SemanticsWidgetRole::TabBar)
                .graphics_layer(move || transform()),
            BoxSpec::default(),
            move || {
                // Read here, a new selection runs this loop, and only the
                // cells whose look it changes compose again.
                let committed = selection.committed.get();
                let visual = selection.visual.get();
                for index in 0..tabs.len() {
                    TabCell(
                        Rc::clone(&tabs),
                        Rc::clone(&label_styles),
                        TabCellLook {
                            index,
                            geometry,
                            color: spec.base_color,
                            committed: SelectionState::of(index, committed),
                            bold: visual == index,
                            select: selection.select.clone(),
                        },
                    );
                }
            },
        );
    });
}

#[derive(Clone, PartialEq)]
struct TabCellLook {
    index: usize,
    geometry: TabGeometry,
    color: Color,
    committed: SelectionState,
    bold: bool,
    select: TabSelect,
}

#[composable]
fn TabCell(tabs: Rc<Vec<LiquidTab>>, label_styles: Rc<[TextStyle; 2]>, look: TabCellLook) {
    let TabCellLook {
        index,
        geometry,
        color,
        committed,
        bold,
        select,
    } = look;
    let tab = &tabs[index];
    let icon_offset = tab.icon_offset;
    let cell = Modifier::empty()
        .offset(BLOB_MARGIN + index as f32 * geometry.pitch, BLOB_MARGIN)
        .size(Size::new(geometry.cell_width, BLOB_HEIGHT))
        .stable_semantics(super::selection::selection_semantics(
            tab.label.to_string(),
            SemanticsWidgetRole::Tab,
            index,
            committed,
            select.callback(),
        ))
        .focusable();
    let icon = tab.icon.clone();
    let icon_style = tab.icon_style;
    let icon_scale = tab.icon_scale;
    let label = tab.label.clone();
    let label_style = label_styles[usize::from(bold)].clone();
    Box(cell, BoxSpec::default(), move || {
        let label = label.clone();
        let label_style = label_style.clone();
        let icon = icon.clone();
        Box(
            Modifier::empty()
                .offset(
                    (geometry.cell_width - TAB_ICON_SIZE) * 0.5 + icon_offset.0,
                    3.0 + icon_offset.1,
                )
                .size(Size::new(TAB_ICON_SIZE, TAB_ICON_SIZE)),
            BoxSpec::default(),
            move || TabIcon(icon.clone(), icon_style, color, icon_scale),
        );
        Box(
            Modifier::empty()
                .offset(0.0, 35.0)
                .size(Size::new(geometry.cell_width, 12.0)),
            BoxSpec::default().content_alignment(Alignment::CENTER),
            move || {
                Text(label.clone(), Modifier::empty(), label_style.clone());
            },
        );
    });
}

#[cfg(test)]
#[path = "tests/tab_cells_tests.rs"]
mod tests;
