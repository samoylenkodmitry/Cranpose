use super::{TargetRect, movable_glyph_kind};

/// A glyph draw: turned or plain, `None` for a retained run, and its bounds.
type Draw = (Option<bool>, TargetRect);

fn movable(draws: &[Draw]) -> Option<bool> {
    movable_glyph_kind(draws, |draw| draw.0, |draw| draw.1)
}

fn cell(kind: bool, column: u32) -> Draw {
    (Some(kind), (column * 20, 0, 16, 16))
}

#[test]
fn the_fewer_kind_moves_when_the_kinds_alternate_over_their_own_pixels() {
    let draws = [
        cell(true, 0),
        cell(false, 1),
        cell(true, 2),
        cell(true, 3),
        cell(false, 4),
        cell(true, 5),
    ];
    assert_eq!(movable(&draws), Some(false));
    let turned_fewer = draws.map(|(kind, bounds)| (kind.map(|turned| !turned), bounds));
    assert_eq!(movable(&turned_fewer), Some(true));
}

#[test]
fn grouped_or_single_kind_draws_stay_as_they_are() {
    assert_eq!(
        movable(&[cell(true, 0), cell(true, 1), cell(false, 2)]),
        None
    );
    assert_eq!(movable(&[cell(false, 0), cell(false, 1)]), None);
    assert_eq!(movable(&[]), None);
}

#[test]
fn a_retained_run_keeps_every_draw_in_its_place() {
    let draws = [
        cell(true, 0),
        cell(false, 1),
        (None, (40, 0, 16, 16)),
        cell(true, 3),
        cell(false, 4),
    ];
    assert_eq!(movable(&draws), None);
}

#[test]
fn a_draw_never_moves_past_one_of_the_other_kind_it_shares_pixels_with() {
    // The plain draw at 1 covers the turned one after it, so the plain draws
    // cannot move after the turned ones; the turned ones move instead.
    let overlapped = [
        cell(true, 0),
        (Some(false), (20, 0, 30, 16)),
        cell(true, 2),
        cell(false, 4),
    ];
    assert_eq!(movable(&overlapped), Some(true));
    // Here each kind covers a later draw of the other: neither moves.
    let tangled = [
        (Some(true), (0, 0, 30, 16)),
        (Some(false), (20, 0, 30, 16)),
        (Some(true), (40, 0, 30, 16)),
        cell(false, 6),
    ];
    assert_eq!(movable(&tangled), None);
}
