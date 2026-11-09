//! The style sheet both screens share. A length written `8s` is eight points
//! at the tier's scale; every other length is written as the pixels it is.

use perf_data::*;

/// The sRGB color `value` as CSS.
pub(crate) fn hex(value: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", value[0], value[1], value[2])
}

/// The sRGB color `value` at straight alpha `alpha` as CSS.
pub(crate) fn rgba(value: [u8; 3], alpha: f32) -> String {
    format!("rgba({}, {}, {}, {alpha})", value[0], value[1], value[2])
}

/// The sheet's text with every `<number>s` turned into that many pixels at
/// `scale`.
fn scaled(css: &str, scale: f32) -> String {
    let mut out = String::with_capacity(css.len() * 2);
    let mut number = String::new();
    for ch in css.chars() {
        if ch.is_ascii_digit() || (ch == '.' && !number.is_empty()) {
            number.push(ch);
            continue;
        }
        if ch == 's' && !number.is_empty() {
            let value: f32 = number.parse().expect("a length");
            out.push_str(&format!("{}px", value * scale));
        } else {
            out.push_str(&number);
            out.push(ch);
        }
        number.clear();
    }
    out.push_str(&number);
    out
}

/// One class per palette entry: a fill, a chip, a text color and a gradient.
fn palette_classes() -> String {
    let mut css = String::new();
    for (index, rgb) in PALETTE_RGB.iter().enumerate() {
        css.push_str(&format!(
            ".p{index} {{ background-color: {}; }}\n",
            hex(*rgb)
        ));
    }
    for index in 0..GRADIENT_END_RGB.len() {
        css.push_str(&format!(
            ".chip{index} {{ background-color: {}; color: {}; }}\n.g{index} {{ color: {}; }}\n",
            hex(CHIP_BACKGROUND_RGB[index]),
            hex(GRADIENT_END_RGB[index]),
            hex(GRADIENT_END_RGB[index]),
        ));
    }
    for (index, rgb) in PALETTE_RGB.iter().enumerate() {
        css.push_str(&format!(
            ".s{index} {{ background-image: linear-gradient(180deg, {}, {}); }}\n",
            rgba(*rgb, 0.25),
            rgba(*rgb, 0.0),
        ));
    }
    css
}

/// The sheet of the gauntlet at `scale`.
pub(crate) fn gauntlet(scale: f32) -> String {
    scaled(
        &format!("{BASE}{}{GAUNTLET}{LAYERS}", palette_classes()),
        scale,
    )
}

const BASE: &str = "
:root {
    background-color: #EEF0F5;
    color: #111827;
    font-family: Roboto;
    font-size: 12px;
    line-height: 1.4;
}
.page { height: 100%; }
.top {
    height: 56px;
    flex-shrink: 0;
    padding: 0 16px;
    background-color: #1E2A4A;
    justify-content: center;
}
.top-title { font-size: 20px; font-weight: 700; color: #FFFFFF; }
.bold { font-weight: 700; }
";

const GAUNTLET: &str = "
.content { flex: 1 1 0; min-height: 0; }
.tickers {
    flex-shrink: 0;
    flex-direction: row;
    flex-wrap: wrap;
    gap: 4s;
    padding: 6s;
    background-color: #E2E8F0;
}
.tick {
    flex-direction: row;
    gap: 4s;
    align-items: center;
    padding: 3s 6s;
    border-radius: 6s;
    background-color: #FFFFFF;
}
.tick-symbol { font-size: 10s; font-weight: 700; color: #111827; }
.tick-text { font-size: 10s; color: #374151; }
.tick-change { font-size: 10s; }
.track {
    flex-shrink: 0;
    background-color: #E5E7EB;
    overflow: hidden;
}
.track-tick { width: 20s; height: 4s; border-radius: 2s; }
.track-card { flex: 1 1 0; height: 6s; border-radius: 3s; }
.fill-tick { height: 4s; border-radius: 2s; }
.fill-card { height: 6s; border-radius: 3s; }
.area { position: relative; flex: 1 1 0; min-height: 0; overflow: hidden; }
.list {
    position: absolute;
    left: 0;
    right: 0;
    top: 0;
    padding: 0 8s;
    gap: 8s;
}
.cards { flex-direction: row; gap: 8s; }
.cell { flex: 1 1 0; min-width: 0; position: relative; }
.card {
    gap: 6s;
    padding: 10s;
    border-radius: 12s;
    border: 1px solid #E5E7EB;
    background-color: #FFFFFF;
    box-shadow: 0 1s 3s rgba(0, 0, 0, 0.24);
}
.head { flex-direction: row; gap: 8s; align-items: center; }
.avatar { width: 32s; height: 32s; border-radius: 16s; flex-shrink: 0; object-fit: cover; }
.head-text { flex: 1 1 0; min-width: 0; }
.title {
    font-size: 13s;
    font-weight: 700;
    color: #111827;
    max-height: 36.9s;
    overflow: hidden;
}
.subtitle { font-size: 11s; color: #6B7280; }
.body { font-size: 12s; color: #374151; max-height: 67.7s; overflow: hidden; }
.chips { flex-direction: row; flex-wrap: wrap; gap: 4s; }
.chip { padding: 3s 8s; border-radius: 10s; font-size: 10s; }
.progress { flex-direction: row; gap: 6s; align-items: center; }
.percent { font-size: 10s; color: #6B7280; }
.spark { display: block; width: 100%; height: 36s; }
.stats { flex-direction: row; }
.stat { flex: 1 1 0; min-width: 0; align-items: center; }
.stat-value { font-size: 12s; font-weight: 700; color: #111827; }
.stat-label { font-size: 9s; color: #6B7280; }
.divider { width: 1px; background-color: #E5E7EB; }
.badge {
    position: absolute;
    top: 6s;
    right: 6s;
    padding: 2s 6s;
    border-radius: 8s;
    background-color: rgba(239, 68, 68, 0.9);
}
.badge-text { font-size: 9s; font-weight: 700; color: rgba(255, 255, 255, 0.9); }
.level { gap: 1s; padding: 1s 1s 1s 3s; border-radius: 4s; }
.lv0 { background-color: #F1F5F9; }
.lv1 { background-color: #CBD5E1; }
.level-row { flex-direction: row; gap: 2s; }
.level-chip { flex: 1 1 0; min-width: 0; border-radius: 2s; font-size: 9s; color: #FFFFFF; }
";

/// The rules of the stacked panels, whose lengths are points and never scale.
const LAYERS: &str = "
.layer {
    position: absolute;
    left: 0;
    top: 0;
    width: 220px;
    gap: 8px;
    padding: 10px;
    border-radius: 12px;
    background-color: rgba(30, 41, 59, 0.753);
}
.layer-title { font-size: 13px; font-weight: 700; color: #FFFFFF; }
.layer-cells { gap: 3px; }
.layer-line { flex-direction: row; gap: 3px; }
.layer-cell {
    display: flex;
    width: 22px;
    height: 22px;
    flex-shrink: 0;
    border-radius: 4px;
    align-items: center;
    justify-content: center;
    font-size: 9px;
    color: #FFFFFF;
}
.nested {
    gap: 2px;
    padding: 8px;
    border-radius: 8px;
    background-color: rgba(255, 255, 255, 0.902);
}
.nested-title { font-size: 11px; font-weight: 700; color: #111827; }
.nested-text { font-size: 11px; color: #374151; }
";

/// The sheet of the still wall.
pub(crate) fn wall() -> String {
    let tier = wall_tier(perf_data::launch().tier);
    let (label, value) = wall_text_size(tier.tile_height);
    let dot = wall_dot_size(tier.tile_height);
    format!(
        "{BASE}
.page {{ background-color: {background}; }}
.clock {{ height: {WALL_CLOCK_HEIGHT}px; flex-shrink: 0; padding: 0 8px; justify-content: center; }}
.clock-text {{ font-size: {WALL_CLOCK_TEXT}px; color: {ink}; }}
.board {{ position: relative; flex: 1 1 0; min-height: 0; }}
.tiles {{ padding: {WALL_GAP}px; gap: {WALL_GAP}px; }}
.line {{ flex-direction: row; gap: {WALL_GAP}px; }}
.tile {{
    position: relative;
    flex-shrink: 0;
    width: {tile_width}px;
    height: {tile_height}px;
    border-radius: {WALL_CORNER}px;
}}
.tile-label {{ position: absolute; font-size: {label}px; font-weight: 700; color: {ink}; }}
.tile-dot {{ position: absolute; width: {dot}px; height: {dot}px; border-radius: {half}px; }}
.tile-value {{ position: absolute; font-size: {value}px; color: {ink}; }}
.cursor {{
    position: absolute;
    left: 0;
    top: 0;
    width: {tile_width}px;
    height: {tile_height}px;
    border: {WALL_CURSOR_WIDTH}px solid {ink};
    border-radius: {WALL_CORNER}px;
}}
",
        ink = hex(WALL_INK_RGB),
        background = hex(WALL_BACKGROUND_RGB),
        tile_width = tier.tile_width,
        tile_height = tier.tile_height,
        half = dot / 2.0,
    )
}
