// Media3-aligned Play / Pause / Seek / Speed / Mute transport bar and interactive drag/click
// seek scrubber shared by both the Video Wall and the TikTok-style Reels gallery.

use std::time::Duration;

use cranpose::prelude::{
    rememberMuteButtonState, rememberPlayPauseButtonState, rememberPlaybackSpeedState,
    rememberPlayerError, rememberPlayerState, rememberProgressStateWithTickInterval, Player,
    PlayerState,
};
use cranpose_core::rememberMutableStateOf;
use cranpose_ui::{
    composable,
    text::FontWeight,
    widgets::{FlowRow, FlowSpec, Slider, SliderSpec},
    Box, BoxSpec, Brush, Color, Column, ColumnSpec, CornerRadii, LinearArrangement, Modifier, Row,
    RowSpec, Text, VerticalAlignment,
};
use cranpose_ui_graphics::{Point, Rect, Size};

use super::controls::{styled_text, ChipButton, ChipTone};

const BG_TRANSPORT: Color = Color(0.06, 0.08, 0.13, 0.90);
const TRACK_BG: Color = Color(1.0, 1.0, 1.0, 0.18);
const TRACK_BUFFERED: Color = Color(0.58, 0.72, 0.96, 0.42);
const TRACK_PLAYED: Color = Color::from_rgb_u8(0x38, 0xBD, 0xF8);
const THUMB_FILL: Color = Color::WHITE;
const TEXT_PRIMARY: Color = Color::from_rgb_u8(0xF3, 0xF6, 0xFC);
const TEXT_MUTED: Color = Color::from_rgb_u8(0x94, 0xA3, 0xB8);
const TEXT_ERROR: Color = Color::from_rgb_u8(0xF8, 0x71, 0x71);

const SEEK_STEP_SECS: u64 = 10;
const SPEED_STEPS: [f32; 4] = [0.5, 1.0, 1.5, 2.0];

pub(super) fn next_playback_speed(current: f32) -> f32 {
    for &step in &SPEED_STEPS {
        if step > current + 0.05 {
            return step;
        }
    }
    SPEED_STEPS[0]
}

pub(super) fn format_speed(speed: f32) -> String {
    let raw = format!("{speed:.1}");
    format!("Speed: {}x", raw.trim_end_matches(".0"))
}

pub(super) fn format_timecode(duration: Duration) -> String {
    let total = duration.as_secs();
    let hours = total / 3600;
    let mins = (total % 3600) / 60;
    let secs = total % 60;
    if hours > 0 {
        format!("{hours}:{mins:02}:{secs:02}")
    } else {
        format!("{mins}:{secs:02}")
    }
}

pub(super) fn player_status_message(player: &Player, fallback_label: &str) -> (String, Color) {
    let err = rememberPlayerError(player).value();
    let state = rememberPlayerState(player).value();
    if let Some(error) = err {
        return (format!("{fallback_label} · Error: {error}"), TEXT_ERROR);
    }
    let text = match state {
        PlayerState::Idle => format!("{fallback_label} · Idle"),
        PlayerState::Buffering => format!("{fallback_label} · Buffering stream..."),
        PlayerState::Ready => format!("{fallback_label} · Decoding first GPU frame..."),
        PlayerState::Ended => format!("{fallback_label} · Playback ended"),
    };
    (text, TEXT_PRIMARY)
}

#[composable]
pub(super) fn VideoSeekScrubber(player: Player, modifier: Modifier) {
    let progress = rememberProgressStateWithTickInterval(&player, Duration::from_millis(120));
    let scrub_target = rememberMutableStateOf(|| None::<f32>);
    let duration = progress.duration();
    let current = progress.current_position();
    let played_frac = progress.fraction().unwrap_or(0.0).clamp(0.0, 1.0);
    let buffered_frac = progress.buffered_fraction().unwrap_or(0.0).clamp(0.0, 1.0);
    let display_frac = scrub_target.value().unwrap_or(played_frac);
    let seekable = progress.is_seekable() || duration.is_some();
    let shown_pos = match (scrub_target.value(), duration) {
        (Some(frac), Some(dur)) => {
            Duration::from_secs_f64(dur.as_secs_f64() * f64::from(frac.clamp(0.0, 1.0)))
        }
        _ => current,
    };
    let time_label = match duration {
        Some(dur) => format!("{} / {}", format_timecode(shown_pos), format_timecode(dur)),
        None => format!("{} / --:--", format_timecode(shown_pos)),
    };

    Row(
        modifier,
        RowSpec::new()
            .horizontal_arrangement(LinearArrangement::SpacedBy(8.0))
            .vertical_alignment(VerticalAlignment::CenterVertically),
        move || {
            let drag_progress = progress.clone();
            let commit_progress = progress.clone();
            Slider(
                Modifier::empty()
                    .weight(1.0)
                    .height(22.0)
                    .content_description("Video seek bar"),
                display_frac,
                move |next| {
                    scrub_target.set(Some(next));
                    drag_progress.seek_to_fraction(next);
                },
                move || {
                    if let Some(final_frac) = scrub_target.value() {
                        commit_progress.seek_to_fraction(final_frac);
                    }
                    scrub_target.set(None);
                },
                SliderSpec::new().thumb_extent(12.0).enabled(seekable),
                move |scope| {
                    let frac = scope.value().clamp(0.0, 1.0);
                    let thumb_x = scope.thumb_offset();
                    let thumb_drag = if scope.is_dragging() {
                        ThumbDrag::Active
                    } else {
                        ThumbDrag::Resting
                    };
                    Box(
                        Modifier::empty().fill_max_size().draw_behind(move |draw| {
                            draw_seek_track(draw, frac, buffered_frac, thumb_x, thumb_drag);
                        }),
                        BoxSpec::default(),
                        || {},
                    );
                },
            );
            Text(
                time_label.clone(),
                Modifier::empty(),
                styled_text(11.0, TEXT_PRIMARY, Some(FontWeight::MEDIUM)),
            );
        },
    );
}

#[derive(Clone, Copy)]
enum ThumbDrag {
    Resting,
    Active,
}

fn draw_seek_track(
    draw: &mut dyn cranpose_ui::DrawScope,
    frac: f32,
    buffered_frac: f32,
    thumb_x: f32,
    drag: ThumbDrag,
) {
    let sz = draw.size();
    let track_h = 5.0;
    let top = (sz.height - track_h) * 0.5;
    let corner = CornerRadii::uniform(track_h * 0.5);
    draw.draw_round_rect_at(
        Rect::from_origin_size(Point::new(0.0, top), Size::new(sz.width, track_h)),
        Brush::solid(TRACK_BG),
        corner,
    );
    if buffered_frac > 0.001 {
        draw.draw_round_rect_at(
            Rect::from_origin_size(
                Point::new(0.0, top),
                Size::new(sz.width * buffered_frac, track_h),
            ),
            Brush::solid(TRACK_BUFFERED),
            corner,
        );
    }
    if frac > 0.001 {
        draw.draw_round_rect_at(
            Rect::from_origin_size(Point::new(0.0, top), Size::new(sz.width * frac, track_h)),
            Brush::solid(TRACK_PLAYED),
            corner,
        );
    }
    let thumb_d = match drag {
        ThumbDrag::Active => 14.0,
        ThumbDrag::Resting => 11.0,
    };
    let thumb_y = (sz.height - thumb_d) * 0.5;
    let clamped_x = thumb_x.clamp(0.0, (sz.width - thumb_d).max(0.0));
    draw.draw_round_rect_at(
        Rect::from_origin_size(Point::new(clamped_x, thumb_y), Size::new(thumb_d, thumb_d)),
        Brush::solid(THUMB_FILL),
        CornerRadii::uniform(thumb_d * 0.5),
    );
}

#[composable]
pub(super) fn FocusedTransportBar(player: Player, tile_idx: usize, source_license: &'static str) {
    let play_pause = rememberPlayPauseButtonState(&player);
    let mute = rememberMuteButtonState(&player);
    let speed = rememberPlaybackSpeedState(&player);
    let err = rememberPlayerError(&player).value();
    let seek_back_player = player.clone();
    let seek_fwd_player = player.clone();
    let cur_speed = speed.playback_speed();

    Column(
        Modifier::empty()
            .fill_max_width()
            .draw_behind(|scope| {
                scope.draw_round_rect(Brush::solid(BG_TRANSPORT), CornerRadii::uniform(10.0));
            })
            .padding_each(10.0, 6.0, 10.0, 6.0),
        ColumnSpec::new().vertical_arrangement(LinearArrangement::SpacedBy(4.0)),
        move || {
            VideoSeekScrubber(player.clone(), Modifier::empty().fill_max_width());
            let err_line = err.as_ref().map(|e| format!("Stream error: {e}"));
            let row_pp = play_pause.clone();
            let row_sb = seek_back_player.clone();
            let row_sf = seek_fwd_player.clone();
            let row_mute = mute.clone();
            let row_speed = speed.clone();
            FlowRow(
                Modifier::empty().fill_max_width(),
                FlowSpec::new()
                    .main_axis_spacing(6.0)
                    .cross_axis_spacing(4.0),
                move || {
                    let pp = row_pp.clone();
                    ChipButton(
                        if pp.show_play() { "Play" } else { "Pause" },
                        ChipTone::from_active(!pp.show_play()),
                        move || pp.on_click(),
                    );
                    let sb = row_sb.clone();
                    ChipButton("-10s", ChipTone::Idle, move || {
                        let target = sb
                            .current_position()
                            .saturating_sub(Duration::from_secs(SEEK_STEP_SECS));
                        sb.seek_to(target);
                    });
                    let sf = row_sf.clone();
                    ChipButton("+10s", ChipTone::Idle, move || {
                        let next = sf
                            .current_position()
                            .saturating_add(Duration::from_secs(SEEK_STEP_SECS));
                        let clamped = sf.duration().map_or(next, |dur| next.min(dur));
                        sf.seek_to(clamped);
                    });
                    let m = row_mute.clone();
                    ChipButton(
                        if m.is_muted() { "Unmute" } else { "Mute" },
                        ChipTone::from_active(!m.is_muted()),
                        move || m.on_click(),
                    );
                    let sp = row_speed.clone();
                    ChipButton(
                        format_speed(cur_speed),
                        ChipTone::from_active((cur_speed - 1.0).abs() > 0.05),
                        move || {
                            sp.update_playback_speed(next_playback_speed(cur_speed));
                        },
                    );
                    if let Some(msg) = err_line.clone() {
                        Text(
                            msg,
                            Modifier::empty(),
                            styled_text(10.5, TEXT_ERROR, Some(FontWeight::MEDIUM)),
                        );
                    } else {
                        Text(
                            format!("Tile #{} · License: {source_license}", tile_idx + 1),
                            Modifier::empty(),
                            styled_text(10.5, TEXT_MUTED, None),
                        );
                    }
                },
            );
        },
    );
}
