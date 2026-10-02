use cranpose::prelude::*;

#[allow(non_snake_case)]
pub fn WatchApp() {
    Box(
        Modifier::empty().fill_max_size().background(Color(0.05, 0.1, 0.25, 1.0)),
        BoxSpec::default(),
        || {
            Text("Cranpose on watchOS", Modifier::empty().offset(16.0, 80.0), TextStyle::default());
        },
    );
}
