use super::*;

fn rgb(color: Color) -> (u8, u8, u8) {
    (
        (color.0 * 255.0).round() as u8,
        (color.1 * 255.0).round() as u8,
        (color.2 * 255.0).round() as u8,
    )
}

#[test]
fn the_wear_scroll_indicator_colours_are_the_ones_compose_draws() {
    let on_background = Color::from_rgb_u8(0xDF, 0xF6, 0xFF);
    assert_eq!(rgb(set_luminance(on_background, 80.0)), (180, 202, 211));
    assert_eq!(rgb(set_luminance(on_background, 20.0)), (30, 51, 58));
}

#[test]
fn a_lab_round_trip_would_have_given_a_different_track() {
    let track = set_luminance(Color::from_rgb_u8(0xDF, 0xF6, 0xFF), 20.0);
    assert_eq!(rgb(track).0, 30, "not the Lab answer of 31");
}

#[test]
fn set_luminance_matches_the_shipped_aar() {
    const GOLDEN: &[(u32, f32, u32)] = &[
        (0xFFDF_F6FF, 0.0, 0xFF00_0000),
        (0xFFDF_F6FF, 1.0, 0xFF00_0407),
        (0xFFDF_F6FF, 5.0, 0xFF00_131A),
        (0xFFDF_F6FF, 20.0, 0xFF1E_333A),
        (0xFFDF_F6FF, 33.3, 0xFF3D_5259),
        (0xFFDF_F6FF, 50.0, 0xFF65_7A82),
        (0xFFDF_F6FF, 66.7, 0xFF90_A6AE),
        (0xFFDF_F6FF, 80.0, 0xFFB4_CAD3),
        (0xFFDF_F6FF, 95.0, 0xFFDE_F5FE),
        (0xFFDF_F6FF, 100.0, 0xFFFF_FFFF),
        (0xFFE3_E3E3, 20.0, 0xFF2F_3131),
        (0xFFE3_E3E3, 80.0, 0xFFC6_C6C6),
        (0xFFB9_F2FF, 20.0, 0xFF00_363E),
        (0xFFB9_F2FF, 80.0, 0xFF97_D0DC),
        (0xFF5E_7E93, 20.0, 0xFF10_3446),
        (0xFF5E_7E93, 80.0, 0xFFAA_CBE2),
        (0xFFFF_FFFF, 20.0, 0xFF2F_3131),
        (0xFF00_0000, 50.0, 0xFF77_7777),
        (0xFFFF_4B32, 20.0, 0xFF67_0500),
        (0xFFFF_4B32, 80.0, 0xFFFF_B4A7),
        (0xFF2F_A8F5, 20.0, 0xFF00_3351),
        (0xFF66_D9FF, 80.0, 0xFF61_D4FA),
    ];
    for &(source, l_star, expected) in GOLDEN {
        let got = argb_from_color(set_luminance(color_from_argb(source), l_star));
        assert_eq!(
            got, expected,
            "setLuminance(#{source:08X}, {l_star}) = #{got:08X}, want #{expected:08X}"
        );
    }
}

#[test]
fn the_cam16_forward_reports_the_hue_and_chroma_the_platform_reports() {
    let (hue, chroma) = cam16_hue_chroma(0xFFDF_F6FF);
    assert!((hue - 220.671_2).abs() < 1e-3, "hue {hue}");
    assert!((chroma - 15.368_903).abs() < 1e-4, "chroma {chroma}");
}

#[test]
fn the_ends_of_the_range_are_neutral_rather_than_solved() {
    assert_eq!(
        rgb(set_luminance(Color::from_rgb_u8(255, 0, 0), 0.0)),
        (0, 0, 0)
    );
    assert_eq!(
        rgb(set_luminance(Color::from_rgb_u8(255, 0, 0), 100.0)),
        (255, 255, 255)
    );
}

#[test]
fn a_grey_source_stays_grey_at_any_lightness() {
    for l_star in [10.0, 20.0, 50.0, 80.0, 90.0] {
        let (r, g, b) = rgb(set_luminance(Color::from_rgb_u8(0x80, 0x80, 0x80), l_star));
        assert!(
            r.abs_diff(g) <= 1 && g.abs_diff(b) <= 1,
            "L* {l_star} gave ({r}, {g}, {b})"
        );
    }
}

#[test]
fn the_critical_planes_are_the_eight_bit_boundaries() {
    for (index, &plane) in CRITICAL_PLANES.iter().enumerate() {
        let boundary = index as f64 + 0.5;
        assert!(
            (true_delinearized(plane) - boundary).abs() < 1e-9,
            "plane {index} is {plane}, which delinearizes to {}",
            true_delinearized(plane)
        );
    }
}
