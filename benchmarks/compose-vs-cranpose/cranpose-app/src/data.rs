//! Deterministic benchmark data. `compose-app/.../Data.kt` implements the same
//! generator bit for bit, so both apps draw identical content.

use cranpose::Color;

pub const POST_COUNT: usize = 5000;
pub const BAR_COUNT: usize = 24;

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

/// Eight saturated colors, used for avatars, chips, particles and tiles.
pub const PALETTE: [Color; 8] = [
    Color::from_rgb_u8(0xEF, 0x44, 0x44),
    Color::from_rgb_u8(0xF9, 0x73, 0x16),
    Color::from_rgb_u8(0xEA, 0xB3, 0x08),
    Color::from_rgb_u8(0x22, 0xC5, 0x5E),
    Color::from_rgb_u8(0x14, 0xB8, 0xA6),
    Color::from_rgb_u8(0x3B, 0x82, 0xF6),
    Color::from_rgb_u8(0x8B, 0x5C, 0xF6),
    Color::from_rgb_u8(0xEC, 0x48, 0x99),
];
/// Light chip backgrounds matching [`PALETTE`].
pub const CHIP_BACKGROUND: [Color; 8] = [
    Color::from_rgb_u8(0xFE, 0xE2, 0xE2),
    Color::from_rgb_u8(0xFF, 0xED, 0xD5),
    Color::from_rgb_u8(0xFE, 0xF9, 0xC3),
    Color::from_rgb_u8(0xDC, 0xFC, 0xE7),
    Color::from_rgb_u8(0xCC, 0xFB, 0xF1),
    Color::from_rgb_u8(0xDB, 0xEA, 0xFE),
    Color::from_rgb_u8(0xED, 0xE9, 0xFE),
    Color::from_rgb_u8(0xFC, 0xE7, 0xF3),
];
/// Dark ends of the media gradients.
pub const GRADIENT_END: [Color; 8] = [
    Color::from_rgb_u8(0x7F, 0x1D, 0x1D),
    Color::from_rgb_u8(0x7C, 0x2D, 0x12),
    Color::from_rgb_u8(0x71, 0x3F, 0x12),
    Color::from_rgb_u8(0x14, 0x53, 0x2D),
    Color::from_rgb_u8(0x13, 0x4E, 0x4A),
    Color::from_rgb_u8(0x1E, 0x3A, 0x8A),
    Color::from_rgb_u8(0x4C, 0x1D, 0x95),
    Color::from_rgb_u8(0x83, 0x18, 0x43),
];

pub struct Rng(u32);

impl Rng {
    pub fn new(seed: u32) -> Self {
        let state = seed.wrapping_mul(0x9E37_79B9) ^ 0xA5A5_A5A5;
        Self(if state == 0 { 1 } else { state })
    }

    pub fn next(&mut self) -> u32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        x
    }

    pub fn below(&mut self, bound: u32) -> u32 {
        self.next() % bound
    }

    pub fn unit(&mut self) -> f32 {
        (self.next() >> 8) as f32 / 16_777_216.0
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
