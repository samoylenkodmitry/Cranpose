use compose_core::{location_key, MemoryApplier};
use compose_ui::{composable, Composition, Modifier, Text};
use criterion::{criterion_group, criterion_main, Criterion};

#[composable]
fn static_label(label: &'static str) {
    Text(label.to_string(), Modifier::empty());
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
