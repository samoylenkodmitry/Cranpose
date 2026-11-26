// Stress-test presets, verified CORS-enabled open-media catalog with per-source licensing
// metadata, TikTok-style vertical reel playlist, and instant-play built-in offline clips.

#[cfg(not(target_arch = "wasm32"))]
use std::sync::Arc;

use cranpose::prelude::{MediaItem, MediaMetadata};

pub(super) const DEFAULT_SOURCE_ID: &str = "blender-bbb-1080p60";

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
            Self::CinematicHero => "1x Showcase · 498M px/s",
            Self::Single4k120 => "1x 4K 120 · 995M px/s",
            Self::Quad4k60 => "4x 4K 60 · 1.99B px/s",
            Self::Wall16x1080p60 => "16x 1080p 60 · 1.99B px/s",
            Self::Single8k60 => "1x 8K 60 · 1.99B px/s",
            Self::Wall64x720p60 => "64x 720p 60 · 3.54B px/s",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum WallLayout {
    Grid,
    Reels,
    MosaicWave,
    Carousel3d,
    Spotlight,
}

impl WallLayout {
    pub(super) const ALL: [Self; 5] = [
        Self::Grid,
        Self::Reels,
        Self::MosaicWave,
        Self::Carousel3d,
        Self::Spotlight,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Grid => "Grid",
            Self::Reels => "Reels",
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
    // Commercially permissible with attribution, CC0, Public Domain, or permissive terms.
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
    // Direct CORS-verified HTTPS MP4 / WebM / fMP4 HLS / MPEG-DASH stream playable in-app.
    DirectStream {
        url: &'static str,
        mime: &'static str,
    },
    // External reference portal (raw cinema image sequences, API-gated, or non-commercial).
    ExternalPortal {
        page_url: &'static str,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CatalogCategory {
    All,
    Blender,
    Nature,
    Jellyfin,
    NetflixXiph,
    OfflinePortals,
}

impl CatalogCategory {
    pub(super) const ALL: [Self; 6] = [
        Self::All,
        Self::Blender,
        Self::Nature,
        Self::Jellyfin,
        Self::NetflixXiph,
        Self::OfflinePortals,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Blender => "Blender",
            Self::Nature => "8K/4K Nature",
            Self::Jellyfin => "Jellyfin",
            Self::NetflixXiph => "Netflix / Xiph",
            Self::OfflinePortals => "Offline & Links",
        }
    }

    pub(super) fn matches(self, entry: &CatalogEntry) -> bool {
        match self {
            Self::All => true,
            Self::Blender => entry.id.starts_with("blender-"),
            Self::Nature => entry.id.starts_with("nature-"),
            Self::Jellyfin => entry.id.starts_with("jellyfin-") || entry.id == "dvb-uhd-dash",
            Self::NetflixXiph => entry.id.starts_with("netflix-") || entry.id.starts_with("xiph-"),
            Self::OfflinePortals => {
                matches!(
                    entry.origin,
                    MediaOrigin::BuiltInOffline | MediaOrigin::ExternalPortal { .. }
                )
            }
        }
    }
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
        id: "blender-bbb-1080p60",
        title: "Big Buck Bunny (1080p 60 FPS)",
        provider: "Blender Foundation",
        spec: "1080p 60 FPS · H.264 MP4 · Direct CORS",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Default showcase stream. Attribution: Blender Foundation | peach.blender.org.",
        origin: MediaOrigin::DirectStream {
            url: "https://distribution.bbb3d.renderfarming.net/video/mp4/bbb_sunflower_1080p_60fps_normal.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "blender-bbb-4k60",
        title: "Big Buck Bunny (4K 60 FPS)",
        provider: "Blender Foundation",
        spec: "4K 2160p 60 FPS · H.264 MP4 · Direct CORS",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Full UHD 60 FPS Sunflower master for 1x/4x 4K GPU decode tests.",
        origin: MediaOrigin::DirectStream {
            url: "https://distribution.bbb3d.renderfarming.net/video/mp4/bbb_sunflower_2160p_60fps_normal.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "blender-bbb-dash",
        title: "Big Buck Bunny (MPEG-DASH ABR)",
        provider: "Blender Foundation / DASH-IF",
        spec: "MPEG-DASH up to 4K · Adaptive H.264 + AAC",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Pure-Rust fixed-duration SegmentTemplate DASH manifest + MSE/native driver.",
        origin: MediaOrigin::DirectStream {
            url: "https://dash.akamaized.net/akamai/bbb_30fps/bbb_30fps.mpd",
            mime: "application/dash+xml",
        },
    },
    CatalogEntry {
        id: "blender-bbb-hls",
        title: "Big Buck Bunny (fMP4 HLS ABR)",
        provider: "Blender Foundation / Shaka",
        spec: "fMP4 HLS ABR · H.264 + separate AAC audio",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Fragmented-MP4 HLS playable across Chrome, Firefox, Safari, desktop, and mobile.",
        origin: MediaOrigin::DirectStream {
            url: "https://storage.googleapis.com/shaka-demo-assets/bbb-dark-truths-hls/hls.m3u8",
            mime: "application/vnd.apple.mpegurl",
        },
    },
    CatalogEntry {
        id: "blender-charge-4k",
        title: "Charge 4K (Blender Open Movie)",
        provider: "Blender Studio",
        spec: "4K 3840x1608 · VP9 WebM · Futuristic action",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Direct 4K stream hosted on Wikimedia Commons with CORS. Attribution: Blender Studio.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/7/7a/Charge_-_Blender_Open_Movie-full_movie.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "blender-tos-dash",
        title: "Tears of Steel (MPEG-DASH ABR)",
        provider: "Blender Foundation",
        spec: "MPEG-DASH ABR · Live-action + VFX sci-fi",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Adaptive fMP4 DASH stream playable in every browser and native platform.",
        origin: MediaOrigin::DirectStream {
            url: "https://demo.unified-streaming.com/k8s/features/stable/video/tears-of-steel/tears-of-steel.ism/.mpd",
            mime: "application/dash+xml",
        },
    },
    CatalogEntry {
        id: "blender-tos-4k",
        title: "Tears of Steel 4K (Blender Open Movie)",
        provider: "Blender Foundation",
        spec: "4K 3840x1714 · VP9 WebM · Direct CORS",
        license: "CC BY 3.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Attribution: Blender Foundation | mango.blender.org.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/1/10/Tears_of_Steel_in_4k_-_Official_Blender_Foundation_release.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "blender-sprite-fright",
        title: "Sprite Fright (Blender Open Movie)",
        provider: "Blender Studio",
        spec: "2K 2048x858 · VP9 WebM · Stylized animation",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Direct stream hosted on Wikimedia Commons. Attribution: Blender Studio.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/7/76/Sprite_Fright_-_Blender_Open_Movie-full_movie.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "netflix-sol-levante",
        title: "Sol Levante 4K (Netflix Open Content)",
        provider: "Netflix / Production I.G",
        spec: "4K 3840x2160 · VP9 WebM · Anime particles & lighting",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Direct 4K WebM stream (Netflix S3 hosts raw 16 GB ProRes without web CORS).",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/4/44/Sol_Levante.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "xiph-netflix-aerial-4k60",
        title: "Xiph / Netflix Aerial (4K 60 FPS 10-bit)",
        provider: "Xiph.Org / Netflix Open Content",
        spec: "4K 4096x2160 60 FPS · 10-bit WebM · Fast start",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Direct CORS-enabled 4K 60 FPS 10-bit sequence from media.xiph.org (Derf's Collection).",
        origin: MediaOrigin::DirectStream {
            url: "https://media.xiph.org/video/derf/webm/Netflix_Aerial_4096x2160_60fps_10bit_420.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "xiph-netflix-coaster-4k60",
        title: "Xiph / Netflix Roller Coaster (4K 60 FPS 10-bit)",
        provider: "Xiph.Org / Netflix Open Content",
        spec: "4K 4096x2160 60 FPS · 10-bit WebM · High motion",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Direct CORS-enabled 4K 60 FPS high-motion stress clip from media.xiph.org.",
        origin: MediaOrigin::DirectStream {
            url: "https://media.xiph.org/video/derf/webm/Netflix_RollerCoaster_4096x2160_60fps_10bit_420.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-potomac-8k",
        title: "Potomac River Rapids — 8K Nature",
        provider: "Wikimedia Commons",
        spec: "8K 7680x4320 · Real-world flowing water · WebM",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Native 8K UHD real-world nature footage with fine water detail and CORS.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/c/cd/Rough_water_on_the_Potomac_River_Washington_DC_2024-01-14_11-25-03_1.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-lake-sunset-8k",
        title: "Lake Untreusee Sunset — 8K Nature Timelapse",
        provider: "Wikimedia Commons",
        spec: "8K 8192x5461 · Lake & golden hour clouds · WebM",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Ultra-high-resolution 8K nature sunset timelapse with direct CORS streaming.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/8/8b/Sonnenuntergang_am_Untreusee_20250918_LQ.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-earth-iss-8k",
        title: "Earth from the ISS — NASA 8K Ultra HD",
        provider: "NASA Johnson Space Center",
        spec: "8K 8192x4096 · Orbital Earth horizons · WebM",
        license: "Public Domain (NASA)",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Shot aboard the International Space Station in 8K Ultra HD; public domain.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/c/c3/View_of_the_Earth_from_the_ISS_%288K%29.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-ocean-sst-8k60",
        title: "Global Ocean Currents — NASA 8K 60 FPS",
        provider: "NASA Scientific Visualization Studio",
        spec: "8K 8192x4096 60 FPS · Global sea temperature · WebM",
        license: "Public Domain (NASA)",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "23 years of satellite ocean surface dynamics in native 8K 60 FPS.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/b/b1/23_Years_of_Sea_Surface_Temperature_%28SST%29_-_2002-2025_%28SVS5600_-_8k_2x1_4096p60_h265%29.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-alaska-aurora-4k",
        title: "Alaskan Aurora Timelapse — NASA 4K+ Nature",
        provider: "NASA Scientific Visualization Studio",
        spec: "4K+ 4240x2832 · Fast-start night sky · WebM",
        license: "Public Domain (NASA)",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Compact 12 MB 4K+ aurora borealis timelapse for instant playback.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/c/c5/Alaskan_Aurora_Time_Lapse_%28SVS15077%29.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-iceland-aurora-4k",
        title: "Northern Lights over Myvatn, Iceland — 4K Nature",
        provider: "Giles Laurent / Wikimedia Commons",
        spec: "4K 3840x2160 · Night sky aurora · WebM",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Real-world 4K Icelandic aurora footage. Attribution: Giles Laurent.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/f/f4/002_Northern_lights_in_the_night_sky_over_M%C3%BDvatn_in_Iceland_Video_by_Giles_Laurent.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-waterfall-peru-portrait",
        title: "Huascaran Waterfall, Peru — 9:16 Nature Reel",
        provider: "Wikimedia Commons",
        spec: "1080x1920 Portrait · Mountain waterfall · WebM",
        license: "CC0 1.0 Public Domain",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Vertical 9:16 mountain waterfall footage ideal for the Reels vertical gallery.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/f/f7/Waterfall_in_Huascar%C3%A1n_National_Park_-_Peru.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "nature-waterfall-nz-portrait",
        title: "Mt Nimrod Waterfall, New Zealand — 9:16 Nature Reel",
        provider: "Wikimedia Commons",
        spec: "1080x1920 Portrait · Forest stream · WebM",
        license: "CC0 1.0 Public Domain",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Vertical 9:16 scenic reserve waterfall clip for TikTok-style vertical scrolling.",
        origin: MediaOrigin::DirectStream {
            url: "https://upload.wikimedia.org/wikipedia/commons/9/9f/Mt_Nimrod_Scenic_Reserve_-_Waterfall_on_Nimrod_Stream_%28video%29.webm",
            mime: "video/webm",
        },
    },
    CatalogEntry {
        id: "jellyfin-1080p-avc",
        title: "Jellyfin 1080p H.264 (5 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "1080p SDR · H.264 AVC · 5 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Ideal for 16x 1080p multi-decoder stress. Attribution: Gnattu / Jellyfin.",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/SDR/AVC/Test%20Jellyfin%201080p%20AVC%205M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-1080p-av1",
        title: "Jellyfin 1080p AV1 10-bit (5 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "1080p SDR · AV1 10-bit · 5 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Hardware AV1 10-bit decode benchmark on Apple M3+, modern Android, desktop, Web.",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/SDR/AV1/Test%20Jellyfin%201080p%20AV1%2010bit%205M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-hevc-hdr10",
        title: "Jellyfin 4K HDR10 HEVC (40 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "4K HDR10 PQ · HEVC 10-bit · 40 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "BT.2020 PQ 10-bit zero-copy HDR pipeline and SDR tone-mapping benchmark.",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%204K%20HEVC%20HDR10%2040M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-hevc-150m",
        title: "Jellyfin 4K HDR10 HEVC (150 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "4K HDR10 PQ · HEVC 10-bit · 150 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Extreme bitrate decoder bandwidth and demuxer throughput test.",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%204K%20HEVC%20HDR10%20150M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-4k-av1-hdr10",
        title: "Jellyfin 4K HDR10 AV1 (40 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "4K HDR10 PQ · AV1 10-bit · 40 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "4K 10-bit HDR AV1 hardware decode stress test.",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/HDR/HDR10/AV1/Test%20Jellyfin%204K%20AV1%20HDR10%2040M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-8k-av1-hdr10",
        title: "Jellyfin 8K HDR10 AV1 (100 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "8K HDR10 PQ · AV1 10-bit · 100 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "Browser-compatible 8K HDR10 AV1 decode benchmark (Chrome, Firefox, Apple M3+).",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/HDR/HDR10/AV1/Test%20Jellyfin%208K%20AV1%20HDR10%20100M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "jellyfin-8k-hevc-hdr10",
        title: "Jellyfin 8K HDR10 HEVC (150 Mbps)",
        provider: "Jellyfin Test Videos (ams1 mirror)",
        spec: "8K HDR10 PQ · HEVC 10-bit · 150 Mbps · CORS",
        license: "CC BY-SA 4.0",
        license_kind: LicenseKind::ShareAlikeCommercial,
        note: "8K 150 Mbps HEVC stress clip (requires hardware HEVC decode).",
        origin: MediaOrigin::DirectStream {
            url: "https://ams1.mirror.jellyfin.org/files/test-videos/HDR/HDR10/HEVC/Test%20Jellyfin%208K%20HEVC%20HDR10%20150M.mp4",
            mime: "video/mp4",
        },
    },
    CatalogEntry {
        id: "dvb-uhd-dash",
        title: "DVB / DASH-IF UHD MultiRate (4K 60 FPS)",
        provider: "DASH-IF / DVB Test Content",
        spec: "4K 60 FPS MPEG-DASH · HEVC 10-bit MultiRate",
        license: "DASH-IF Test (academic 8K sets: CC BY-NC 4.0)",
        license_kind: LicenseKind::VerifyPerFile,
        note: "Fixed-duration DASH UHD HEVC stream (requires hardware HEVC decode in browser).",
        origin: MediaOrigin::DirectStream {
            url: "https://dash.akamaized.net/dash264/TestCasesUHD/2b/11/MultiRate.mpd",
            mime: "application/dash+xml",
        },
    },
    CatalogEntry {
        id: "builtin-pack",
        title: "Built-in Multi-Clip Wall (Offline)",
        provider: "Cranpose Synthetic Pack",
        spec: "H.264 / AAC · 3 streams · Zero-copy offline",
        license: "Apache-2.0 OR MIT",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Generated in-repo for zero-network 64-tile stress testing on every platform.",
        origin: MediaOrigin::BuiltInOffline,
    },
    CatalogEntry {
        id: "netflix-nocturne",
        title: "Nocturne 120 FPS (Netflix Open Content)",
        provider: "Netflix Open Content",
        spec: "4K HDR 120 FPS · Raw EXR image sequence (not MP4)",
        license: "CC BY 4.0",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Upstream distributes raw .exr frames only; encode locally & open via File Picker.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://opencontent.netflix.com/",
        },
    },
    CatalogEntry {
        id: "uvg-4k-120",
        title: "Ultra Video Group (UVG 4K 120 FPS)",
        provider: "Tampere University",
        spec: "Native 4K 120 FPS raw .yuv throughput dataset",
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
        spec: "HEVC, AV1, HDR10, Dolby Vision MKV/MP4 index",
        license: "Mixed third-party licenses",
        license_kind: LicenseKind::VerifyPerFile,
        note: "External compatibility wiki with per-file third-party licenses.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://kodi.wiki/view/Samples",
        },
    },
    CatalogEntry {
        id: "pexels-videos",
        title: "Pexels Real-World Video Gallery",
        provider: "Pexels",
        spec: "4K/HD real-world footage (API-gated downloads)",
        license: "Pexels License (no raw standalone resale)",
        license_kind: LicenseKind::PermissiveCommercial,
        note: "Dynamic API-gated URLs; download clips and open via File Picker.",
        origin: MediaOrigin::ExternalPortal {
            page_url: "https://www.pexels.com/videos/",
        },
    },
];

// Curated vertical TikTok-style Reels playlist mixing instant-start H.264, 9:16 portrait nature,
// 4K/8K nature & space, Blender open movies, Netflix Open Content, and Jellyfin test clips.
pub(super) const REEL_FEED_IDS: &[&str] = &[
    "blender-bbb-1080p60",
    "nature-waterfall-peru-portrait",
    "nature-alaska-aurora-4k",
    "xiph-netflix-aerial-4k60",
    "nature-waterfall-nz-portrait",
    "blender-charge-4k",
    "nature-iceland-aurora-4k",
    "netflix-sol-levante",
    "blender-tos-dash",
    "xiph-netflix-coaster-4k60",
    "jellyfin-1080p-avc",
    "jellyfin-4k-av1-hdr10",
    "nature-potomac-8k",
    "nature-earth-iss-8k",
    "nature-lake-sunset-8k",
    "blender-bbb-dash",
    "builtin-pack",
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

// Native targets embed the compact 181 KB Apache-2.0/MIT clips for offline 64-tile testing.
// Web targets fetch the identical files copied beside the bundle by `package-web.sh` so zero
// bytes of video data enter `desktop_app_bg.wasm`.
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
