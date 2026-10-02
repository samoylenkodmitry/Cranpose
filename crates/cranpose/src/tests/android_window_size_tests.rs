use super::WindowSize;

#[test]
fn a_kept_size_reads_back_as_itself() {
    let size = WindowSize {
        width: 1080,
        height: 2143,
        density: 3.0,
    };
    assert_eq!(WindowSize::from_bytes(&size.to_bytes()), Some(size));
}

#[test]
fn a_file_that_names_no_window_is_not_a_size() {
    assert_eq!(WindowSize::from_bytes(&[]), None);
    assert_eq!(WindowSize::from_bytes(&[1; 13]), None);
    let empty = WindowSize {
        width: 0,
        height: 2143,
        density: 3.0,
    };
    assert_eq!(WindowSize::from_bytes(&empty.to_bytes()), None);
    let flat = WindowSize {
        width: 1080,
        height: 2143,
        density: f32::NAN,
    };
    assert_eq!(WindowSize::from_bytes(&flat.to_bytes()), None);
}
