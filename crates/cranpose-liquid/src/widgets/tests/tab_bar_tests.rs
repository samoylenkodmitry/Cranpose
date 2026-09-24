use super::*;
use crate::widgets::tab_motion::tab_lens_activity_motion;

#[test]
fn tab_bar_spec_normalizes_the_maximum_cell_width() {
    assert_eq!(LiquidTabBarSpec::default().max_tab_width, TAB_WIDTH);
    assert_eq!(LiquidTabBarSpec::new(85.0).max_tab_width, 85.0);
    assert_eq!(LiquidTabBarSpec::new(0.0).max_tab_width, 1.0);
    assert_eq!(LiquidTabBarSpec::new(f32::NAN).max_tab_width, TAB_WIDTH);
}

#[test]
fn drag_pointer_centers_the_lens_and_preserves_end_overdrag() {
    let geometry = TabGeometry {
        width: 400.0,
        cell_width: 100.0,
        pitch: 100.0,
    };
    assert_eq!(geometry.drag_left(50.0, 4, true), 0.0);
    assert_eq!(geometry.drag_left(250.0, 4, true), 200.0);
    assert_eq!(geometry.drag_left(-100.0, 4, true), -20.0);
    assert_eq!(geometry.drag_left(500.0, 4, true), 355.0);

    assert_eq!(geometry.drag_left(-100.0, 4, false), 0.0);
    assert_eq!(geometry.drag_left(500.0, 4, false), 300.0);
}

#[test]
fn resting_lens_centers_on_its_cell_and_stays_inside_the_pill() {
    let tab = 78.0;
    assert_eq!(tab_lens_resting_left(0, tab, 5), 0.0);
    assert_eq!(tab_lens_resting_left(1, tab, 5), tab);
    assert_eq!(tab_lens_resting_left(3, tab, 5), 3.0 * tab);
    assert_eq!(tab_lens_resting_left(4, tab, 5), 4.0 * tab);
    assert_eq!(tab_lens_resting_left(9, tab, 5), 4.0 * tab);
}

#[test]
fn flight_deformation_keeps_native_optics_in_the_unscaled_capsule() {
    let geometry = TabFlightGeometry {
        center: (80.0, 54.0),
        base_size: Size::new(111.0, 70.0),
        strain: Size::new(14.0 / 111.0, -14.0 / 70.0),
        lens_position: 0.0,
        lens_activity: 1.0,
        resting_tint: Color::TRANSPARENT,
        accessory_center: None,
    };
    let dynamics = tab_flight_dynamics(
        geometry,
        TabFlightNode {
            origin: (0.0, 0.0),
            size: Size::new(160.0, 108.0),
        },
    );
    let shape = dynamics.morph.unwrap().primary;
    assert_eq!((shape.2, shape.3, shape.4), (111.0, 70.0, 35.0));
}

#[test]
fn released_bubble_keeps_its_elastic_rebound() {
    let base = Size::new(104.5, 54.0);
    let strain = Size::new(0.017416525, -0.024551005);
    let actual = tab_lens_deformed_size(base, strain);
    assert!((actual.width - 106.32003).abs() < 0.0001);
    assert!((actual.height - 52.67425).abs() < 0.0001);
}

#[test]
fn flight_lens_node_is_centered_on_the_bar_axis() {
    for node_height in [48.0, 64.0, 96.0, 128.0] {
        let center = tab_lens_node_top(node_height) + node_height * 0.5;
        assert!((center - BAR_HEIGHT * 0.5).abs() < f32::EPSILON);
    }
}

#[test]
fn liquid_tab_builds_reference_content() {
    assert_eq!(TAB_ICON_SIZE, 32.0);
    let tab = LiquidTab::new(crate::icons::STAR, "Discover");
    assert_eq!(tab.icon, LiquidTabIcon::Vector(crate::icons::STAR));
    assert_eq!(tab.label, "Discover");
    assert_eq!(tab.icon_style, LiquidTabIconStyle::Plain);

    let badge = LiquidTab::app_badge(crate::icons::APPLE, "WWDC");
    assert_eq!(badge.icon_style, LiquidTabIconStyle::AppBadge);

    let compact = LiquidTab::new(crate::icons::ACCOUNT_CIRCLE, "Account").with_icon_scale(0.72);
    assert!((compact.icon_scale - 0.72).abs() < f32::EPSILON);
    assert_eq!(tab.clone().with_icon_scale(f32::NAN).icon_scale, 1.0);
    assert_eq!(tab.with_icon_scale(2.0).icon_scale, 1.5);
}

#[test]
fn artwork_offsets_preserve_the_tab_and_reject_nonfinite_coordinates() {
    let tab = LiquidTab::new(crate::icons::STAR, "Discover");
    let shifted = tab.clone().with_icon_offset(-1.0 / 3.0, 1.0);
    assert_eq!(shifted.icon_offset, (-1.0 / 3.0, 1.0));
    assert_eq!(shifted.icon, tab.icon);
    assert_eq!(shifted.label, tab.label);
    assert_eq!(
        tab.clone().with_icon_offset(f32::NAN, 1.0).icon_offset,
        (0.0, 1.0)
    );
    assert_eq!(
        tab.with_icon_offset(2.0, f32::INFINITY).icon_offset,
        (2.0, 0.0)
    );
}

#[test]
fn template_tab_keeps_artwork_size_and_label() {
    let bitmap = cranpose_ui_graphics::ImageBitmap::from_rgba8(1, 1, vec![0, 0, 0, 255])
        .expect("template pixel");
    let painter = Painter::from_bitmap(bitmap);
    let size = Size::new(24.0, 28.0);
    let tab = LiquidTab::from_painter(painter.clone(), size, "Saved");
    assert_eq!(tab.icon, LiquidTabIcon::Painter { painter, size });
    assert_eq!(tab.label, "Saved");
    assert_eq!(tab.icon_style, LiquidTabIconStyle::Plain);
}

#[test]
fn app_badge_geometry_honors_the_tab_optical_scale() {
    let full = app_badge_geometry(1.0);
    let corrected = app_badge_geometry(0.85);
    assert_eq!(full.size, Size::new(20.0, 32.0));
    assert_eq!(corrected.size, Size::new(17.0, 27.2));
    assert!((corrected.glyph.width - 11.9).abs() < 1.0e-5);
    assert!((corrected.corner_radius / full.corner_radius - 0.85).abs() < f32::EPSILON);
    assert!((corrected.stripe.x / full.stripe.x - 0.85).abs() < f32::EPSILON);
    assert!((corrected.glyph.width / full.glyph.width - 0.85).abs() < f32::EPSILON);
}

#[test]
fn base_tab_content_remains_neutral_under_the_moving_selection_layer() {
    let colors =
        crate::theme::LiquidColors::light(cranpose_ui_graphics::Color::from_rgb_u8(0, 122, 255));
    assert_eq!(tab_base_content_color(colors), colors.label);
    assert_eq!(tab_selection_content_color(colors), colors.accent);
}

#[test]
fn tab_deformation_preserves_independent_native_axis_motion() {
    let base = Size::new(120.5, 70.0);
    let strain = Size::new(0.1820888, -0.2760726);
    let deformed = tab_lens_deformed_size(base, strain);
    assert!((deformed.width - 142.4417).abs() < 0.001);
    assert!((deformed.height - 50.67492).abs() < 0.001);
    assert_eq!(tab_lens_deformed_size(base, Size::new(0.0, 0.0)), base);
}

#[test]
fn selection_mask_and_lens_resolve_the_same_global_sdf() {
    let geometry = TabFlightGeometry {
        center: (212.0, 32.0),
        base_size: Size::new(106.0, 64.0),
        strain: Size::new(0.0, 0.0),
        lens_position: 160.0,
        lens_activity: 1.0,
        resting_tint: Color::BLACK.with_alpha(0.10),
        accessory_center: None,
    };
    let mask_node = TabFlightNode {
        origin: (0.0, 0.0),
        size: Size::new(328.0, 64.0),
    };
    let lens_node = TabFlightNode {
        origin: (132.0, -22.0),
        size: Size::new(160.0, 108.0),
    };
    let mask = tab_flight_dynamics(geometry, mask_node)
        .morph
        .expect("selection mask morph");
    let lens = tab_flight_dynamics(geometry, lens_node)
        .morph
        .expect("lens morph");
    assert_eq!(
        (
            mask.primary.0 + mask_node.origin.0,
            mask.primary.1 + mask_node.origin.1
        ),
        geometry.center
    );
    assert_eq!(
        (
            lens.primary.0 + lens_node.origin.0,
            lens.primary.1 + lens_node.origin.1
        ),
        geometry.center
    );
    assert_eq!(
        (mask.primary.2, mask.primary.3, mask.primary.4),
        (lens.primary.2, lens.primary.3, lens.primary.4)
    );
    assert_eq!(mask.node_size, (328.0, 64.0));
    assert_eq!(lens.node_size, (160.0, 108.0));
}

#[test]
fn unified_bar_has_no_detached_accessory_gap() {
    assert_eq!(tab_bar_accessory_gap(false), 0.0);
    assert_eq!(tab_bar_accessory_gap(true), 10.0);
}

#[test]
fn flight_lens_only_joins_accessory_after_surface_contact() {
    assert!(!accessory_surfaces_touch(0.01));
    assert!(accessory_surfaces_touch(0.0));
    assert!(accessory_surfaces_touch(-4.0));
}

#[test]
fn lens_contact_swell_matches_the_raised_reference() {
    for (cell, raised_width) in [(95.0, 111.0), (104.5, 120.5)] {
        assert_eq!(tab_lens_base_size(cell, 0.0), (cell, 54.0));
        let raised = tab_lens_base_size(cell, 1.0);
        assert!((raised.0 - raised_width).abs() < 0.001);
        assert!((raised.1 - 70.0).abs() < 0.001);
    }
}

#[test]
fn held_bar_grows_about_its_center_and_strains_with_travel() {
    let rest = tab_bar_transform(360.0, 0.0, 1.0);
    assert_eq!(rest.scale_x, 1.0);
    assert_eq!(rest.scale_y, 1.0);
    assert_eq!(rest.translation_x, 0.0);
    for travel in [-1.0, 0.0, 1.0] {
        let layer = tab_bar_transform(360.0, 1.0, travel);
        assert_eq!(layer.translation_y, 0.0);
        assert!((360.0 * layer.scale_x - (374.14 + 2.63 * travel.abs())).abs() < 1e-3);
        assert!((62.0 * layer.scale_y - (64.4352 - 0.45294 * travel.abs())).abs() < 1e-3);
        assert!((layer.translation_x - travel * 2.756).abs() < 1e-3);
    }
    for width in [360.0, 398.0] {
        let layer = tab_bar_transform(width, 1.0, 0.0);
        assert!((width * (layer.scale_x - 1.0) - 14.14).abs() < 0.001);
    }
}

#[test]
fn tab_grid_matches_the_reference_pitch() {
    assert_eq!(TAB_WIDTH, 78.0);
}

#[test]
fn tab_grid_matches_the_reference_inner_inset() {
    assert_eq!(BLOB_MARGIN, 4.0);
    assert_eq!(BLOB_HEIGHT + 2.0 * BLOB_MARGIN, BAR_HEIGHT);
}

#[test]
fn flight_lens_uses_the_measured_sequential_warps() {
    let glass = tab_flight_lens_material(cranpose_ui_graphics::Color::BLACK, 1.0);
    let generic_lens = Glass::lens();
    assert!(glass.lift.is_some_and(|lift| (0.0..=0.15).contains(&lift)));
    assert_eq!(glass.refraction_depth_dp, Some(36.0));
    assert_eq!(
        glass.refraction,
        crate::material::GlassRefraction::EdgeLens { reach_dp: 9.0 }
    );
    assert!(glass.refraction_curve < generic_lens.refraction_curve);
    assert!(glass.dispersion * 0.3 < generic_lens.dispersion);
    assert_eq!(glass.blur_radius, Some(0.0));
    assert_eq!(glass.backdrop_blur, Some((4.0, 0.0)));
    assert_eq!(glass.highlight, 1.0);
    assert_eq!(glass.key_fill.map(|light| light.curvature), Some(0.8));
    assert!(
        glass.shadow,
        "the moving lens needs its target-visible SDF contact outline"
    );
    assert!(glass.tint.is_some_and(|tint| tint.a() == 0.0));
    assert_eq!(glass.face_response.unwrap().gain, 0.97);
    assert_eq!(glass.meniscus_absorption, 0.0);
    assert_eq!(glass.adaptive_frost, 0.0);
}

#[test]
fn resting_lens_does_not_blend_two_different_text_projections() {
    let rest = tab_flight_lens_material(Color::BLACK, 0.0);
    let held = tab_flight_lens_material(Color::BLACK, 1.0);
    assert_eq!(rest.optical_zoom, 1.0);
    assert_eq!(rest.backdrop_blur, Some((4.0, 1.0)));
    assert_eq!(rest.refraction_depth_dp, Some(36.0));
    assert_eq!(rest.fold_depth, 0.0);
    assert_eq!(rest.highlight, 0.0);
    assert_eq!(rest.meniscus_absorption, 0.0);
    assert_eq!(rest.shadow_style.unwrap().color.a(), 0.0);
    assert_eq!(held.optical_zoom, 1.0);
    assert_eq!(rest.refraction_depth_dp, held.refraction_depth_dp);
    let dynamics = tab_flight_dynamics(
        TabFlightGeometry {
            center: (50.0, 31.0),
            base_size: Size::new(95.0, 54.0),
            strain: Size::new(0.0, 0.0),
            lens_position: 0.0,
            lens_activity: 0.0,
            resting_tint: Color::TRANSPARENT,
            accessory_center: None,
        },
        TabFlightNode {
            origin: (0.0, 0.0),
            size: Size::new(140.0, 100.0),
        },
    );
    assert_eq!(dynamics.activity, Some(1.0));
}

#[test]
fn flight_lens_retains_neutral_tint_through_direct_motion() {
    assert_eq!(tab_flight_tint_multiplier(0.0), 1.0);
    assert!((tab_flight_tint_multiplier(1.0) - 0.75).abs() < f32::EPSILON);
    assert_eq!(tab_flight_tint_multiplier(-1.0), 1.0);
    assert!((tab_flight_tint_multiplier(2.0) - 0.75).abs() < f32::EPSILON);
}

#[test]
fn bar_surface_adapts_tone_to_its_foreground() {
    let glass = tab_bar_surface_material(cranpose_ui_graphics::Color::BLACK);
    assert_eq!(glass.blur_radius, Some(6.0));
    assert_eq!(glass.saturation, Some(1.0));
    assert_eq!(glass.lift, Some(0.0));
    assert_eq!(glass.refraction_depth, 0.0);
    assert_eq!(glass.refraction_depth_dp, Some(15.5));
    assert_eq!(
        glass.refraction,
        crate::material::GlassRefraction::Surface { reach_dp: 31.0 }
    );
    assert_eq!(glass.transmission_refraction, 1.0);
    assert_eq!(glass.adaptive_frost, 0.0);
    assert!(glass.adaptive_tone);
}

#[test]
fn bar_surface_keeps_its_refraction_after_release() {
    let glass = tab_bar_surface_material(Color::BLACK);
    assert_eq!(glass.refraction_depth_dp, Some(15.5));
    assert_eq!(glass.face_response.unwrap().illumination, 0.0);
}

#[test]
fn bar_surface_tone_tracks_the_local_foreground_polarity() {
    for foreground in [Color::BLACK, Color::WHITE] {
        let surface = tab_bar_surface_material(foreground);
        assert!(surface.adaptive_tone);
        assert_eq!(surface.foreground, Some(foreground));
        assert_eq!(surface.lift, Some(0.0));
    }
}

#[test]
fn bar_surface_tone_keeps_the_face_tint_neutral() {
    for foreground in [Color::BLACK, Color::WHITE] {
        assert_eq!(
            tab_bar_surface_material(foreground).tint,
            Some(Color::TRANSPARENT)
        );
    }
}

#[test]
fn contact_growth_follows_the_native_presentation_clock() {
    let mut samples = 0;
    for pressed in [false, true] {
        for route in 0..4 {
            let runtime =
                cranpose_core::Runtime::new(std::sync::Arc::new(cranpose_core::DefaultScheduler));
            let mut activity = cranpose_animation::Animatable::new(
                if pressed { 0.0 } else { 1.0 },
                runtime.handle(),
            );
            activity.animate_to_at(
                f32::from(pressed),
                tab_lens_activity_motion(pressed),
                1_000_000_000,
            );
            let mut error = 0.0;
            let mut count = 0;
            for line in include_str!("../../tests/fixtures/native_tab_contact.csv")
                .lines()
                .skip(1)
            {
                let sample = line
                    .split(',')
                    .map(|v| v.parse::<f64>().expect("contact sample"))
                    .collect::<Vec<_>>();
                if sample[0] != f64::from(pressed) || sample[1] != f64::from(route) {
                    continue;
                }
                runtime
                    .handle()
                    .drain_frame_callbacks(1_000_000_000 + (sample[2] * 1e9) as u64);
                error += (activity.state().value().clamp(0.0, 1.0) - sample[3] as f32).powi(2);
                count += 1;
            }
            assert!(count >= 30, "every native route must execute");
            let rms = (error / count as f32).sqrt();
            assert!(
                rms < 0.025,
                "native contact progress pressed={pressed} route={route} RMS: {rms}"
            );
            samples += count;
        }
    }
    assert!(samples >= 300);
}

#[test]
fn a_scope_declares_destinations_in_order() {
    let tabs = collect_tabs(|scope| {
        scope.tab("M0 0", "Discover");
        scope.app_badge("M1 1", "WWDC");
        scope.push(LiquidTab::new("M2 2", "Account").with_icon_scale(0.95));
    });

    assert_eq!(tabs.len(), 3);
    assert_eq!(tabs[0].label, "Discover");
    assert_eq!(tabs[0].icon_style, LiquidTabIconStyle::Plain);
    assert_eq!(tabs[0].icon_scale, 1.0);
    assert_eq!(tabs[1].icon_style, LiquidTabIconStyle::AppBadge);
    assert_eq!(tabs[2].icon_scale, 0.95);
}

#[test]
fn a_bar_with_no_destinations_declares_none() {
    assert!(collect_tabs(|_| {}).is_empty());
}

#[test]
fn native_cells_overlap_without_crossing_the_strip_ends() {
    let geometry = TabGeometry::new(88.0, 4);
    assert_eq!(geometry.width, 352.0);
    assert!((geometry.cell_width - 95.0).abs() < 1e-4);
    for (index, center) in [72.5, 158.16667, 243.83333, 329.5].into_iter().enumerate() {
        let actual =
            25.0 + tab_lens_resting_left(index, geometry.pitch, 4) + geometry.cell_width * 0.5;
        assert!((actual - center).abs() < 1e-4);
    }
    for count in [0, 1, 2, 4, 8] {
        for allocation in [1.0, 24.0, 78.0, 120.0, 400.0] {
            let geometry = TabGeometry::new(allocation, count);
            let last = tab_lens_resting_left(count, geometry.pitch, count);
            assert!((last + geometry.cell_width - geometry.width).abs() < 1e-3);
        }
    }
}

#[test]
fn selection_width_uses_the_destination_allocation() {
    for (allocation, native_width) in [(88.0, 95.0), (97.5, 104.5)] {
        assert!((tab_lens_rest_width(allocation) - native_width).abs() < 1e-4);
    }
}
#[test]
fn held_drag_coordinates_follow_the_lifted_cell_in_both_directions() {
    let geometry = TabGeometry::new(88.0, 4);
    for (travel, physical_x) in [(1.0, 304.66666), (-1.0, 47.333332)] {
        let transform = tab_bar_transform(360.0, 1.0, travel);
        let center = geometry.width * 0.5;
        let local_x = (physical_x - center - transform.translation_x) / transform.scale_x + center;
        let optical_x = geometry.optical_pointer_x(local_x, 1.0, travel);
        assert!(
            (optical_x - physical_x).abs() < 0.001,
            "pointer drift: {}",
            optical_x - physical_x
        );
        assert_eq!(
            geometry.optical_pointer_x(physical_x, 0.0, travel),
            physical_x
        );
    }
}
#[test]
fn ink_selection_uses_the_same_bounded_shape_before_face_magnification() {
    let geometry = TabGeometry::new(88.0, 4);
    for activity in [0.0, 0.5, 1.0] {
        for value in [-1000.0, -0.1, 0.0, 0.1, 1000.0] {
            let strain = Size::new(value, -value * 1.5);
            let selected = tab_ink_selection(geometry, 120.0, activity, strain, Color::BLUE);
            let (width, height) = tab_lens_base_size(geometry.cell_width, activity);
            let size = tab_lens_deformed_size(Size::new(width, height), strain);
            assert_eq!(selected.content_zoom, 1.0 + 0.16 * activity);
            assert!((selected.bounds.width - size.width).abs() < 0.0001);
            assert!((selected.bounds.height - size.height).abs() < 0.0001);
            assert!((selected.bounds.x + selected.bounds.width * 0.5 - 167.5).abs() < 0.0001);
            assert!(selected.bounds.width > 0.0 && selected.bounds.height > 0.0);
        }
    }
}
