use super::*;

fn fade_starts_at() -> f32 {
    FLAME_REACH - EDGE_FADE
}

fn widest_band() -> f32 {
    STYLES
        .iter()
        .map(|style| style.band_width)
        .fold(0.0f32, f32::max)
}

#[test]
fn the_window_is_the_panel_plus_the_flames_reach_on_every_side() {
    assert_eq!(WINDOW_WIDTH, CONTENT_WIDTH + 2.0 * FLAME_REACH);
    assert_eq!(WINDOW_HEIGHT, CONTENT_HEIGHT + 2.0 * FLAME_REACH);
}

#[test]
fn the_fire_fades_out_before_the_window_ends_and_has_room_to_burn_first() {
    assert!(
        fade_starts_at() > 0.0,
        "the fade has to start outside the panel, or it eats the flame itself"
    );
    assert!(
        fade_starts_at() > widest_band() * 3.0,
        "the fire burns about three band widths out; it has to be past its brightest \
         before the fade begins, or the window clips a hard edge into it"
    );
}

#[test]
fn the_halo_eases_out_well_inside_the_window_so_nothing_reads_as_a_line() {
    assert!(
        HALO_FALLOFF <= fade_starts_at(),
        "the halo has to reach nothing before the window's own fade starts, \
         or the two fades compound into the very edge the gradient is there to remove"
    );
    assert!(
        HALO_FALLOFF > widest_band() * 3.0,
        "the gradient has to span the fire's whole glow, not cut into its bright band"
    );
}

#[test]
fn a_click_walks_the_flames_and_comes_back_around() {
    let labels: Vec<&str> = (0..STYLES.len() as u32 + 1)
        .map(|clicks| style_after_clicks(clicks).label)
        .collect();
    assert_eq!(labels[0], FIRE_CLASSIC.label);
    assert_eq!(labels[STYLES.len()], labels[0], "the cycle closes");
    let distinct: std::collections::BTreeSet<&str> =
        labels[..STYLES.len()].iter().copied().collect();
    assert_eq!(distinct.len(), STYLES.len(), "every flame is its own");
}

#[test]
fn hovering_fans_the_flame_and_pressing_is_a_full_blaze() {
    assert_eq!(blaze(2.0, 1.5, 3.0, false, false), 2.0);
    assert_eq!(blaze(2.0, 1.5, 3.0, true, false), 3.0);
    assert_eq!(
        blaze(2.0, 1.5, 3.0, true, true),
        6.0,
        "a press wins over the hover it arrives with"
    );
}
