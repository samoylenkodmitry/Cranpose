// Stress-test presets, verified open-media catalog with per-source licensing metadata,
// and instant-play built-in offline clips for zero-network startup across every platform.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use cranpose::prelude::{MediaItem, MediaMetadata};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum StressScenario {
    CinematicHero,
    Single4k120,
    Quad4k60,
    Wall16x1080p60,
    Single8k60,
    Wall64x720p60,
}

impl StressScenario {
    pub(super) const ALL: [Self; 6] = [
        Self::CinematicHero,
        Self::Single4k120,
        Self::Quad4k60,
        Self::Wall16x1080p60,
        Self::Single8k60,
        Self::Wall64x720p60,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::CinematicHero => "Cinematic",
            Self::Single4k120 => "1x 4K 120",
            Self::Quad4k60 => "4x 4K 60",
            Self::Wall16x1080p60 => "16x 1080p",
            Self::Single8k60 => "1x 8K 60",
            Self::Wall64x720p60 => "64x 720p Wall",
        }
    }

    pub(super) const fn grid(self) -> (usize, usize) {
        match self {
            Self::CinematicHero | Self::Single4k120 | Self::Single8k60 => (1, 1),
            Self::Quad4k60 => (2, 2),
            Self::Wall16x1080p60 => (4, 4),
            Self::Wall64x720p60 => (8, 8),
        }
    }

    pub(super) const fn tile_count(self) -> usize {
        let (cols, rows) = self.grid();
        cols * rows
    }

    // Target decoded pixels/sec for each stress scenario.
    pub(super) const fn target_pixels_per_sec(self) -> u64 {
        match self {
            Self::CinematicHero => 3840 * 2160 * 60,
            Self::Single4k120 => 3840 * 2160 * 120,
            Self::Quad4k60 => 4 * 3840 * 2160 * 60,
            Self::Wall16x1080p60 => 16 * 1920 * 1080 * 60,
            Self::Single8k60 => 7680 * 4320 * 60,
            Self::Wall64x720p60 => 64 * 1280 * 720 * 60,
        }
    }

    pub(super) const fn target_label(self) -> &'static str {
        match self {
            Self::CinematicHero => "4K HDR · 498M px/s",
            Self::Single4k120 => "1x 4K 120 · 995M px/s",
            Self::Quad4k60 => "4x 4K 60 · 1.99B px/s",
            Self::Wall16x1080p60 => "16x 1080p 60 · 1.99B px/s",
            Self::Single8k60 => "1x 8K 60 · 1.99B px/s",
            Self::Wall64x720p60 => "64x 720p 60 · 3.54B px/s",
        }
    }

    pub(super) const fn default_source_id(self) -> &'static str {
        match self {
            Self::CinematicHero
            | Self::Single4k120
            | Self::Quad4k60
            | Self::Wall16x1080p60
            | Self::Single8k60
            | Self::Wall64x720p60 => "builtin-pack",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WallLayout {
    Grid,
    MosaicWave,
    Carousel3d,
    Spotlight,
}

impl WallLayout {
    pub(super) const ALL: [Self; 4] = [
        Self::Grid,
        Self::MosaicWave,
        Self::Carousel3d,
        Self::Spotlight,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::MosaicWave => "Wave",
            Self::Carousel3d => "3D Ring",
            Self::Spotlight => "Spotlight",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DecoderConcurrency {
    Auto,
    Count(usize),
}

impl DecoderConcurrency {
    pub(super) const PRESETS: [Self; 6] = [
        Self::Auto,
        Self::Count(4),
        Self::Count(8),
        Self::Count(16),
        Self::Count(32),
        Self::Count(64),
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Count(4) => "4 Dec",
            Self::Count(8) => "8 Dec",
            Self::Count(16) => "16 Dec",
            Self::Count(32) => "32 Dec",
            Self::Count(64) => "64 Dec",
            Self::Count(_) => "Custom",
        }
    }

    pub(super) const fn resolved(self, tile_count: usize) -> usize {
        let cap = match self {
            Self::Auto => platform_default_decoder_cap(),
            Self::Count(n) => n,
        };
        if tile_count < cap {
            tile_count
        } else {
            cap
        }
    }
}

const fn platform_default_decoder_cap() -> usize {
    #[cfg(any(target_os = "android", target_os = "ios", target_arch = "wasm32"))]
    {
        4
    }
    #[cfg(not(any(target_os = "android", target_os = "ios", target_arch = "wasm32")))]
    {
        16
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum LicenseKind {
    // Commercially permissible with attribution or permissive terms.
    PermissiveCommercial,
    // Commercially permissible with attribution and ShareAlike (CC BY-SA 4.0).
    ShareAlikeCommercial,
    // Strictly non-commercial (e.g. CC BY-NC 4.0) — never bundled into Cranpose.
    NonCommercialOnly,
    // External directory with mixed per-file licenses — verify before bundling.
    VerifyPerFile,
}

impl LicenseKind {
    pub(super) const fn badge(self) -> &'static str {
        match self {
            Self::PermissiveCommercial => "Commercial OK",
            Self::ShareAlikeCommercial => "Commercial + SA",
            Self::NonCommercialOnly => "Non-Commercial Only",
            Self::VerifyPerFile => "Check Per-File License",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum MediaOrigin {
    // Built-in Apache-2.0 / MIT Cranpose synthetic clips (works 100% offline).
    BuiltInOffline,
    // Direct HTTPS MP4 / HLS / MPEG-DASH stream playable on demand.
    DirectStream {
        url: &'static str,
        mime: &'static str,
    },
    // External reference portal (e.g. multi-GB mezzanine S3 or non-commercial test suite).
    ExternalPortal {
        page_url: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct CatalogEntry {
    pub(super) id: &'static str,
    pub(super) title: &'static str,
    pub(super) provider: &'static str,
    pub(super) spec: &'static str,
    pub(super) license: &'static str,
    pub(super) license_kind: LicenseKind,
    pub(super) note: &'static str,
    pub(super) origin: MediaOrigin,
}

pub(super) const CATALOG: &[CatalogEntry] = &[
    CatalogEntry {
        id: "builtin-pack",
        title: "Built-in Multi-Clip Wall (Offline)",
        provider: "Cranpose Synthetic Pack",
        spec: "H.264 / AAC · 3 streams · Zero-copy offline",
        license: "Apache-2.0 OR MIT",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Generated in-repo; starts instantly with zero network latency on every platform.",
        origin: MediaOrigin::BuiltInOffline,
    },
    CatalogEntry {
        id: "blender-tos-hls",
        title: "Tears of Steel (Blender Open Movie)",
        provider: "Blender Foundation",
        spec: "HLS ABR · Live-action + VFX sci-fi",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Attribution: Blender Foundation | mango.blender.org. 4K lossless masters on studio.blender.org.",
        origin: MediaOrigin::DirectStream {
            url: "https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/.m3u8",
            mime: "application/vnd.apple.mpegurl",
        },
    },
    CatalogEntry {
        id: "blender-bbb-hls",
        title: "Big Buck Bunny (HLS ABR)",
        provider: "Blender Foundation",
        spec: "HLS ABR · Multi-bitrate H.264",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Attribution: Blender Foundation | peach.blender.org.",
        origin: MediaOrigin::DirectStream {
            url: "https://test-streams.mux.dev/x36xhzz/x36xhzz.m3u8",
            mime: "application/vnd.apple.mpegurl",
        },
    },
    CatalogEntry {
        id: "blender-bbb-dash",
        title: "Big Buck Bunny (MPEG-DASH)",
        provider: "Blender Foundation / DASH-IF",
        spec: "MPEG-DASH · Adaptive H.264",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Exercises Cranpose's pure-Rust MPEG-DASH manifest + MSE/native streaming pipeline.",
        origin: MediaOrigin::DirectStream {
            url: "https://dash.akamaized.net/akamai/bbb_30fps/bbb_30fps.mpd",
            mime: "application/dash+xml",
        },
    },
    CatalogEntry {
        id: "jellyfin-1080p-avc",
        title: "Jellyfin 1080p H.264 (5 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "1080p SDR · H.264 AVC · 5 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Ideal for 16x 1080p multi-decoder stress. Attribution: Gnattu / Jellyfin.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/SDR/AVC/Test%20Jellyfin%201080p%20AVC%205M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-1080p-av1",
        title: "Jellyfin 1080p AV1 10-bit (5 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "1080p SDR · AV1 10-bit · 5 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Hardware AV1 10-bit decode benchmark on Apple M3+, modern Android, Intel/AMD/NVIDIA, Web.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/SDR/AV1/Test%20Jellyfin%201080p%20AV1%2010bit%205M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-hevc-hdr10",
        title: "Jellyfin 4K HDR10 HEVC (40 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "4K HDR10 PQ · HEVC 10-bit · 40 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "BT.2020 PQ 10-bit zero-copy HDR pipeline and SDR tone-mapping benchmark.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%204K%20HEVC%20HDR10%2040M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-hevc-150m",
        title: "Jellyfin 4K HDR10 HEVC (150 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "4K HDR10 PQ · HEVC 10-bit · 150 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Extreme bitrate decoder bandwidth & demuxer throughput test.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%204K%20HEVC%20HDR10%20150M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-av1-hdr10",
        title: "Jellyfin 4K HDR10 AV1 (40 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "4K HDR10 PQ · AV1 10-bit · 40 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "4K 10-bit HDR AV1 hardware decode stress test.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/HDR/HDR10/AV1/Test%20Jellyfin%204K%20AV1%20HDR10%2040M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-8k-hevc-hdr10",
        title: "Jellyfin 8K HDR10 HEVC (150 Mbps)",
        provider: "Jellyfin Test Videos (Gnattu)",
        spec: "8K HDR10 PQ · HEVC 10-bit · 150 Mbps",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "8K 150 Mbps stress clip for 1x 8K scenario; download locally for repeatable runs.",
        origin: MediaOrigin::DirectStream {
            url: "https://repo.jellyfin.org/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%208K%20HEVC%20HDR10%20150M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "dvb-uhd-dash",
        title: "DVB / DASH-IF UHD MultiRate",
        provider: "DASH-IF / DVB Test Content",
        spec: "UHD MPEG-DASH · Multi-rate HDR/SDR",
        license: "DASH-IF Test (academic 8K sets: CC BY-NC 4.0)",
        license_kind: LicenseKind::VerifyPerFile,
        note: "Live UHD DASH test stream. Academic 8K HDR 50 FPS sets carry CC BY-NC; never bundled.",
        origin: MediaOrigin::DirectStream {
            url: "https://dash.akamaized.net/dash264/TestCasesUHD/2b/11/MultiRate.mpd",
            mime: "application/dash+xml",
        },
    },
    CatalogEntry {
        id: "netflix-sol-levante",
        title: "Sol Levante (Netflix Open Content)",
        provider: "Netflix / Production I.G",
        spec: "4K HDR · Intricate anime lighting & particles",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Open S3 mezzanine package at opencontent.netflix.com; download locally & open via File Picker.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://opencontent.netflix.com/",
        },
    },
    CatalogEntry {
        id: "netflix-nocturne",
        title: "Nocturne (Netflix Open Content)",
        provider: "Netflix Open Content",
        spec: "4K HDR · 60 & 120 FPS motion showcase",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "60/120 FPS HDR reference footage at opencontent.netflix.com; load local encode for 1x 4K 120.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://opencontent.netflix.com/",
        },
    },
    CatalogEntry {
        id: "blender-charge",
        title: "Charge (Blender Open Movie)",
        provider: "Blender Studio",
        spec: "4K Cinematic · Futuristic action & lighting",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Download 4K master from studio.blender.org/films/charge/ and open locally.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://studio.blender.org/films/charge/",
        },
    },
    CatalogEntry {
        id: "xiph-derf",
        title: "Xiph Test Media (Derf's Collection)",
        provider: "Xiph.Org Foundation",
        spec: "Lossless 4K / HDR / 16-bit masters",
        license: "CC BY / BSD (varies per sequence)",
        license_kind: LicenseKind::VerifyPerFile,
        note: "Check per-sequence license at media.xiph.org before redistributing.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://media.xiph.org/video/derf/",
        },
    },
    CatalogEntry {
        id: "uvg-4k-120",
        title: "Ultra Video Group (UVG 4K 120 FPS)",
        provider: "Tampere University",
        spec: "Native 4K 120 FPS throughput dataset",
        license: "CC BY-NC 4.0 (Non-Commercial ONLY)",
        license_kind: LicenseKind::NonCommercialOnly,
        note: "Restricted to non-commercial research/testing; NEVER bundled into Cranpose.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://ultravideo.fi/videosequences.html",
        },
    },
    CatalogEntry {
        id: "kodi-samples",
        title: "Kodi A/V Compatibility Samples",
        provider: "Kodi Wiki",
        spec: "HEVC, AV1, HDR10, Dolby Vision, MKV/MP4",
        license: "Mixed third-party licenses",
        license_kind: LicenseKind::VerifyPerFile,
        note: "External compatibility links; inspect each sample's upstream license individually.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://kodi.wiki/view/Samples",
        },
    },
    CatalogEntry {
        id: "pexels-videos",
        title: "Pexels Real-World Video Gallery",
        provider: "Pexels",
        spec: "4K/HD real-world cinematic footage",
        license: "Pexels License (no raw standalone resale)",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Free commercial use in apps; do not redistribute unaltered files standalone.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://www.pexels.com/videos/",
        },
    },
];

pub(super) fn entry_by_id(id: &str) -> &'static CatalogEntry {
    CATALOG
        .iter()
        .find(|entry| entry.id == id)
        .unwrap_or(&CATALOG[0])
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) enum ActiveSource {
    Catalog(&'static str),
    CustomUrl(String),
    LocalUri { uri: String, label: String },
}

impl ActiveSource {
    pub(super) fn title(&self) -> String {
        match self {
            Self::Catalog(id) => entry_by_id(id).title.to_owned(),
            Self::CustomUrl(url) => format!("Stream: {url}"),
            Self::LocalUri { label, .. } => format!("Local: {label}"),
        }
    }

    pub(super) fn license_summary(&self) -> &'static str {
        match self {
            Self::Catalog(id) => entry_by_id(id).license,
            Self::CustomUrl(_) => "User-supplied stream URL",
            Self::LocalUri { .. } => "Local user file",
        }
    }
}

// Native targets embed the compact 181 KB Apache-2.0/MIT clips so the playground plays immediately
// offline. Web targets fetch the identical files copied beside the bundle by `package-web.sh` so
// zero bytes of video data enter `desktop_app_bg.wasm`.
#[cfg(not(target_arch = "wasm32"))]
const OFFLINE_CLIPS: [&[u8]; 3] = [
    include_bytes!("../../../../player-demo/assets/feed_clip_1.mp4"),
    include_bytes!("../../../../player-demo/assets/feed_clip_2.mp4"),
    include_bytes!("../../../../player-demo/assets/feed_clip_3.mp4"),
];

#[cfg(target_arch = "wasm32")]
const WEB_OFFLINE_URLS: [&str; 3] = [
    "./assets/video/feed_clip_1.mp4",
    "./assets/video/feed_clip_2.mp4",
    "./assets/video/feed_clip_3.mp4",
];

const OFFLINE_TITLES: [&str; 3] = [
    "Ocean Drift (Built-in H.264)",
    "Neon Night (Built-in H.264)",
    "Alpine Run (Built-in H.264)",
];

#[derive(Clone, PartialEq, Eq)]
pub(super) struct BuiltinClipPack {
    #[cfg(not(target_arch = "wasm32"))]
    clips: [Arc<[u8]>; 3],
}

impl BuiltinClipPack {
    pub(super) fn new() -> Self {
        #[cfg(not(target_arch = "wasm32"))]
        {
            Self {
                clips: [
                    Arc::from(OFFLINE_CLIPS[0]),
                    Arc::from(OFFLINE_CLIPS[1]),
                    Arc::from(OFFLINE_CLIPS[2]),
                ],
            }
        }
        #[cfg(target_arch = "wasm32")]
        {
            Self {}
        }
    }

    pub(super) fn item_for_slot(&self, slot: usize) -> MediaItem {
        let idx = slot % 3;
        let title = format!("{} · Tile {}", OFFLINE_TITLES[idx], slot + 1);
        #[cfg(not(target_arch = "wasm32"))]
        {
            MediaItem::from_bytes(Arc::clone(&self.clips[idx]), "video/mp4")
                .with_metadata(MediaMetadata::titled(title))
        }
        #[cfg(target_arch = "wasm32")]
        {
            MediaItem::new(WEB_OFFLINE_URLS[idx])
                .with_mime_type("video/mp4")
                .with_metadata(MediaMetadata::titled(title))
        }
    }
}

pub(super) fn build_media_item(
    source: &ActiveSource,
    pack: &BuiltinClipPack,
    slot: usize,
) -> MediaItem {
    match source {
        ActiveSource::Catalog(id) => match entry_by_id(id).origin {
            MediaOrigin::BuiltInOffline | MediaOrigin::ExternalPortal { .. } => {
                pack.item_for_slot(slot)
            }
            MediaOrigin::DirectStream { url, mime } => MediaItem::new(url)
                .with_mime_type(mime)
                .with_metadata(MediaMetadata::titled(format!(
                    "{} · Tile {}",
                    entry_by_id(id).title,
                    slot + 1
                ))),
        },
        ActiveSource::CustomUrl(url) => MediaItem::new(url.trim())
            .with_metadata(MediaMetadata::titled(format!("Custom · Tile {}", slot + 1))),
        ActiveSource::LocalUri { uri, label } => MediaItem::new(uri.as_str()).with_metadata(
            MediaMetadata::titled(format!("{label} · Tile {}", slot + 1)),
        ),
    }
}
