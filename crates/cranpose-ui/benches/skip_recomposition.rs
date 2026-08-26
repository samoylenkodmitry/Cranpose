use cranpose_core::{MemoryApplier, location_key};
use cranpose_ui::{Composition, Modifier, Text, TextStyle, composable};
use criterion::{Criterion, criterion_group, criterion_main};

#[composable]
fn static_label(label: &'static str) {
    Text(label.to_string(), Modifier::empty(), TextStyle::default());
}

fn skip_recomposition_static_label(c: &mut Criterion) {
    let mut composition = Composition::new(MemoryApplier::new());
    let key = location_key(file!(), line!(), column!());

    composition
        .render(key, || static_label("Hello"))
        .expect("initial render");

    c.bench_function("skip_recomposition_static_label", |b| {
        b.iter(|| {
            composition
                .render(key, || static_label("Hello"))
                .expect("render");
        });
    });
}

criterion_group!(benches, skip_recomposition_static_label);
criterion_main!(benches);
