use super::*;

const TABLE: &[u8] = include_bytes!("../../tests/fixtures/default_tracking.trak");

#[test]
fn normal_tracking_interpolates_in_font_units_and_clamps_size_endpoints() {
    let table = ttf_parser::trak::Table::parse(TABLE).unwrap();
    let tracking = FontTracking::from_table(table, 2048.0).unwrap();
    for (size, units) in [
        (6.0, 38.0),
        (9.0, 38.0),
        (9.5, 31.0),
        (10.0, 24.0),
        (15.0, -11.0),
        (20.0, -46.0),
        (40.0, -46.0),
    ] {
        assert!((tracking.at_size(size) - units * size / 2048.0).abs() < 0.00001);
    }
    for size in [0.0, -1.0, f32::NAN, f32::INFINITY] {
        assert_eq!(tracking.at_size(size), 0.0);
    }
    assert_eq!(FontTracking::default().at_size(10.0), 0.0);
}

#[test]
fn missing_normal_track_and_unordered_sizes_are_ignored() {
    for (offset, value) in [(20, 65536i32), (28, 0), (32, 8 * 65536)] {
        let mut bytes = TABLE.to_vec();
        bytes[offset..offset + 4].copy_from_slice(&value.to_be_bytes());
        let table = ttf_parser::trak::Table::parse(&bytes).unwrap();
        assert!(FontTracking::from_table(table, 2048.0).is_none());
    }
    for length in 0..TABLE.len() {
        let parsed = ttf_parser::trak::Table::parse(&TABLE[..length]);
        assert!(
            parsed
                .and_then(|table| FontTracking::from_table(table, 2048.0))
                .is_none()
        );
    }
}
