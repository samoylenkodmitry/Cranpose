use super::{
    GlyphKind,
    GlyphKind::{Plain, Turned},
    TargetRect, movable_glyph_kind,
};

// A glyph draw: turned or plain, `None` for a retained run, and its bounds.
type Draw = (Option<GlyphKind>, TargetRect);

fn movable(draws: &[Draw]) -> Option<GlyphKind> {
    movable_glyph_kind(draws, |draw| draw.0, |draw| draw.1)
}

fn cell(kind: GlyphKind, column: u32) -> Draw {
    (Some(kind), (column * 20, 0, 16, 16))
}

#[test]
fn the_fewer_kind_moves_when_the_kinds_alternate_over_their_own_pixels() {
    let draws = [
        cell(Turned, 0),
        cell(Plain, 1),
        cell(Turned, 2),
        cell(Turned, 3),
        cell(Plain, 4),
        cell(Turned, 5),
    ];
    assert_eq!(movable(&draws), Some(Plain));
    let turned_fewer = draws.map(|(kind, bounds)| (kind.map(GlyphKind::other), bounds));
    assert_eq!(movable(&turned_fewer), Some(Turned));
}

#[test]
fn grouped_or_single_kind_draws_stay_as_they_are() {
    assert_eq!(
        movable(&[cell(Turned, 0), cell(Turned, 1), cell(Plain, 2)]),
        None
    );
    assert_eq!(movable(&[cell(Plain, 0), cell(Plain, 1)]), None);
    assert_eq!(movable(&[]), None);
}

#[test]
fn a_retained_run_keeps_every_draw_in_its_place() {
    let draws = [
        cell(Turned, 0),
        cell(Plain, 1),
        (None, (40, 0, 16, 16)),
        cell(Turned, 3),
        cell(Plain, 4),
    ];
    assert_eq!(movable(&draws), None);
}

#[test]
fn a_draw_never_moves_past_one_of_the_other_kind_it_shares_pixels_with() {
    // The plain draw at 1 covers the turned one after it, so the plain draws
    // cannot move after the turned ones; the turned ones move instead.
    let overlapped = [
        cell(Turned, 0),
        (Some(Plain), (20, 0, 30, 16)),
        cell(Turned, 2),
        cell(Plain, 4),
    ];
    assert_eq!(movable(&overlapped), Some(Turned));
    // Here each kind covers a later draw of the other: neither moves.
    let tangled = [
        (Some(Turned), (0, 0, 30, 16)),
        (Some(Plain), (20, 0, 30, 16)),
        (Some(Turned), (40, 0, 30, 16)),
        cell(Plain, 6),
    ];
    assert_eq!(movable(&tangled), None);
}
