use super::*;

#[test]
fn a_surviving_density_scope_keeps_its_entry_when_a_sibling_leaves() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{Composition, MemoryApplier, MutableState, location_key};

    let mut composition = Composition::new(MemoryApplier::new());
    let runtime = composition.runtime_handle();
    let with_leading = MutableState::with_runtime(true, runtime.clone());
    let survivor = MutableState::with_runtime(3.0_f32, runtime);
    let seen = Rc::new(Cell::new(0.0_f32));

    #[cranpose_macros::composable]
    fn Reader(seen: Rc<Cell<f32>>) {
        seen.set(density().density());
    }

    fn tree(with_leading: bool, survivor: f32, seen: Rc<Cell<f32>>) {
        if with_leading {
            ProvideDensity(Density::new(2.0, 1.0), || {});
        }
        ProvideDensity(Density::new(survivor, 1.0), move || Reader(seen));
    }

    #[cranpose_macros::composable]
    fn Panel(with_leading: MutableState<bool>, survivor: MutableState<f32>, seen: Rc<Cell<f32>>) {
        tree(with_leading.value(), survivor.value(), seen);
    }

    let key = location_key(file!(), line!(), column!());
    {
        let seen = Rc::clone(&seen);
        composition
            .render(key, move || Panel(with_leading, survivor, Rc::clone(&seen)))
            .expect("initial composition");
    }
    assert_eq!(seen.get(), 3.0);

    with_leading.set_value(false);
    while composition
        .process_invalid_scopes()
        .expect("drop the leading density scope")
    {}
    survivor.set_value(4.0);
    while composition
        .process_invalid_scopes()
        .expect("change the surviving density")
    {}
    assert_eq!(
        seen.get(),
        4.0,
        "the surviving density scope must keep its entry and its reader's \
         subscription when the sibling leaves"
    );
}

#[test]
fn a_provided_density_reaches_the_content_and_ends_with_it() {
    use std::{cell::Cell, rc::Rc};

    use cranpose_core::{Composition, MemoryApplier, location_key};

    let mut composition = Composition::new(MemoryApplier::new());
    let outer = Rc::new(Cell::new(0.0_f32));
    let inside = Rc::new(Cell::new(0.0_f32));
    let after = Rc::new(Cell::new(0.0_f32));

    let key = location_key(file!(), line!(), column!());
    {
        let (outer, inside, after) = (Rc::clone(&outer), Rc::clone(&inside), Rc::clone(&after));
        let mut render = move || {
            outer.set(density().density());
            ProvideDensity(Density::new(3.0, 1.0), || inside.set(density().density()));
            after.set(density().density());
        };
        composition.render(key, &mut render).expect("render");
    }

    assert_eq!(inside.get(), 3.0, "the subtree reads the grid it was given");
    assert_ne!(
        outer.get(),
        3.0,
        "the provision does not escape upwards out of its content"
    );
    assert_eq!(
        after.get(),
        outer.get(),
        "nor does it leak into what follows it"
    );
}

#[test]
fn a_dp_length_lands_on_a_whole_device_pixel() {
    let density = Density::new(2.0, 1.0);
    assert_eq!(density.dp(13.3), 13.5);
    assert_eq!(density.to_px(density.dp(13.3)), 27.0);
    assert_eq!(density.dp(0.25), 0.5);
}

#[test]
fn a_text_size_moves_with_the_font_scale_and_a_dp_length_does_not() {
    let density = Density::new(2.0, 1.24);
    assert_eq!(density.dp(15.0), 15.0);
    assert_eq!(density.to_px(density.sp(15.0)), 37.0);
}

#[test]
fn centring_puts_the_leading_gap_on_a_pixel_boundary() {
    let density = Density::new(2.0, 1.0);
    let leading = density.centre(52.0, 46.5);
    assert_eq!(density.to_px(leading), 6.0);
}

#[test]
fn floor_and_ceil_move_whole_pixels_only() {
    let density = Density::new(2.0, 1.0);
    assert_eq!(density.floor(13.3), 13.0);
    assert_eq!(density.ceil(13.3), 13.5);
    assert_eq!(density.ceil(13.5), 13.5);
}

#[test]
fn a_nonsense_grid_falls_back_to_one_rather_than_dividing_by_zero() {
    let density = Density::new(0.0, f32::NAN);
    assert_eq!(density.density(), 1.0);
    assert_eq!(density.font_scale(), 1.0);
    assert_eq!(density.dp(3.7), 4.0);
}
