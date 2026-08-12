use cranpose_ui::Color;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PaletteChoice {
    Default,
    Deuteranopia,
    Tritanopia,
    Monochrome,
}

impl PaletteChoice {
    pub const ALL: [PaletteChoice; 4] = [
        PaletteChoice::Default,
        PaletteChoice::Deuteranopia,
        PaletteChoice::Tritanopia,
        PaletteChoice::Monochrome,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PaletteChoice::Default => "DEFAULT",
            PaletteChoice::Deuteranopia => "DEUTER",
            PaletteChoice::Tritanopia => "TRITAN",
            PaletteChoice::Monochrome => "MONO",
        }
    }

    pub fn from_ordinal(value: usize) -> PaletteChoice {
        Self::ALL[value.min(Self::ALL.len() - 1)]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GamePalette {
    pub background: Color,
    pub vignette: Color,
    pub rail: Color,
    pub rail_accent: Color,
    pub paddle: Color,
    pub paddle_edge: Color,
    pub paddle_shield: Color,
    pub ball_core: Color,
    pub ball_glow: Color,
    pub standard: Color,
    pub standard_edge: Color,
    pub armored: Color,
    pub armor_seam: Color,
    pub explosive: Color,
    pub phase: Color,
    pub relay: Color,
    pub danger: Color,
    pub success: Color,
    pub hud: Color,
    pub hud_dim: Color,
    pub field: Color,
    pub portal: Color,
}

const DEFAULT_PALETTE: GamePalette = GamePalette {
    // Wear OS asks a full-screen app for a pure black base so the OLED panel
    // can switch those pixels off; the old 0x03050A read as a lit near-black
    // at the display edge. The vignette above it is unchanged, so only the
    // rim goes black. Deuteranopia and tritanopia inherit this field.
    background: Color::from_rgb_u8(0x00, 0x00, 0x00),
    vignette: Color::from_rgb_u8(0x0A, 0x12, 0x20),
    rail: Color::from_rgb_u8(0x16, 0x32, 0x4C),
    rail_accent: Color::from_rgb_u8(0x2A, 0x6E, 0x96),
    paddle: Color::from_rgb_u8(0xB9, 0xF2, 0xFF),
    paddle_edge: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    paddle_shield: Color::from_rgb_u8(0x7D, 0xF9, 0xC8),
    ball_core: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    ball_glow: Color::from_rgb_u8(0x56, 0xE0, 0xFF),
    standard: Color::from_rgb_u8(0x2F, 0xA8, 0xF5),
    standard_edge: Color::from_rgb_u8(0x9B, 0xE0, 0xFF),
    armored: Color::from_rgb_u8(0x9B, 0x6B, 0xFF),
    armor_seam: Color::from_rgb_u8(0xE4, 0xD4, 0xFF),
    explosive: Color::from_rgb_u8(0xFF, 0x9A, 0x2E),
    phase: Color::from_rgb_u8(0xFF, 0x5F, 0xD2),
    relay: Color::from_rgb_u8(0x5C, 0xF2, 0xC8),
    danger: Color::from_rgb_u8(0xFF, 0x4B, 0x32),
    success: Color::from_rgb_u8(0xB9, 0xFF, 0xCB),
    hud: Color::from_rgb_u8(0xDF, 0xF6, 0xFF),
    hud_dim: Color::from_rgb_u8(0x5E, 0x7E, 0x93),
    field: Color::from_rgb_u8(0x2C, 0x4C, 0x8A),
    portal: Color::from_rgb_u8(0x66, 0xD9, 0xFF),
};

const DEUTERANOPIA_PALETTE: GamePalette = GamePalette {
    standard: Color::from_rgb_u8(0x3F, 0x9C, 0xFF),
    standard_edge: Color::from_rgb_u8(0xB9, 0xDC, 0xFF),
    armored: Color::from_rgb_u8(0x9A, 0x7B, 0xFF),
    armor_seam: Color::from_rgb_u8(0xE7, 0xDB, 0xFF),
    explosive: Color::from_rgb_u8(0xFF, 0xC2, 0x4B),
    phase: Color::from_rgb_u8(0xFF, 0x7B, 0xE0),
    relay: Color::from_rgb_u8(0x7F, 0xE8, 0xFF),
    danger: Color::from_rgb_u8(0xFF, 0x7A, 0x45),
    success: Color::from_rgb_u8(0xCF, 0xE9, 0xFF),
    ball_glow: Color::from_rgb_u8(0x7E, 0xD4, 0xFF),
    field: Color::from_rgb_u8(0x3A, 0x5C, 0x9E),
    portal: Color::from_rgb_u8(0x8A, 0xD7, 0xFF),
    ..DEFAULT_PALETTE
};

const TRITANOPIA_PALETTE: GamePalette = GamePalette {
    standard: Color::from_rgb_u8(0xFF, 0x6B, 0x8A),
    standard_edge: Color::from_rgb_u8(0xFF, 0xC6, 0xD2),
    armored: Color::from_rgb_u8(0xB0, 0x7B, 0xFF),
    armor_seam: Color::from_rgb_u8(0xEB, 0xDB, 0xFF),
    explosive: Color::from_rgb_u8(0xFF, 0x9C, 0x6B),
    phase: Color::from_rgb_u8(0x7B, 0xE8, 0xFF),
    relay: Color::from_rgb_u8(0x8C, 0xFF, 0xB4),
    danger: Color::from_rgb_u8(0xFF, 0x3B, 0x5C),
    success: Color::from_rgb_u8(0xD7, 0xFF, 0xE4),
    ball_core: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    ball_glow: Color::from_rgb_u8(0xFF, 0xA8, 0xC0),
    rail: Color::from_rgb_u8(0x4A, 0x24, 0x36),
    rail_accent: Color::from_rgb_u8(0x9C, 0x4A, 0x66),
    field: Color::from_rgb_u8(0x8A, 0x3A, 0x56),
    portal: Color::from_rgb_u8(0x9C, 0xE8, 0xFF),
    ..DEFAULT_PALETTE
};

const MONOCHROME_PALETTE: GamePalette = GamePalette {
    background: Color::from_rgb_u8(0x00, 0x00, 0x00),
    vignette: Color::from_rgb_u8(0x0E, 0x0E, 0x0E),
    rail: Color::from_rgb_u8(0x2A, 0x2A, 0x2A),
    rail_accent: Color::from_rgb_u8(0x5A, 0x5A, 0x5A),
    paddle: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    paddle_edge: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    paddle_shield: Color::from_rgb_u8(0xDD, 0xDD, 0xDD),
    ball_core: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    ball_glow: Color::from_rgb_u8(0xBB, 0xBB, 0xBB),
    standard: Color::from_rgb_u8(0x8E, 0x8E, 0x8E),
    standard_edge: Color::from_rgb_u8(0xE4, 0xE4, 0xE4),
    armored: Color::from_rgb_u8(0x5E, 0x5E, 0x5E),
    armor_seam: Color::from_rgb_u8(0xF2, 0xF2, 0xF2),
    explosive: Color::from_rgb_u8(0xCF, 0xCF, 0xCF),
    phase: Color::from_rgb_u8(0xA6, 0xA6, 0xA6),
    relay: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    danger: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    success: Color::from_rgb_u8(0xFF, 0xFF, 0xFF),
    hud: Color::from_rgb_u8(0xF0, 0xF0, 0xF0),
    hud_dim: Color::from_rgb_u8(0x7A, 0x7A, 0x7A),
    field: Color::from_rgb_u8(0x3A, 0x3A, 0x3A),
    portal: Color::from_rgb_u8(0xD0, 0xD0, 0xD0),
};

pub fn palette_for(choice: PaletteChoice, high_contrast: bool) -> GamePalette {
    let base = match choice {
        PaletteChoice::Default => DEFAULT_PALETTE,
        PaletteChoice::Deuteranopia => DEUTERANOPIA_PALETTE,
        PaletteChoice::Tritanopia => TRITANOPIA_PALETTE,
        PaletteChoice::Monochrome => MONOCHROME_PALETTE,
    };
    if !high_contrast {
        return base;
    }
    GamePalette {
        background: Color::BLACK,
        vignette: Color::BLACK,
        rail: brighten(base.rail, 0.35),
        rail_accent: brighten(base.rail_accent, 0.35),
        standard: brighten(base.standard, 0.28),
        standard_edge: Color::WHITE,
        armored: brighten(base.armored, 0.28),
        armor_seam: Color::WHITE,
        explosive: brighten(base.explosive, 0.24),
        phase: brighten(base.phase, 0.24),
        relay: brighten(base.relay, 0.24),
        hud: Color::WHITE,
        hud_dim: brighten(base.hud_dim, 0.4),
        field: brighten(base.field, 0.45),
        portal: brighten(base.portal, 0.3),
        danger: brighten(base.danger, 0.3),
        success: brighten(base.success, 0.3),
        paddle: brighten(base.paddle, 0.25),
        paddle_edge: brighten(base.paddle_edge, 0.25),
        ball_core: Color::WHITE,
        paddle_shield: brighten(base.paddle_shield, 0.3),
        ..base
    }
}

pub fn brighten(color: Color, amount: f32) -> Color {
    Color(
        (color.0 + (1.0 - color.0) * amount).clamp(0.0, 1.0),
        (color.1 + (1.0 - color.1) * amount).clamp(0.0, 1.0),
        (color.2 + (1.0 - color.2) * amount).clamp(0.0, 1.0),
        color.3,
    )
}

pub fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let f = t.clamp(0.0, 1.0);
    Color(
        from.0 + (to.0 - from.0) * f,
        from.1 + (to.1 - from.1) * f,
        from.2 + (to.2 - from.2) * f,
        from.3 + (to.3 - from.3) * f,
    )
}

pub fn pack(color: Color) -> u32 {
    let to_byte = |value: f32| (value.clamp(0.0, 1.0) * 255.0 + 0.5) as u32;
    (to_byte(color.3) << 24) | (to_byte(color.0) << 16) | (to_byte(color.1) << 8) | to_byte(color.2)
}

pub fn unpack(value: u32) -> Color {
    Color::from_rgba_u8(
        ((value >> 16) & 0xFF) as u8,
        ((value >> 8) & 0xFF) as u8,
        (value & 0xFF) as u8,
        ((value >> 24) & 0xFF) as u8,
    )
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorScheme {
    pub primary: Color,
    pub primary_container: Color,
    pub on_primary: Color,
    pub on_primary_container: Color,
    pub surface_container: Color,
    pub on_surface: Color,
    pub on_surface_variant: Color,
    pub outline: Color,
    pub background: Color,
    pub on_background: Color,
}

impl ColorScheme {
    pub fn of(palette: &GamePalette) -> ColorScheme {
        ColorScheme {
            primary: palette.paddle,
            primary_container: mix(palette.standard, palette.background, 0.68),
            on_primary: palette.background,
            on_primary_container: palette.hud,
            surface_container: mix(palette.rail, palette.background, 0.55),
            on_surface: palette.hud,
            on_surface_variant: palette.hud_dim,
            outline: mix(palette.rail_accent, palette.background, 0.3),
            background: palette.background,
            on_background: palette.hud,
        }
    }
}

/// The same colour at a given CIE L\*.
///
/// Wear Material 3 builds its scroll indicator out of one colour taken to two
/// lightnesses — `setLuminance(onBackground, 80f)` for the thumb and
/// `setLuminance(onBackground, 20f)` for the track — so the port needs the
/// same operation rather than an alpha over the background, which lands
/// somewhere else entirely. Hue and chroma are carried through untouched;
/// only L\* moves.
pub fn with_luminance(color: Color, l_star: f32) -> Color {
    let (_, a, b) = to_lab(color);
    let (r, g, bl) = from_lab(l_star, a, b);
    Color(r, g, bl, color.3)
}

const WHITE_X: f32 = 0.950_489;
const WHITE_Z: f32 = 1.088_840;

fn to_linear(channel: f32) -> f32 {
    if channel <= 0.040_45 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn to_srgb(channel: f32) -> f32 {
    let value = if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    };
    value.clamp(0.0, 1.0)
}

fn lab_f(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    if t > DELTA * DELTA * DELTA {
        t.cbrt()
    } else {
        t / (3.0 * DELTA * DELTA) + 4.0 / 29.0
    }
}

fn lab_f_inverse(t: f32) -> f32 {
    const DELTA: f32 = 6.0 / 29.0;
    if t > DELTA {
        t * t * t
    } else {
        3.0 * DELTA * DELTA * (t - 4.0 / 29.0)
    }
}

fn to_lab(color: Color) -> (f32, f32, f32) {
    let r = to_linear(color.0);
    let g = to_linear(color.1);
    let b = to_linear(color.2);
    let x = (0.412_456 * r + 0.357_576 * g + 0.180_437 * b) / WHITE_X;
    let y = 0.212_673 * r + 0.715_152 * g + 0.072_175 * b;
    let z = (0.019_334 * r + 0.119_192 * g + 0.950_304 * b) / WHITE_Z;
    let (fx, fy, fz) = (lab_f(x), lab_f(y), lab_f(z));
    (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))
}

fn from_lab(l: f32, a: f32, b: f32) -> (f32, f32, f32) {
    let fy = (l + 16.0) / 116.0;
    let fx = fy + a / 500.0;
    let fz = fy - b / 200.0;
    let x = lab_f_inverse(fx) * WHITE_X;
    let y = lab_f_inverse(fy);
    let z = lab_f_inverse(fz) * WHITE_Z;
    let r = 3.240_454 * x - 1.537_138 * y - 0.498_531 * z;
    let g = -0.969_266 * x + 1.876_011 * y + 0.041_556 * z;
    let bl = 0.055_643 * x - 0.204_026 * y + 1.057_225 * z;
    (to_srgb(r), to_srgb(g), to_srgb(bl))
}

/// A colour ready to be drawn at `alpha` the way Compose's graphics layer
/// delivers it.
///
/// Every `ScalingLazyColumn` row carries a `graphicsLayer { alpha }`, and a
/// layer below full opacity is rendered into an **8-bit** offscreen buffer and
/// then composited. So the row's colours are quantised to whole channel values
/// *before* the alpha multiplies them, and folding the alpha into a float
/// colour instead lands up to one level away on every pixel of every faded
/// row. `surface_container` is `mix(rail, background, 0.55)` = (9.9, 22.5,
/// 34.2); at alpha 0.9 Compose draws (9, 21, 31) where the unquantised product
/// rounds to (9, 20, 31). That single level covered thirty per cent of the
/// scrolled settings screen.
///
/// Quantising at full opacity is a no-op — `round(round(c))` is `round(c)` —
/// so this is safe to use for every row rather than only the faded ones.
///
/// **This models a layer; it is not one.** Where a row's own content overlaps
/// — a label over its capsule, a switch thumb over its track — Compose
/// composites the two opaquely inside the layer and fades the result, while
/// drawing each primitive at `alpha` fades them separately and leaves the
/// capsule showing through the glyph edges. That difference is small (it needs
/// partial glyph coverage to appear at all) and it needs a real offscreen
/// layer to fix.
pub fn faded_by_layer(color: Color, alpha: f32) -> Color {
    Color(
        quantise(color.0),
        quantise(color.1),
        quantise(color.2),
        color.3 * alpha,
    )
}

/// One channel snapped to the 8-bit value a layer buffer would hold.
fn quantise(channel: f32) -> f32 {
    (channel.clamp(0.0, 1.0) * 255.0).round() / 255.0
}

pub fn mix(color: Color, towards: Color, amount: f32) -> Color {
    let t = amount.clamp(0.0, 1.0);
    Color(
        color.0 + (towards.0 - color.0) * t,
        color.1 + (towards.1 - color.1) * t,
        color.2 + (towards.2 - color.2) * t,
        1.0,
    )
}

/// The palette the Credits screen ships with.
pub fn default_palette() -> GamePalette {
    DEFAULT_PALETTE
}
