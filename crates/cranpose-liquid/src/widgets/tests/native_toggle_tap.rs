// The native iOS 27 toggle tap of tests/fixtures/native_toggle_tap.csv: one
// row per display-link frame, timed from the touch-down delivery.

pub(crate) const RELEASE_NANOS: u64 = 163_787_000;

pub(crate) struct NativeTapFrame {
    pub(crate) time_nanos: u64,
    pub(crate) width: f64,
    pub(crate) height: f64,
    pub(crate) white_opacity: f64,
    pub(crate) track_fill: f64,
}

pub(crate) fn frames() -> impl Iterator<Item = NativeTapFrame> {
    include_str!("../../../tests/fixtures/native_toggle_tap.csv")
        .lines()
        .skip(1)
        .map(|row| {
            let values = row
                .split(',')
                .map(|value| value.parse::<f64>().expect("native sample"))
                .collect::<Vec<_>>();
            NativeTapFrame {
                time_nanos: (values[0] * 1e9) as u64,
                width: values[1],
                height: values[2],
                white_opacity: values[3],
                track_fill: values[5],
            }
        })
}
