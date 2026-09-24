use super::*;

fn sample() -> ShapeRecord {
    ShapeRecord {
        rect: [1.0, 2.0, 3.0, 4.0],
        radii: [5.0, 6.0, 7.0, 8.0],
        color: [0.2, 0.4, 0.6, 0.8],
        stroke_width: 9.0,
        flags: 10,
        brush: 11,
        reserved: 12,
        arc: [13.0, 14.0, 15.0, 16.0],
        arc_band: [17.0, 18.0, 19.0, 20.0],
        arc_normalized: [21.0, 22.0, 23.0, 24.0],
    }
}

fn append_sample(records: &mut ShapeRecords) {
    records.push(
        ShapeRecordBody {
            rect: [1.0, 2.0, 3.0, 4.0],
            color: [0.2, 0.4, 0.6, 0.8],
            stroke_width: 9.0,
            flags: 10,
            brush: 11,
            placement: 12,
            arc_geometry: [13.0, 14.0, 19.0, 20.0],
        },
        ShapeRecordCurve {
            radii: [5.0, 6.0, 7.0, 8.0],
            arc_normalized: [21.0, 22.0, 23.0, 24.0],
        },
        [15.0, 16.0, 17.0, 18.0],
    );
}

#[test]
fn columns_preserve_every_record_bit_and_gpu_field() {
    let record = sample();
    let mut special = record;
    special.arc = [
        f32::NEG_INFINITY,
        -0.0,
        f32::from_bits(0x7fc0_0021),
        f32::INFINITY,
    ];
    special.arc_band = [-0.0, f32::from_bits(0xffc0_0001), 1.0, 2.0];
    let mut records = ShapeRecords::default();
    assert!(records.is_empty());
    assert_eq!(records.get(0), None);
    append_sample(&mut records);
    let mut body = records.bodies()[0];
    body.arc_geometry = [f32::NEG_INFINITY, -0.0, 1.0, 2.0];
    records.push(
        body,
        records.curves()[0],
        [
            special.arc[2],
            special.arc[3],
            special.arc_band[0],
            special.arc_band[1],
        ],
    );
    assert_eq!(records.len(), 2);
    assert_eq!(records.iter().len(), 2);
    for (actual, expected) in records.iter().zip([record, special]) {
        assert_eq!(bytemuck::bytes_of(&actual), bytemuck::bytes_of(&expected));
    }
    assert_eq!(records.get(0), Some(record));
    assert_eq!(records.get(2), None);
    assert_eq!(records.iter().rev().nth(1), Some(record));
    assert_eq!(records.bodies()[0].arc_geometry, [13.0, 14.0, 19.0, 20.0]);
    assert_eq!(records.curves()[0].radii, record.radii);
    assert_eq!(records.curves()[0].arc_normalized, record.arc_normalized);
    assert_eq!(std::mem::size_of::<ShapeRecordBody>(), 64);
    assert_eq!(std::mem::size_of::<ShapeRecordCurve>(), 32);
    assert_eq!(
        &records.source_bytes()[..16],
        bytemuck::bytes_of(&[15.0f32, 16.0, 17.0, 18.0])
    );
}

#[test]
fn clearing_and_reserving_keep_columns_aligned_without_reallocating() {
    let mut records = ShapeRecords::with_capacity(4);
    append_sample(&mut records);
    let capacity = records.capacity();
    let bytes = records.heap_bytes();
    let bodies = records.bodies().as_ptr();
    let curves = records.curves().as_ptr();
    records.clear();
    records.reserve(2);
    append_sample(&mut records);
    assert_eq!(records.capacity(), capacity);
    assert_eq!(records.heap_bytes(), bytes);
    assert_eq!(records.bodies().as_ptr(), bodies);
    assert_eq!(records.curves().as_ptr(), curves);
    assert_eq!(records.len(), records.curves().len());
    assert_eq!(records.get(0), Some(sample()));
    assert_eq!(records.clone(), records);
}
