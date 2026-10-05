//! Deterministic benchmark data the Rust apps share. `shared-kotlin/.../PerfData.kt`,
//! `flutter-app/lib/data.dart`, `rn-app/src/data.ts` and `maui-app/PerfData.cs`
//! implement the same generator bit for bit, so every app draws identical
//! content.

pub const POST_COUNT: usize = 5000;
pub const BAR_COUNT: usize = 24;

/// Eight saturated colors, used for avatars, chips, particles and tiles.
pub const PALETTE_RGB: [[u8; 3]; 8] = [
    [0xEF, 0x44, 0x44],
    [0xF9, 0x73, 0x16],
    [0xEA, 0xB3, 0x08],
    [0x22, 0xC5, 0x5E],
    [0x14, 0xB8, 0xA6],
    [0x3B, 0x82, 0xF6],
    [0x8B, 0x5C, 0xF6],
    [0xEC, 0x48, 0x99],
];
/// Light chip backgrounds matching [`PALETTE_RGB`].
pub const CHIP_BACKGROUND_RGB: [[u8; 3]; 8] = [
    [0xFE, 0xE2, 0xE2],
    [0xFF, 0xED, 0xD5],
    [0xFE, 0xF9, 0xC3],
    [0xDC, 0xFC, 0xE7],
    [0xCC, 0xFB, 0xF1],
    [0xDB, 0xEA, 0xFE],
    [0xED, 0xE9, 0xFE],
    [0xFC, 0xE7, 0xF3],
];
/// Dark ends of the media gradients.
pub const GRADIENT_END_RGB: [[u8; 3]; 8] = [
    [0x7F, 0x1D, 0x1D],
    [0x7C, 0x2D, 0x12],
    [0x71, 0x3F, 0x12],
    [0x14, 0x53, 0x2D],
    [0x13, 0x4E, 0x4A],
    [0x1E, 0x3A, 0x8A],
    [0x4C, 0x1D, 0x95],
    [0x83, 0x18, 0x43],
];

const WORDS: [&str; 46] = [
    "lorem",
    "ipsum",
    "dolor",
    "sit",
    "amet",
    "consectetur",
    "adipiscing",
    "elit",
    "sed",
    "do",
    "eiusmod",
    "tempor",
    "incididunt",
    "ut",
    "labore",
    "et",
    "dolore",
    "magna",
    "aliqua",
    "enim",
    "ad",
    "minim",
    "veniam",
    "quis",
    "nostrud",
    "exercitation",
    "ullamco",
    "laboris",
    "nisi",
    "aliquip",
    "ex",
    "ea",
    "commodo",
    "consequat",
    "duis",
    "aute",
    "irure",
    "in",
    "reprehenderit",
    "voluptate",
    "velit",
    "esse",
    "cillum",
    "fugiat",
    "nulla",
    "pariatur",
];
const FIRST: [&str; 16] = [
    "Ada", "Linus", "Grace", "Alan", "Barbara", "Dennis", "Ken", "Margaret", "Edsger", "Donald",
    "Frances", "John", "Radia", "Tim", "Guido", "Bjarne",
];
const LAST: [&str; 16] = [
    "Lovelace",
    "Torvalds",
    "Hopper",
    "Turing",
    "Liskov",
    "Ritchie",
    "Thompson",
    "Hamilton",
    "Dijkstra",
    "Knuth",
    "Allen",
    "McCarthy",
    "Perlman",
    "Berners",
    "Rossum",
    "Stroustrup",
];
const TAGS: [&str; 12] = [
    "#rust", "#kotlin", "#compose", "#android", "#gpu", "#layout", "#text", "#perf", "#wgpu",
    "#skia", "#ui", "#mobile",
];

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        let state = seed.wrapping_mul(0x9E37_79B9) ^ 0xA5A5_A5A5;
        Self(if state == 0 { 1 } else { state })
    }

    pub fn next_u32(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn below(&mut self, bound: u32) -> u32 {
        self.next_u32() % bound
    }

    pub fn unit(&mut self) -> f32 {
        (self.next_u32() >> 8) as f32 / 16_777_216.0
    }
}

pub struct Post {
    pub id: u64,
    pub author: String,
    pub handle: String,
    pub initials: String,
    pub color: usize,
    pub title: String,
    pub body: String,
    pub bars: [f32; BAR_COUNT],
    pub tags: [(String, usize); 3],
    pub stats: [String; 4],
}

/// Posts are immutable, so identity decides equality, as `@Immutable` does in
/// Compose.
impl PartialEq for Post {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

fn capitalized(text: String) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_ascii_uppercase().to_string() + chars.as_str(),
        None => text,
    }
}

fn sentence(rng: &mut Rng, min: u32, max: u32) -> String {
    let count = min + rng.below(max - min + 1);
    let words: Vec<&str> = (0..count).map(|_| WORDS[rng.below(46) as usize]).collect();
    capitalized(words.join(" "))
}

/// Formats a count the way social apps do: `734`, `12.4k`.
pub fn compact_count(value: u32) -> String {
    if value >= 1000 {
        format!("{}.{}k", value / 1000, (value % 1000) / 100)
    } else {
        value.to_string()
    }
}

pub fn post(index: usize) -> Post {
    let mut rng = Rng::new(index as u32 + 1);
    let first = FIRST[rng.below(16) as usize];
    let last = LAST[rng.below(16) as usize];
    let minutes = rng.below(59) + 1;
    let handle = format!(
        "@{}{} · {}m",
        first.to_ascii_lowercase(),
        rng.below(1000),
        minutes
    );
    let initials = format!("{}{}", &first[..1], &last[..1]);
    let color = rng.below(8) as usize;
    let title = sentence(&mut rng, 4, 8);
    let body = sentence(&mut rng, 22, 38) + ".";
    let mut bars = [0.0; BAR_COUNT];
    for bar in &mut bars {
        *bar = 0.15 + rng.unit() * 0.85;
    }
    let tags = std::array::from_fn(|_| {
        let tag = rng.below(TAGS.len() as u32) as usize;
        (TAGS[tag].to_string(), tag % 8)
    });
    let stats = std::array::from_fn(|_| compact_count(rng.below(20_000)));
    Post {
        id: index as u64,
        author: format!("{first} {last}"),
        handle,
        initials,
        color,
        title,
        body,
        bars,
        tags,
        stats,
    }
}

#[derive(PartialEq)]
pub struct Comment {
    pub author: String,
    pub text: String,
    pub color: usize,
}

/// The first `count` comments under post `id`.
pub fn comments(id: u64, count: usize) -> Vec<Comment> {
    (0..count)
        .map(|index| {
            let mut rng = Rng::new(100_000 + id as u32 * 16 + index as u32);
            let author = FIRST[rng.below(16) as usize].to_string();
            let text = sentence(&mut rng, 6, 14);
            Comment {
                author,
                text,
                color: index % 8,
            }
        })
        .collect()
}

pub fn posts() -> Vec<Post> {
    (0..POST_COUNT).map(post).collect()
}

#[derive(PartialEq)]
pub struct Quote {
    pub symbol: String,
    pub base: f32,
    pub speed: f32,
    pub phase: f32,
}

#[expect(
    clippy::approx_constant,
    reason = "6.283 mirrors Data.kt exactly, so both apps generate the same phases"
)]
pub fn quotes(count: usize) -> Vec<Quote> {
    let mut rng = Rng::new(7777);
    (0..count)
        .map(|_| {
            let length = 3 + rng.below(2);
            let symbol = (0..length)
                .map(|_| char::from(b'A' + rng.below(26) as u8))
                .collect();
            Quote {
                symbol,
                base: 10.0 + rng.unit() * 990.0,
                speed: 0.5 + rng.unit() * 2.5,
                phase: rng.unit() * 6.283,
            }
        })
        .collect()
}

/// Formats `value` with two decimals and a sign when asked, without the
/// general formatter, as a tuned app would on a per-frame path.
pub fn fixed2(value: f32, signed: bool) -> String {
    let cents = (value.abs() * 100.0).round() as u32;
    let sign = if value < 0.0 {
        "-"
    } else if signed {
        "+"
    } else {
        ""
    };
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub size: f32,
    pub color: usize,
}

/// The first three quarters of `count` particles are circles, the rest rounded
/// squares.
pub fn is_circle(index: usize, count: usize) -> bool {
    index < count * 3 / 4
}

pub fn particles(count: usize) -> Vec<Particle> {
    let mut rng = Rng::new(4242);
    (0..count)
        .map(|index| Particle {
            x: rng.unit(),
            y: rng.unit(),
            vx: (rng.unit() - 0.5) * 0.4,
            vy: (rng.unit() - 0.5) * 0.4,
            size: if is_circle(index, count) {
                3.0 + rng.unit() * 7.0
            } else {
                6.0 + rng.unit() * 14.0
            },
            color: rng.below(8) as usize,
        })
        .collect()
}

/// Wraps `value` into `[0, 1)`.
pub fn wrap_unit(value: f32) -> f32 {
    value - value.floor()
}

// ------------------------------------------------------------------ gauntlet
//
// Everything the gauntlet shows each frame is a function of the frame index,
// in integer arithmetic wherever a value is printed, so both apps show the
// same digits on the same frame.

/// What one load tier puts on screen; `../README.md` lists the tiers.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct GauntletTier {
    /// Cards side by side in each list row.
    pub columns: usize,
    /// Multiplies every size and text size: below 1 fits more on screen.
    pub scale: f32,
    /// Quote tiles in the ticker strip.
    pub tickers: usize,
    /// Nested levels in each deep cluster.
    pub depth: usize,
}

pub const GAUNTLET_TIERS: [GauntletTier; 12] = [
    GauntletTier {
        columns: 1,
        scale: 1.0,
        tickers: 8,
        depth: 6,
    },
    GauntletTier {
        columns: 2,
        scale: 0.85,
        tickers: 12,
        depth: 8,
    },
    GauntletTier {
        columns: 2,
        scale: 0.7,
        tickers: 16,
        depth: 10,
    },
    GauntletTier {
        columns: 3,
        scale: 0.6,
        tickers: 20,
        depth: 12,
    },
    GauntletTier {
        columns: 3,
        scale: 0.5,
        tickers: 28,
        depth: 14,
    },
    GauntletTier {
        columns: 4,
        scale: 0.45,
        tickers: 36,
        depth: 16,
    },
    GauntletTier {
        columns: 4,
        scale: 0.4,
        tickers: 44,
        depth: 20,
    },
    GauntletTier {
        columns: 5,
        scale: 0.35,
        tickers: 56,
        depth: 24,
    },
    GauntletTier {
        columns: 6,
        scale: 0.3,
        tickers: 72,
        depth: 28,
    },
    GauntletTier {
        columns: 7,
        scale: 0.27,
        tickers: 96,
        depth: 32,
    },
    GauntletTier {
        columns: 8,
        scale: 0.25,
        tickers: 120,
        depth: 40,
    },
    GauntletTier {
        columns: 10,
        scale: 0.2,
        tickers: 160,
        depth: 48,
    },
];

/// What `am start` asked of the gauntlet in an app without its own
/// activity code: the launch activity writes `tier freeze` to a file.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Launch {
    /// Load tier, 1 to 12.
    pub tier: usize,
    /// Stop on this frame and hold still, for picture comparisons; 0 runs on.
    pub freeze: u32,
}

impl Launch {
    /// `tier freeze`; the defaults (tier 5, no freeze) for anything missing.
    pub fn parse(text: &str) -> Self {
        let mut fields = text.split_whitespace().map(str::parse::<u32>);
        let tier = fields.next().and_then(Result::ok).unwrap_or(5) as usize;
        let freeze = fields.next().and_then(Result::ok).unwrap_or(0);
        Self { tier, freeze }
    }

    /// The launch the launch activity wrote to `launch.txt` in the app's
    /// `files` folder before the native side started.
    pub fn read(files: Option<std::path::PathBuf>) -> Self {
        let text = files.and_then(|files| std::fs::read_to_string(files.join("launch.txt")).ok());
        Self::parse(text.as_deref().unwrap_or_default())
    }
}

/// Tier `1..=12`; anything else is clamped into that range.
pub fn gauntlet_tier(tier: usize) -> GauntletTier {
    GAUNTLET_TIERS[tier.clamp(1, GAUNTLET_TIERS.len()) - 1]
}

/// Card rows between two deep clusters in the list.
pub const CARD_ROWS_PER_CLUSTER: usize = 5;
/// Avatars the cards cycle through.
pub const AVATAR_COUNT: usize = 8;
/// Side of an avatar bitmap in pixels.
pub const AVATAR_SIZE: u32 = 64;
/// Points in a card's sparkline.
pub const GAUNTLET_SPARK_POINTS: usize = 48;

/// What a list row holds: a row of cards, or every sixth row a deep cluster.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum GauntletRow {
    /// The first card index of the row.
    Cards(usize),
    /// The cluster's index.
    Cluster(usize),
}

pub fn gauntlet_row(row: usize, columns: usize) -> GauntletRow {
    let block = row / (CARD_ROWS_PER_CLUSTER + 1);
    let within = row % (CARD_ROWS_PER_CLUSTER + 1);
    if within == CARD_ROWS_PER_CLUSTER {
        GauntletRow::Cluster(block)
    } else {
        GauntletRow::Cards((block * CARD_ROWS_PER_CLUSTER + within) * columns)
    }
}

#[derive(PartialEq)]
pub struct Ticker {
    pub symbol: String,
    pub base_cents: i32,
    pub swing_cents: i32,
    pub step: i32,
    pub phase: i32,
}

pub fn tickers(count: usize) -> Vec<Ticker> {
    let mut rng = Rng::new(31_337);
    (0..count)
        .map(|_| {
            let length = 3 + rng.below(2);
            let symbol = (0..length)
                .map(|_| char::from(b'A' + rng.below(26) as u8))
                .collect();
            Ticker {
                symbol,
                base_cents: 1_000 + rng.below(99_000) as i32,
                swing_cents: 50 + rng.below(950) as i32,
                step: 1 + rng.below(9) as i32,
                phase: rng.below(2_000) as i32,
            }
        })
        .collect()
}

/// A triangle wave over `period`: 0 at the ends, `period / 2` in the middle.
fn triangle(value: i32, period: i32) -> i32 {
    let position = value.rem_euclid(period);
    (period / 2) - (position - period / 2).abs()
}

/// The quote's price on `frame`, in cents.
pub fn ticker_cents(ticker: &Ticker, frame: u32) -> i32 {
    let wave = triangle(frame as i32 * ticker.step + ticker.phase, 2_000);
    ticker.base_cents + ticker.swing_cents * (wave - 500) / 500
}

/// `1234` → `12.34`.
pub fn cents_text(cents: i32) -> String {
    let sign = if cents < 0 { "-" } else { "" };
    let cents = cents.abs();
    format!("{sign}{}.{:02}", cents / 100, cents % 100)
}

/// The change from the base price as a signed percent: `+1.25%`.
pub fn change_text(ticker: &Ticker, cents: i32) -> String {
    let basis_points = (cents - ticker.base_cents) * 10_000 / ticker.base_cents;
    let sign = if basis_points < 0 { "-" } else { "+" };
    let basis_points = basis_points.abs();
    format!("{sign}{}.{:02}%", basis_points / 100, basis_points % 100)
}

/// A card's progress on `frame`, in thousandths.
pub fn progress_permille(card: usize, frame: u32) -> u32 {
    (frame * 3 + card as u32 * 37) % 1_000
}

/// The tilt of a card's badge on `frame`, in degrees: -5 to 5.
pub fn badge_degrees(card: usize, frame: u32) -> f32 {
    triangle((frame * 2 + card as u32 * 30) as i32, 40) as f32 * 0.5 - 5.0
}

/// The content's share of the screen width on `frame`: 0.92 to 1.
pub fn width_fraction(frame: u32) -> f32 {
    0.92 + 0.08 * triangle((frame * 3) as i32, 200) as f32 / 100.0
}

/// The sparkline's height at `point`, as a fraction of the chart, on `frame`.
pub fn spark_value(card: usize, point: usize, frame: u32) -> f32 {
    let phase = (frame as f32 + card as f32 * 7.0) * 0.11;
    0.5 + 0.38 * (point as f32 * 0.32 + phase).sin()
        + 0.08 * (point as f32 * 1.7 + card as f32).sin()
}

/// The two ends of each avatar's gradient, as RGB bytes.
const AVATAR_COLORS: [[u8; 6]; AVATAR_COUNT] = [
    [0xEF, 0x44, 0x44, 0x7F, 0x1D, 0x1D],
    [0xF9, 0x73, 0x16, 0x7C, 0x2D, 0x12],
    [0xEA, 0xB3, 0x08, 0x71, 0x3F, 0x12],
    [0x22, 0xC5, 0x5E, 0x14, 0x53, 0x2D],
    [0x14, 0xB8, 0xA6, 0x13, 0x4E, 0x4A],
    [0x3B, 0x82, 0xF6, 0x1E, 0x3A, 0x8A],
    [0x8B, 0x5C, 0xF6, 0x4C, 0x1D, 0x95],
    [0xEC, 0x48, 0x99, 0x83, 0x18, 0x43],
];

/// Avatar `index` as tightly packed RGBA: a radial gradient crossed by
/// diagonal stripes, so a misplaced or mis-scaled image is visible.
pub fn avatar_rgba(index: usize) -> Vec<u8> {
    let colors = AVATAR_COLORS[index % AVATAR_COUNT];
    let size = AVATAR_SIZE as i32;
    let mut pixels = Vec::with_capacity((AVATAR_SIZE * AVATAR_SIZE * 4) as usize);
    for y in 0..size {
        for x in 0..size {
            let dx = x - size / 2;
            let dy = y - size * 3 / 8;
            let t = ((dx * dx + dy * dy) * 255 / (40 * 40)).min(255);
            let stripe = ((x + y + index as i32 * 3) / 6) % 2 == 0;
            for channel in 0..3 {
                let near = i32::from(colors[channel]);
                let far = i32::from(colors[channel + 3]);
                let mut value = (near * (255 - t) + far * t) / 255;
                if stripe {
                    value += (255 - value) / 6;
                }
                pixels.push(value as u8);
            }
            pixels.push(0xFF);
        }
    }
    pixels
}
