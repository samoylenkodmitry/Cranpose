use std::time::{Duration, Instant};

use cranpose_live::{Edit, Expression, Patch, Registry, Session};
use serde_json::json;

fn measure(name: &str, samples: usize, mut update: impl FnMut(usize)) {
    let mut elapsed = Vec::with_capacity(samples);
    for index in 0..samples {
        let start = Instant::now();
        update(index);
        elapsed.push(start.elapsed());
    }
    elapsed.sort_unstable();
    let millis = |duration: Duration| duration.as_secs_f64() * 1000.0;
    eprintln!(
        "{name}: {samples} commits, p50={:.3}ms p95={:.3}ms max={:.3}ms",
        millis(elapsed[samples / 2]),
        millis(elapsed[samples * 95 / 100]),
        millis(elapsed[samples - 1])
    );
}

#[test]
#[ignore = "manual CPU latency measurement; excludes rendering and presentation"]
fn source_and_patch_commit_latency() {
    let samples = std::env::var("CRANPOSE_LIVE_SAMPLES")
        .map_or(1000, |value| value.parse().expect("integer sample count"));
    assert!(samples > 0);
    let rows = (0..100)
        .map(|index| format!("key(\"row{index}\", || Text(\"hello\".into()));"))
        .collect::<String>();
    let column = format!("fn Screen() {{ Column(|| {{ {rows} }}); }}");
    let row = column.replace("Column", "Row");
    let registry = Registry::discover().expect("catalogue");
    let program = cranpose_live::parse_source(&registry, &column, "Screen").expect("source");
    let session = Session::new(registry, program).expect("session");
    measure("Rust source, 101 nodes", samples, |index| {
        session
            .replace_source(
                session.revision(),
                if index % 2 == 0 { &row } else { &column },
                "Screen",
            )
            .expect("source update");
    });
    measure("JSON argument patch, 101 nodes", samples, |index| {
        session
            .apply(Patch {
                base_revision: session.revision(),
                edits: vec![Edit::SetArgument {
                    target: "row50".into(),
                    name: "text".into(),
                    value: Expression::Literal {
                        value: json!(index.to_string()),
                    },
                }],
            })
            .expect("patch update");
    });
}
