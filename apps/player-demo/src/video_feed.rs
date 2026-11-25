// Vertical short-form video feed showcase exercising VerticalPager, PlayerPool preloading,
// ContentFrame shutter handoff, and Media3 unstyled control state holders with real bundled
// H.264/AAC MP4 clips playing zero-copy on every shipped GPU platform.

use std::{sync::Arc, time::Duration};

use cranpose::prelude::*;
use cranpose_ui::text::FontWeight;
use web_time::Instant;

use crate::sized;

const CLIP_1_MP4: &[u8] = include_bytes!("../assets/feed_clip_1.mp4");
const CLIP_2_MP4: &[u8] = include_bytes!("../assets/feed_clip_2.mp4");
const CLIP_3_MP4: &[u8] = include_bytes!("../assets/feed_clip_3.mp4");

#[derive(Clone, PartialEq)]
struct FeedEntry {
    key: String,
    title: &'static str,
    subtitle: &'static str,
    poster_color: Color,
    item: MediaItem,
}

fn bundled_feed() -> Vec<FeedEntry> {
    let clips: [Arc<[u8]>; 3] = [
        Arc::from(CLIP_1_MP4),
        Arc::from(CLIP_2_MP4),
        Arc::from(CLIP_3_MP4),
    ];
    let palette = [
        (
            "Ocean Drift",
            "H.264 9:16 · Zero-copy GPU",
            Color(0.08, 0.20, 0.34, 1.0),
        ),
        (
            "Neon Night",
            "H.264 Color Sweep · Preloaded Next",
            Color(0.24, 0.10, 0.30, 1.0),
        ),
        (
            "Alpine Run",
            "H.264 RGB Motion · Instant handoff",
            Color(0.08, 0.26, 0.22, 1.0),
        ),
        (
            "Studio Light",
            "Zero-copy GPU · Master audio clock",
            Color(0.28, 0.18, 0.08, 1.0),
        ),
        (
            "City Rain",
            "PlayerPool 3 slots · Bounded memory",
            Color(0.12, 0.16, 0.28, 1.0),
        ),
    ];
    (0..20)
        .map(|index| {
            let (title, subtitle, poster_color) = palette[index % palette.len()];
            let bytes = Arc::clone(&clips[index % clips.len()]);
            FeedEntry {
                key: format!("feed-{index}"),
                title,
                subtitle,
                poster_color,
                item: MediaItem::from_bytes(bytes, "video/mp4")
                    .with_metadata(MediaMetadata::titled(format!("{title} #{index}"))),
            }
        })
        .collect()
}

fn auto_swipe_interval() -> Option<Duration> {
    let ms = launch_args().int("auto_swipe_ms")?;
    u64::try_from(ms)
        .ok()
        .filter(|&ms| ms >= 100)
        .map(Duration::from_millis)
}

#[composable]
pub fn VerticalVideoFeed() {
    let entries = remember(bundled_feed).with(Clone::clone);
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
    let pool_version = rememberMutableStateOf(|| 0_u64);
    let pager = rememberPagerState(0);
    let current_page = pager.current_page();

    let mute_pool = pool.clone();
    LaunchedEffect((), async move |_| {
        let start_muted = launch_args()
            .boolean("muted")
            .unwrap_or(cfg!(target_arch = "wasm32"));
        if start_muted {
            mute_pool.set_user_muted(true);
        }
    });

    let sync_pool = pool.clone();
    LaunchedEffect(current_page, async move |_| {
        if let Err(err) = sync_pool.update_feed_window(&pool_items, current_page) {
            log::warn!("video feed: update pool window failed: {err}");
        } else {
            if let Some((key, _)) = pool_items.get(current_page)
                && let Some(active) = sync_pool.player_for(key)
            {
                active.set_repeat_mode(RepeatMode::One);
            }
            pool_version.set(pool_version.value().saturating_add(1));
        }
    });

    let count = entries.len();
    if let Some(swipe_every) = auto_swipe_interval() {
        let auto_pager = pager;
        LaunchedEffect(swipe_every, async move |_| {
            let mut next = 0_usize;
            loop {
                delay(swipe_every).await;
                next = (next + 1) % count;
                auto_pager.animate_scroll_to_page(next);
            }
        });
    }

    VerticalPager(
        Modifier::empty().fill_max_size(),
        pager,
        PagerSpec::default(),
        count,
        move |page| {
            FeedCard(
                entries[page].clone(),
                pool.clone(),
                pool_version,
                page,
                count,
            );
        },
    );
}

#[composable]
fn FeedCard(
    entry: FeedEntry,
    pool: PlayerPool,
    pool_version: MutableState<u64>,
    page: usize,
    total: usize,
) {
    let _version = pool_version.value();
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
                let presentation = rememberPresentationState(&active);
                let opened_at = remember(Instant::now).with(|t| *t);
                let logged_ttff = rememberMutableStateOf(|| false);
                if !presentation.cover_surface() && !logged_ttff.value() {
                    logged_ttff.set(true);
                    log::info!(
                        "video_feed card={} ttff_ms={:.1} pool_slots={active_slots}",
                        page + 1,
                        opened_at.elapsed().as_secs_f64() * 1000.0
                    );
                }
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
