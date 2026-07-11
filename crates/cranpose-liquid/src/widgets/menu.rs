//! A popup menu that morphs out of its anchor: the glass bubble springs from
//! the anchor corner while its items fade in (the WWDC "Show" menu).

use crate::material::{Glass, GlassDynamics, GlassMorph, LiquidModifierExt, LiquidShape};
use crate::theme::{liquid_colors, liquid_typography};
use cranpose_core::{mutableStateOf, remember};
use cranpose_macros::composable;
use cranpose_ui::text::{FontWeight, SpanStyle, TextStyle, TextUnit};
use cranpose_ui::widgets::{
    Box, BoxSpec, Column, ColumnSpec, PopupDismissable, Row, RowSpec, Text,
};
use cranpose_ui::{Modifier, PointerEventKind, PointerInputScope};
use cranpose_ui_graphics::{Brush, CornerRadii, GraphicsLayer, Point, Rect, RenderEffect};
use cranpose_ui_layout::VerticalAlignment;
use std::cell::Cell;
use std::rc::Rc;

/// One menu entry.
#[derive(Clone, Debug, PartialEq)]
pub struct LiquidMenuItem {
    pub label: String,
    /// Optional leading icon (24×24 path data).
    pub icon: Option<&'static str>,
    /// Draws the leading checkmark (selected state).
    pub checked: bool,
    /// Destructive styling.
    pub destructive: bool,
    /// Starts a new visual section (full-width hairline above).
    pub section_start: bool,
    /// A non-interactive gray section header ("Show").
    pub header: bool,
}

impl LiquidMenuItem {
    pub fn new(label: impl Into<String>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            checked: false,
            destructive: false,
            section_start: false,
            header: false,
        }
    }

    /// A gray, non-interactive section header row.
    pub fn header(label: impl Into<String>) -> Self {
        Self {
            header: true,
            ..Self::new(label)
        }
    }

    pub fn icon(mut self, icon: &'static str) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    pub fn destructive(mut self) -> Self {
        self.destructive = true;
        self
    }

    pub fn section_start(mut self) -> Self {
        self.section_start = true;
        self
    }
}

const MENU_WIDTH: f32 = 250.0;
const MENU_RADIUS: f32 = 40.0;
/// How far the card's top edge sits below the anchor's top: the settled menu
/// swallows the anchor button ENTIRELY (the reference "…" disappears under
/// the glass, reading as a smudge; only mid-flight does its bump ride the
/// droplet edge).
const ANCHOR_OVERLAP: f32 = 0.0;
const ROW_PADDING_X: f32 = 16.0;
const ROW_PADDING_Y: f32 = 12.0;
/// Width reserved for the leading checkmark column when any item is checkable.
const CHECK_COLUMN: f32 = 24.0;
const ICON_SIZE: f32 = 20.0;
const ICON_GAP: f32 = 12.0;
/// Content materialization blur (dp) at the start of the morph — rows fade in
/// from behind the glass, sharpening over the flight (reference frames 6-9).
const CONTENT_BLUR: f32 = 10.0;

/// A glass popup menu anchored to `anchor` (window coordinates of the button
/// that opened it). `neighbors` are the window-coord rects of other glass
/// controls near the anchor (the rest of the nav cluster): the growing
/// bubble SDF-glues to them in passing, exactly like the reference
/// keyframes. While `expanded`, taps outside dismiss via `on_dismiss`; item
/// taps call `on_item` with the index then dismiss.
///
/// Layout follows the iOS menu: an optional leading checkmark column (present
/// on every row once any item is checkable, so labels align), then the icon
/// column, then the label. Sections split with full-width hairlines; headers
/// are gray non-interactive rows.
#[composable]
#[allow(non_snake_case)]
pub fn LiquidMenu(
    expanded: bool,
    anchor: Rect,
    neighbors: Vec<Rect>,
    items: Vec<LiquidMenuItem>,
    on_item: impl Fn(usize) + 'static,
    on_dismiss: impl Fn() + 'static,
) {
    // The menu outlives `expanded` by one collapse animation: dismissing
    // deflates the droplet back into the anchor (the reference close morph)
    // before the popup unmounts.
    let visible = remember(|| mutableStateOf(false)).with(|s| *s);
    if expanded && !visible.get() {
        visible.set(true);
    }
    if !expanded && !visible.get() {
        return;
    }
    let colors = liquid_colors();
    let typography = liquid_typography();
    let on_item: Rc<dyn Fn(usize)> = Rc::new(on_item);
    let on_dismiss: Rc<dyn Fn()> = Rc::new(on_dismiss);

    // The droplet spring: opening uses the bouncy morph spring (visible size
    // overshoot, the reference menu swells a few percent past its final width
    // and relaxes); closing is a faster, non-bouncy suck-back.
    // Open ≈330ms press→crisp with a soft overshoot (timed against the
    // reference recording); close is a faster suck-back (~200ms).
    let grow = cranpose_animation::animate_float_as_state_with_initial(
        0.0,
        if expanded { 1.0 } else { 0.0 },
        if expanded {
            cranpose_animation::spring(0.58, 380.0)
        } else {
            cranpose_animation::spring(1.0, 1100.0)
        },
        "menu-grow",
    );
    // Content reveal runs on its OWN slower clock: the droplet reaches full
    // size while the rows are still smudges, and they crisp only around
    // settle (the reference keeps content illegible until the last beat).
    // Closing snaps the blur back on fast.
    let reveal_anim = cranpose_animation::animate_float_as_state_with_initial(
        0.0,
        if expanded { 1.0 } else { 0.0 },
        if expanded {
            cranpose_animation::spring(1.0, 110.0)
        } else {
            cranpose_animation::spring(1.0, 900.0)
        },
        "menu-reveal",
    );
    // Body-level read: each animation frame recomposes this menu, which
    // re-registers fresh popup content (see `Popup`), driving the morph.
    // NOT clamped at 1 — the spring's overshoot is the size overshoot.
    let appear = grow.get().max(0.0);
    let reveal = reveal_anim.get().clamp(0.0, 1.0);
    if !expanded && appear < 0.02 {
        visible.set(false);
        return;
    }

    // The node spans from the anchor's top; ANCHOR_OVERLAP places the card's
    // top edge so the settled glass swallows the anchor button entirely.
    let anchor_zone = anchor.height * ANCHOR_OVERLAP;
    let node_size =
        remember(|| Rc::new(Cell::new(cranpose_ui_graphics::Size::ZERO))).with(Rc::clone);
    // Finite-difference morph velocity (per render frame) drives the wobble
    // and the viscous leading-edge bulge — reactive, not canned.
    let last_appear = remember(|| Rc::new(Cell::new(f32::NAN))).with(Rc::clone);

    // Right-align the card under the anchor (menus morph out of trailing
    // buttons), staying on-screen for anchors near the right edge. The host
    // renders the outside-tap scrim (PopupDismissable). While collapsing the
    // scrim must not re-fire the caller's dismiss.
    let scrim_dismiss = Rc::clone(&on_dismiss);
    let scrim_active = expanded;
    PopupDismissable(
        anchor,
        Point::new(anchor.width - MENU_WIDTH, 0.0),
        move || {
            if scrim_active {
                scrim_dismiss()
            }
        },
        {
            let items = items.clone();
            let typography = typography.clone();
            let on_item = Rc::clone(&on_item);
            let on_dismiss = Rc::clone(&on_dismiss);
            let node_size = Rc::clone(&node_size);
            let last_appear = Rc::clone(&last_appear);
            move || {
                // Shapeshift (the WWDC menu-open keyframes): the anchor
                // bubble inflates into the menu card as ONE droplet — the
                // anchor is first carved out (crisp button riding the
                // droplet edge), then swallowed, then melted flat into the
                // settled edge. Neighbors join the field only while the
                // growing edge is actually near them, so the settled menu
                // never halos far-away controls.
                let anchor_center = (MENU_WIDTH - anchor.width * 0.5, anchor.height * 0.5);
                let anchor_shape = (
                    anchor_center.0,
                    anchor_center.1,
                    anchor.width,
                    anchor.height,
                    -1.0,
                );
                // Node origin in window coords (the popup offset from the
                // anchor) converts neighbor rects into node-local shapes.
                let node_origin = (anchor.x + anchor.width - MENU_WIDTH, anchor.y);
                let neighbor_shapes_for_dyn: Vec<(f32, f32, f32, f32, f32)> = neighbors
                    .iter()
                    .map(|rect| {
                        (
                            rect.x + rect.width * 0.5 - node_origin.0,
                            rect.y + rect.height * 0.5 - node_origin.1,
                            rect.width,
                            rect.height,
                            -1.0,
                        )
                    })
                    .collect();
                let morph_size = Rc::clone(&node_size);
                let morph_last_appear = Rc::clone(&last_appear);
                // Muted vibrancy: the absorbed button must read as a soft
                // smudge beneath the glass, not a hot saturated orb.
                let glass = Glass::regular()
                    .shape(LiquidShape::RoundedRect(MENU_RADIUS))
                    .saturation(1.2)
                    .lift(0.34)
                    .no_clip()
                    .shadow(false);
                let card = Modifier::empty()
                    .report_size(Rc::clone(&node_size))
                    .glass_effect_with(glass, move || {
                        let size = morph_size.get();
                        let menu_h = (size.height - anchor_zone).max(24.0);
                        // Shaped spring: the reference droplet lingers near
                        // the button for the first ~100ms (icon fades, slight
                        // swell) and inflates late — an ease-in on the spring
                        // value reproduces that while keeping the overshoot
                        // past 1 (which IS the size overshoot at settle).
                        let t = if appear < 1.0 {
                            appear.powf(1.6)
                        } else {
                            appear
                        };
                        let start = anchor_shape;
                        // Pillowy settled corners (the reference menu's radius
                        // is ~0.26 of its height — far rounder than a desktop
                        // popup).
                        let settle_radius = (menu_h * 0.26).clamp(MENU_RADIUS, 64.0);
                        let target = (
                            MENU_WIDTH * 0.5,
                            anchor_zone + menu_h * 0.5,
                            MENU_WIDTH,
                            menu_h,
                            settle_radius,
                        );
                        let start_radius = anchor.width.min(anchor.height) * 0.5;
                        let lerp = |a: f32, b: f32| a + (b - a) * t;
                        // Width LAGS height: the reference droplet inflates as
                        // a near-circle first and only stretches into the wide
                        // stadium late in the flight.
                        let t_w = t.powf(1.9);
                        let lerp_w = |a: f32, b: f32| a + (b - a) * t_w;
                        let grown_w = lerp_w(start.2, target.2).max(anchor.width * 0.6);
                        let grown_h = lerp(start.3, target.3).max(anchor.height * 0.6);
                        // The reference droplet stays BLOB-ROUND while it
                        // inflates (mid-flight it is nearly circular — radius
                        // tracking the half-extent) and only squares into the
                        // menu's rounded rect late in the flight.
                        let blob_radius = grown_w.min(grown_h) * 0.5;
                        let squareness = ((t - 0.55) / 0.45).clamp(0.0, 1.0);
                        let radius = if t >= 1.0 {
                            target.4
                        } else {
                            let base = start_radius + (blob_radius - start_radius) * t;
                            base + (target.4 - base) * squareness * squareness
                        };
                        let primary = (
                            lerp(start.0, target.0),
                            lerp(start.1, target.1),
                            grown_w,
                            grown_h,
                            radius,
                        );
                        // Morph velocity (per frame): wobble and the viscous
                        // leading-edge bulge follow the actual motion — fast
                        // growth bubbles hard, the settle calms down, the
                        // close morph bulges back toward the anchor.
                        let prev = morph_last_appear.replace(t);
                        let v = if prev.is_nan() { 0.0 } else { t - prev };
                        let speed = v.abs();
                        // Growth direction from the anchor toward the card
                        // center (node coords, y down).
                        let dir_x = target.0 - start.0;
                        let dir_y = target.1 - start.1;
                        let mut bulge_dir = dir_y.atan2(dir_x);
                        if v < 0.0 {
                            bulge_dir += std::f32::consts::PI;
                        }
                        let mut shapes = Vec::new();
                        // The anchor button rides INSIDE the droplet (the
                        // reference dots fade under the growing glass, they
                        // are never carved out); a melting union bump keeps
                        // the birth edge organic and leaves a flat settled
                        // edge. The close morph walks it backwards.
                        let melt = (1.0 - t).clamp(0.0, 1.0);
                        if melt > 0.01 {
                            shapes.push((
                                anchor_shape.0,
                                anchor_shape.1,
                                anchor_shape.2 * melt.max(0.4),
                                anchor_shape.3 * melt.max(0.4),
                                -1.0,
                            ));
                        }
                        // Neighbors participate only while the droplet's
                        // edge is within glue reach of them MID-FLIGHT —
                        // passing glue, never a settled lump (the settled
                        // reference shows neighbors as smudges under the
                        // glass, not edge geometry).
                        let glue = 22.0 * (1.0 - t.clamp(0.0, 1.0) * 0.65);
                        for neighbor in &neighbor_shapes_for_dyn {
                            let (nx, ny, nw, nh, _) = *neighbor;
                            let half_w = primary.2 * 0.5;
                            let half_h = primary.3 * 0.5;
                            let dx = ((nx - primary.0).abs() - half_w - nw * 0.5).max(0.0);
                            let dy = ((ny - primary.1).abs() - half_h - nh * 0.5).max(0.0);
                            let gap = (dx * dx + dy * dy).sqrt();
                            if gap >= glue * 2.2 {
                                continue;
                            }
                            if reveal < 0.45 {
                                // Metaball phase: the neighbor's glass HOST
                                // joins the field (union halo necks with the
                                // droplet) while the button itself is carved
                                // crisp — the reference keeps the blue filter
                                // a sharp circle with its icon until the
                                // glass truly swallows it.
                                shapes.push((nx, ny, nw + 12.0, nh + 12.0, -1.0));
                                shapes.push((nx, ny, nw - 4.0, nh - 4.0, -2.0));
                            } else if t < 0.85 {
                                // Swallowed: melt to a smudge under the glass.
                                let swallow = 1.0 - ((reveal - 0.45) / 0.35).clamp(0.0, 1.0);
                                if swallow > 0.01 {
                                    shapes.push((
                                        nx,
                                        ny,
                                        nw * swallow.max(0.3),
                                        nh * swallow.max(0.3),
                                        -1.0,
                                    ));
                                }
                            }
                        }
                        GlassDynamics {
                            morph: Some(GlassMorph {
                                node_size: (size.width.max(1.0), size.height.max(1.0)),
                                primary,
                                shapes,
                                glue,
                                wobble_amplitude: (speed * 220.0).min(6.5),
                                wobble_phase: t * 8.0,
                                bulge_amplitude: (speed * 340.0).min(10.0),
                                bulge_direction: bulge_dir,
                            }),
                            ..Default::default()
                        }
                    })
                    .width(MENU_WIDTH);

                let has_checks = items.iter().any(|item| item.checked);
                // Finger/pointer sliding through the menu highlights the row
                // under it (release selects) — the iOS drag-through-menu.
                let hovered = remember(|| mutableStateOf(Option::<usize>::None)).with(|s| *s);
                Column(card, ColumnSpec::default(), {
                    let items = items.clone();
                    let typography = typography.clone();
                    let on_item = Rc::clone(&on_item);
                    let on_dismiss = Rc::clone(&on_dismiss);
                    move || {
                        Box(
                            Modifier::empty().height(anchor_zone),
                            BoxSpec::default(),
                            || {},
                        );
                        // The card's drop shadow belongs to the menu rect
                        // only (the glass node also spans the anchor zone).
                        // Soft and wide — the reference menu shadow is a
                        // whisper, not a hard blob. Content stays ABSENT
                        // through the droplet's first stretch and only
                        // materializes late (the reference droplet is blank
                        // at 25%, smudges at 60%, crisp in the last stretch)
                        // — and never renders outside the droplet.
                        // Content is ABSENT through most of the flight (the
                        // reference droplet is empty glass until the last
                        // stretch) and, while CLOSING, rides the fast glass
                        // clock so it collapses WITH the panel instead of
                        // dissolving in place.
                        let content = if expanded {
                            ((reveal - 0.62) / 0.38).clamp(0.0, 1.0).powf(1.2)
                        } else {
                            ((appear - 0.45) / 0.55).clamp(0.0, 1.0)
                        };
                        let shadow_color = if colors.is_dark {
                            cranpose_ui_graphics::Color::BLACK.with_alpha(0.45 * content)
                        } else {
                            cranpose_ui_graphics::Color::BLACK.with_alpha(0.13 * content)
                        };
                        // Rows materialize from behind the glass: heavily
                        // blurred and ghosted early in the flight, sharp at
                        // settle (and back to blur during the close morph).
                        // They also SCALE with the droplet from the anchor
                        // corner — the content lives on the growing surface,
                        // it doesn't fade in at full size.
                        let content_blur = CONTENT_BLUR * (1.0 - content).max(0.0);
                        let content_scale = 0.55 + 0.45 * content;
                        let rows_wrap = Modifier::empty()
                            .fill_max_width()
                            .graphics_layer(move || GraphicsLayer {
                                alpha: content.powf(1.3),
                                scale_x: content_scale,
                                scale_y: content_scale,
                                transform_origin: cranpose_ui_graphics::TransformOrigin {
                                    pivot_fraction_x: 1.0,
                                    pivot_fraction_y: 0.0,
                                },
                                render_effect: (content_blur > 0.4)
                                    .then(|| RenderEffect::blur(content_blur)),
                                ..Default::default()
                            })
                            .drop_shadow(
                                cranpose_ui_graphics::LayerShape::Rounded(
                                    cranpose_ui_graphics::RoundedCornerShape::uniform(MENU_RADIUS),
                                ),
                                move |scope| {
                                    scope.radius = 34.0;
                                    scope.spread = 0.0;
                                    scope.offset.y = 10.0;
                                    scope.color = shadow_color;
                                    scope.cutout = true;
                                },
                            );
                        Box(rows_wrap, BoxSpec::default(), {
                            let items = items.clone();
                            let typography = typography.clone();
                            let on_item = Rc::clone(&on_item);
                            let on_dismiss = Rc::clone(&on_dismiss);
                            move || {
                                Column(
                                    Modifier::empty().fill_max_width(),
                                    ColumnSpec::default(),
                                    {
                                        let items = items.clone();
                                        let typography = typography.clone();
                                        let on_item = Rc::clone(&on_item);
                                        let on_dismiss = Rc::clone(&on_dismiss);
                                        move || {
                                            for (index, item) in items.iter().enumerate() {
                                                if item.section_start && index > 0 {
                                                    // Whisper-subtle: the
                                                    // reference surface reads
                                                    // nearly seamless.
                                                    let separator = colors
                                                        .separator
                                                        .with_alpha(colors.separator.a() * 0.22);
                                                    Box(
                                    Modifier::empty()
                                        .fill_max_width()
                                        .padding_symmetric(ROW_PADDING_X, 0.0)
                                        .height(1.0)
                                        .draw_behind(move |scope| {
                                            scope.draw_rect(cranpose_ui_graphics::Brush::solid(
                                                separator,
                                            ));
                                        }),
                                    BoxSpec::default(),
                                    || {},
                                );
                                                }

                                                if item.header {
                                                    menu_header_row(
                                                        item,
                                                        &typography,
                                                        has_checks,
                                                        colors,
                                                    );
                                                    continue;
                                                }
                                                menu_item_row(
                                                    index,
                                                    item,
                                                    &typography,
                                                    has_checks,
                                                    colors,
                                                    hovered,
                                                    Rc::clone(&on_item),
                                                    Rc::clone(&on_dismiss),
                                                );
                                            }
                                        }
                                    },
                                );
                            }
                        });
                    }
                });
            }
        },
    );
}

/// Gray non-interactive section header, aligned with the icon column.
fn menu_header_row(
    item: &LiquidMenuItem,
    typography: &crate::theme::LiquidTypography,
    has_checks: bool,
    colors: crate::theme::LiquidColors,
) {
    let label = item.label.clone();
    let style = TextStyle {
        span_style: SpanStyle {
            color: Some(colors.secondary_label),
            font_size: TextUnit::Sp(13.0),
            ..typography.footnote.span_style.clone()
        },
        ..typography.footnote.clone()
    };
    let indent = ROW_PADDING_X + if has_checks { CHECK_COLUMN } else { 0.0 };
    let row = Modifier::empty()
        .fill_max_width()
        .padding_each(indent, 12.0, ROW_PADDING_X, 2.0);
    Row(row, RowSpec::default(), move || {
        Text(label.clone(), Modifier::empty(), style.clone());
    });
}

/// One interactive menu row: [check][icon][label].
#[allow(non_snake_case)]
#[allow(clippy::too_many_arguments)]
fn menu_item_row(
    index: usize,
    item: &LiquidMenuItem,
    typography: &crate::theme::LiquidTypography,
    has_checks: bool,
    colors: crate::theme::LiquidColors,
    hovered: cranpose_core::MutableState<Option<usize>>,
    on_item: Rc<dyn Fn(usize)>,
    on_dismiss: Rc<dyn Fn()>,
) {
    let color = if item.destructive {
        colors.destructive
    } else {
        colors.label
    };
    let is_hovered = hovered.get() == Some(index);
    let highlight = if colors.is_dark {
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 72)
    } else {
        cranpose_ui_graphics::Color::from_rgba_u8(120, 120, 128, 48)
    };
    let row_label = item.label.clone();
    let row = Modifier::empty()
        .fill_max_width()
        .semantics(move |config| {
            config.is_button = true;
            config.is_clickable = true;
            config.content_description = Some(row_label.clone());
        })
        .pointer_input(index, {
            let on_item = Rc::clone(&on_item);
            let on_dismiss = Rc::clone(&on_dismiss);
            move |scope: PointerInputScope| {
                let on_item = Rc::clone(&on_item);
                let on_dismiss = Rc::clone(&on_dismiss);
                async move {
                    scope
                        .await_pointer_event_scope(|await_scope| async move {
                            loop {
                                let event = await_scope.await_pointer_event().await;
                                match event.kind {
                                    PointerEventKind::Enter | PointerEventKind::Move => {
                                        hovered.set(Some(index));
                                    }
                                    PointerEventKind::Exit if hovered.get() == Some(index) => {
                                        hovered.set(None);
                                    }
                                    PointerEventKind::Down => {
                                        hovered.set(Some(index));
                                        event.consume();
                                    }
                                    PointerEventKind::Up => {
                                        hovered.set(None);
                                        on_item(index);
                                        on_dismiss();
                                        event.consume();
                                    }
                                    _ => {}
                                }
                            }
                        })
                        .await;
                }
            }
        })
        .draw_behind(move |scope| {
            if is_hovered {
                scope.draw_round_rect(Brush::solid(highlight), CornerRadii::uniform(14.0));
            }
        })
        .padding_symmetric(ROW_PADDING_X, ROW_PADDING_Y);

    let label = item.label.clone();
    let icon = item.icon;
    let checked = item.checked;
    let typography = typography.clone();
    Row(
        row,
        RowSpec::default().vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let label = label.clone();
            if has_checks {
                // Leading checkmark column, reserved on every row so icons
                // and labels align (the reference "Show" menu).
                Box(
                    Modifier::empty().width(CHECK_COLUMN),
                    BoxSpec::default(),
                    move || {
                        if checked {
                            crate::icons::Icon(crate::icons::CHECK, 16.0, color);
                        }
                    },
                );
            }
            if let Some(icon) = icon {
                crate::icons::Icon(icon, ICON_SIZE, color);
                Box(Modifier::empty().width(ICON_GAP), BoxSpec::default(), || {});
            }
            let style = TextStyle {
                span_style: SpanStyle {
                    color: Some(color),
                    font_weight: Some(FontWeight::NORMAL),
                    ..typography.body.span_style.clone()
                },
                ..typography.body.clone()
            };
            Text(label, Modifier::empty().weight(1.0), style);
        },
    );
}
