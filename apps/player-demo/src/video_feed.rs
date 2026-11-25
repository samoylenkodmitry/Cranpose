// Vertical short-form video feed showcase exercising VerticalPager, PlayerPool preloading,
// ContentFrame shutter handoff, and Media3 unstyled control state holders.

use std::time::Duration;

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;

use crate::sized;

#[derive(Clone, PartialEq)]
struct FeedEntry {
    key: String,
    title: &'static str,
    subtitle: &'static str,
    poster_color: Color,
    item: MediaItem,
}

fn sample_feed() -> Vec<FeedEntry> {
    let palette = [
        (
            "Ocean Drift",
            "HLS 1080p60 · Zero-copy GPU",
            Color(0.08, 0.20, 0.34, 1.0),
        ),
        (
            "Neon Night",
            "HEVC 10-bit PQ · HDR tone-map",
            Color(0.24, 0.10, 0.30, 1.0),
        ),
        (
            "Alpine Run",
            "H.264 60 fps · Preloaded Next",
            Color(0.08, 0.26, 0.22, 1.0),
        ),
        (
            "Studio Light",
            "AV1 / HEVC · Instant handoff",
            Color(0.28, 0.18, 0.08, 1.0),
        ),
        (
            "City Rain",
            "Adaptive HLS · Master clock",
            Color(0.12, 0.16, 0.28, 1.0),
        ),
    ];
    (0..20)
        .map(|index| {
            let (title, subtitle, poster_color) = palette[index % palette.len()];
            let uri = format!("https://example.invalid/feed/clip-{index}.m3u8");
            FeedEntry {
                key: format!("feed-{index}"),
                title,
                subtitle,
                poster_color,
                item: MediaItem::new(uri)
                    .with_mime_type("application/x-mpegURL")
                    .with_metadata(MediaMetadata::titled(format!("{title} #{index}"))),
            }
        })
        .collect()
}

#[composable]
pub fn VerticalVideoFeed() {
    let entries = remember(sample_feed).with(Clone::clone);
    let pool_items = remember({
        let entries = entries.clone();
        move || {
            entries
                .iter()
                .map(|entry| (entry.key.clone(), entry.item.clone()))
                .collect::<Vec<_>>()
        }
    })
    .with(Clone::clone);
    let pool = rememberPlayerPool(PlayerPoolConfig {
        max_concurrent_players: 3,
        load_control: LoadControl::FEED,
        preload_ahead: 1,
        preload_behind: 1,
    });
    let pager = rememberPagerState(0);
    let current_page = pager.current_page();

    let sync_pool = pool.clone();
    LaunchedEffect(current_page, async move |_| {
        if let Err(err) = sync_pool.update_feed_window(&pool_items, current_page) {
            log::warn!("video feed: update pool window failed: {err}");
        }
    });

    let count = entries.len();
    VerticalPager(
        Modifier::empty().fill_max_size(),
        pager,
        PagerSpec::default(),
        count,
        move |page| {
            FeedCard(entries[page].clone(), pool.clone(), page, count);
        },
    );
}

#[composable]
fn FeedCard(entry: FeedEntry, pool: PlayerPool, page: usize, total: usize) {
    let player = pool.player_for(&entry.key);
    let poster = entry.poster_color;
    let title = entry.title;
    let subtitle = entry.subtitle;
    let active_slots = pool.active_count();

    Box(
        Modifier::empty()
            .fill_max_size()
            .background(Color(0.04, 0.05, 0.07, 1.0)),
        BoxSpec::new().content_alignment(Alignment::BOTTOM_START),
        move || {
            if let Some(active) = player.clone() {
                ContentFrame(
                    active.clone(),
                    Modifier::empty().fill_max_size(),
                    ContentScale::Crop,
                    move || {
                        PosterShutter(poster, title, subtitle);
                    },
                );
                FeedOverlayControls(active, title, subtitle, page, total, active_slots);
            } else {
                PosterShutter(poster, title, subtitle);
            }
        },
    );
}

#[composable]
fn PosterShutter(color: Color, title: &'static str, subtitle: &'static str) {
    Box(
        Modifier::empty().fill_max_size().background(color),
        BoxSpec::new().content_alignment(Alignment::CENTER),
        move || {
            Column(
                Modifier::empty().padding(24.0),
                ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
                move || {
                    Text(
                        title,
                        Modifier::empty(),
                        sized(30.0, Some(FontWeight::BOLD)),
                    );
                    Text(subtitle, Modifier::empty(), sized(16.0, None));
                },
            );
        },
    );
}

#[composable]
fn FeedOverlayControls(
    player: Player,
    title: &'static str,
    subtitle: &'static str,
    page: usize,
    total: usize,
    active_slots: usize,
) {
    let play_pause = rememberPlayPauseButtonState(&player);
    let mute = rememberMuteButtonState(&player);
    let progress = rememberProgressStateWithTickInterval(&player, Duration::from_millis(100));

    let play_label = if play_pause.show_play() {
        "Play"
    } else {
        "Pause"
    };
    let mute_label = if mute.is_muted() { "Unmute" } else { "Mute" };
    let progress_pct = format!(
        "{:.0}",
        (progress.fraction().unwrap_or(0.0) * 100.0).clamp(0.0, 100.0)
    );

    Column(
        Modifier::empty()
            .fill_max_width()
            .background(Color(0.0, 0.0, 0.0, 0.45))
            .padding(20.0),
        ColumnSpec::default().vertical_arrangement(LinearArrangement::spaced_by(8.0)),
        move || {
            Text(
                format!("{title}  ({}/{total})", page + 1),
                Modifier::empty(),
                sized(22.0, Some(FontWeight::BOLD)),
            );
            Text(
                format!("{subtitle}  ·  pool {active_slots}/3  ·  {progress_pct}%"),
                Modifier::empty(),
                sized(14.0, None),
            );
            let (pp_row, m_row) = (play_pause.clone(), mute.clone());
            Row(
                Modifier::empty(),
                RowSpec::default().horizontal_arrangement(LinearArrangement::spaced_by(12.0)),
                move || {
                    let pp = pp_row.clone();
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || pp.on_click(),
                        move || {
                            Text(
                                play_label,
                                Modifier::empty(),
                                sized(15.0, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                    let m = m_row.clone();
                    Button(
                        Modifier::empty(),
                        ButtonSpec::default(),
                        move || m.on_click(),
                        move || {
                            Text(
                                mute_label,
                                Modifier::empty(),
                                sized(15.0, Some(FontWeight::BOLD)),
                            );
                        },
                    );
                },
            );
        },
    );
}
