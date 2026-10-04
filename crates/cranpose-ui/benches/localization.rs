use std::{hint::black_box, time::Duration};

use cranpose_core::{Composition, MemoryApplier};
use cranpose_ui::{
    AppContext, Catalog, Locale, Modifier, ProvideLocalization, Text, TextStyle, composable,
    localization::Resource, tr,
};
use criterion::{Criterion, criterion_group, criterion_main};

#[composable(no_skip)]
fn Label(localized: bool, count: u32) {
    if localized {
        Text(
            tr!("Files: {count}", id = "files", count = count),
            Modifier::empty(),
            TextStyle::default(),
        );
    } else {
        Text(
            format!("Files: {count}"),
            Modifier::empty(),
            TextStyle::default(),
        );
    }
}

fn benchmark(c: &mut Criterion) {
    let context = AppContext::new();
    context.enter(|| {
        let catalog = Catalog::from_resources(
            "en",
            &[
                Resource {
                    locale: "en",
                    namespace: "cranpose-ui",
                    source: "files = Files: { $count }",
                },
                Resource {
                    locale: "fr",
                    namespace: "cranpose-ui",
                    source: "files = Fichiers : { $count }",
                },
            ],
        )
        .expect("catalog");
        let locales = [
            Locale::parse("en").expect("English"),
            Locale::parse("fr").expect("French"),
        ];
        let mut group = c.benchmark_group("localization");
        group
            .sample_size(20)
            .measurement_time(Duration::from_secs(2));
        for (name, localized, change_count, change_locale) in [
            ("plain", false, false, false),
            ("unchanged", true, false, false),
            ("arguments", true, true, false),
            ("language", true, false, true),
        ] {
            let mut composition = Composition::new(MemoryApplier::new());
            let mut iteration = 0u32;
            let mut render = || {
                iteration = iteration.wrapping_add(1);
                let count = if change_count { iteration } else { 2 };
                let language = if change_locale {
                    iteration as usize % 2
                } else {
                    0
                };
                composition
                    .render(17, || {
                        ProvideLocalization(&catalog, locales[language].clone(), || {
                            Label(localized, black_box(count));
                        });
                    })
                    .expect("render");
            };
            render();
            group.bench_function(name, |b| b.iter(&mut render));
        }
        group.finish();
    });
}

criterion_group!(benches, benchmark);
criterion_main!(benches);
