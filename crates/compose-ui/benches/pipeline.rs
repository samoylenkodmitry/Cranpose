use compose_core::{location_key, Composition, Key, MemoryApplier};
use compose_ui::{
    composable, measure_layout, Column, ColumnSpec, HeadlessRenderer, LayoutMeasurements, Modifier,
    Row, RowSpec, Size, Text,
};
use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion};

const SECTION_COUNT: usize = 4;
const ROWS_PER_SECTION: usize = 64;
const ROWS_PER_SECTION_SAMPLES: &[usize] = &[ROWS_PER_SECTION];
const RECURSIVE_ROWS_PER_LEVEL: usize = 8;
const RECURSIVE_DEPTH: usize = 8;
const RECURSIVE_DEPTH_SAMPLES: &[usize] = &[RECURSIVE_DEPTH];
const ROOT_SIZE: Size = Size {
    width: 1080.0,
    height: 1920.0,
};

#[composable]
fn pipeline_content(sections: usize, rows_per_section: usize) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            for section in 0..sections {
                Column(
                    Modifier::empty().fill_max_width(),
                    ColumnSpec::default(),
                    move || {
                        Text(format!("Section {section}"), Modifier::empty());
                        for row in 0..rows_per_section {
                            Row(Modifier::empty().fill_max_width(), RowSpec::default(), move || {
                                Text(format!("Item {section}-{row} title"), Modifier::empty().weight(1.0));
                                Text(format!("Detail {section}-{row}"), Modifier::empty());
                            });
                        }
                    },
                );
            }
        },
    );
}

#[composable]
fn recursive_pipeline_content(depth: usize, rows_per_level: usize) {
    Column(
        Modifier::empty().fill_max_size(),
        ColumnSpec::default(),
        move || {
            recursive_section(depth, rows_per_level, 0);
        },
    );
}

#[composable]
fn recursive_section(depth: usize, rows_per_level: usize, level: usize) {
    if depth == 0 {
        return;
    }
    Column(
        Modifier::empty().fill_max_width(),
        ColumnSpec::default(),
        move || {
            Text(format!("Level {level}"), Modifier::empty());
            for row in 0..rows_per_level {
                Row(Modifier::empty().fill_max_width(), RowSpec::default(), move || {
                    Text(format!("Node {level}-{row} title"), Modifier::empty().weight(1.0));
                    Text(format!("Detail {level}-{row}"), Modifier::empty());
                });
            }
            recursive_section(depth - 1, rows_per_level, level + 1);
        },
    );
}

struct PipelineFixture {
    composition: Composition<MemoryApplier>,
    key: Key,
    sections: usize,
    rows_per_section: usize,
    root_size: Size,
}

impl PipelineFixture {
    fn new(sections: usize, rows_per_section: usize, root_size: Size) -> Self {
        let key = location_key(file!(), line!(), column!());
        Self {
            composition: Composition::new(MemoryApplier::new()),
            key,
            sections,
            rows_per_section,
            root_size,
        }
    }

    fn compose(&mut self) {
        let sections = self.sections;
        let rows_per_section = self.rows_per_section;
        self.composition
            .render(self.key, || pipeline_content(sections, rows_per_section))
            .expect("composition");
    }

    fn measure(&mut self) -> LayoutMeasurements {
        let root = self.composition.root().expect("composition root");
        let mut applier_guard = self.composition.applier_mut();
        let mut temp_applier = std::mem::take(&mut *applier_guard);
        let measurements =
            measure_layout(&mut temp_applier, root, self.root_size).expect("measure");
        *applier_guard = temp_applier;
        measurements
    }
}

struct RecursiveFixture {
    composition: Composition<MemoryApplier>,
    key: Key,
    depth: usize,
    rows_per_level: usize,
    root_size: Size,
}

impl RecursiveFixture {
    fn new(depth: usize, rows_per_level: usize, root_size: Size) -> Self {
        let key = location_key(file!(), line!(), column!());
        Self {
            composition: Composition::new(MemoryApplier::new()),
            key,
            depth,
            rows_per_level,
            root_size,
        }
    }

    fn compose(&mut self) {
        let depth = self.depth;
        let rows_per_level = self.rows_per_level;
        self.composition
            .render(self.key, || {
                recursive_pipeline_content(depth, rows_per_level)
            })
            .expect("composition");
    }

    fn measure(&mut self) -> LayoutMeasurements {
        let root = self.composition.root().expect("composition root");
        let mut applier_guard = self.composition.applier_mut();
        let mut temp_applier = std::mem::take(&mut *applier_guard);
        let measurements =
            measure_layout(&mut temp_applier, root, self.root_size).expect("measure");
        *applier_guard = temp_applier;
        measurements
    }
}

fn ui_object_count(sections: usize, rows_per_section: usize) -> usize {
    1 + sections * (2 + rows_per_section * 3)
}

fn recursive_ui_object_count(depth: usize, rows_per_level: usize) -> usize {
    1 + depth * (2 + rows_per_level * 3)
}

fn bench_composition(c: &mut Criterion) {
    let sections = SECTION_COUNT;
    let mut group = c.benchmark_group("pipeline_composition");
    for &rows_per_section in ROWS_PER_SECTION_SAMPLES {
        let total_ui_objects = ui_object_count(sections, rows_per_section);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &(sections, rows_per_section),
            |b, &(sections, rows_per_section)| {
                let mut fixture = PipelineFixture::new(sections, rows_per_section, ROOT_SIZE);
                // Warm up the composition so steady-state recomposition is measured.
                fixture.compose();

                b.iter(|| {
                    fixture.compose();
                });
            },
        );
    }
    group.finish();
}

fn bench_measure(c: &mut Criterion) {
    let mut group = c.benchmark_group("pipeline_measure");
    for &rows_per_section in ROWS_PER_SECTION_SAMPLES {
        let sections = SECTION_COUNT;
        let total_ui_objects = ui_object_count(sections, rows_per_section);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &(sections, rows_per_section),
            |b, &(sections, rows_per_section)| {
                let mut fixture = PipelineFixture::new(sections, rows_per_section, ROOT_SIZE);
                fixture.compose();

                b.iter(|| {
                    let measurements = fixture.measure();
                    black_box(measurements);
                });
            },
        );
    }
    group.finish();
}

fn bench_layout(c: &mut Criterion) {
    let sections = SECTION_COUNT;
    let mut group = c.benchmark_group("pipeline_layout");
    for &rows_per_section in ROWS_PER_SECTION_SAMPLES {
        let total_ui_objects = ui_object_count(sections, rows_per_section);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &(sections, rows_per_section),
            |b, &(sections, rows_per_section)| {
                let mut fixture = PipelineFixture::new(sections, rows_per_section, ROOT_SIZE);
                fixture.compose();
                let measurements = fixture.measure();

                b.iter(|| {
                    let tree = measurements.layout_tree();
                    black_box(tree);
                });
            },
        );
    }
    group.finish();
}

fn bench_render(c: &mut Criterion) {
    let sections = SECTION_COUNT;
    let mut group = c.benchmark_group("pipeline_render");
    for &rows_per_section in ROWS_PER_SECTION_SAMPLES {
        let total_ui_objects = ui_object_count(sections, rows_per_section);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &(sections, rows_per_section),
            |b, &(sections, rows_per_section)| {
                let mut fixture = PipelineFixture::new(sections, rows_per_section, ROOT_SIZE);
                fixture.compose();
                let measurements = fixture.measure();
                let layout_tree = measurements.layout_tree();
                let renderer = HeadlessRenderer::new();

                b.iter(|| {
                    let scene = renderer.render(&layout_tree);
                    black_box(scene);
                });
            },
        );
    }
    group.finish();
}

fn bench_full_pipeline(c: &mut Criterion) {
    let mut fixture = PipelineFixture::new(SECTION_COUNT, ROWS_PER_SECTION, ROOT_SIZE);
    let renderer = HeadlessRenderer::new();

    c.bench_function("pipeline_full", |b| {
        b.iter(|| {
            fixture.compose();
            let measurements = fixture.measure();
            let layout_tree = measurements.layout_tree();
            let scene = renderer.render(&layout_tree);
            black_box(scene);
        });
    });
}

fn bench_recursive_composition(c: &mut Criterion) {
    let mut group = c.benchmark_group("recursive_composition");
    for &depth in RECURSIVE_DEPTH_SAMPLES {
        let total_ui_objects = recursive_ui_object_count(depth, RECURSIVE_ROWS_PER_LEVEL);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &depth,
            |b, &depth| {
                let mut fixture = RecursiveFixture::new(depth, RECURSIVE_ROWS_PER_LEVEL, ROOT_SIZE);
                fixture.compose();
                b.iter(|| {
                    fixture.compose();
                });
            },
        );
    }
    group.finish();
}

fn bench_recursive_measure(c: &mut Criterion) {
    let mut group = c.benchmark_group("recursive_measure");
    for &depth in RECURSIVE_DEPTH_SAMPLES {
        let total_ui_objects = recursive_ui_object_count(depth, RECURSIVE_ROWS_PER_LEVEL);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &depth,
            |b, &depth| {
                let mut fixture = RecursiveFixture::new(depth, RECURSIVE_ROWS_PER_LEVEL, ROOT_SIZE);
                fixture.compose();
                b.iter(|| {
                    let measurements = fixture.measure();
                    black_box(measurements);
                });
            },
        );
    }
    group.finish();
}

fn bench_recursive_layout(c: &mut Criterion) {
    let mut group = c.benchmark_group("recursive_layout");
    for &depth in RECURSIVE_DEPTH_SAMPLES {
        let total_ui_objects = recursive_ui_object_count(depth, RECURSIVE_ROWS_PER_LEVEL);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &depth,
            |b, &depth| {
                let mut fixture = RecursiveFixture::new(depth, RECURSIVE_ROWS_PER_LEVEL, ROOT_SIZE);
                fixture.compose();
                let measurements = fixture.measure();
                b.iter(|| {
                    let tree = measurements.layout_tree();
                    black_box(tree);
                });
            },
        );
    }
    group.finish();
}

fn bench_recursive_render(c: &mut Criterion) {
    let mut group = c.benchmark_group("recursive_render");
    for &depth in RECURSIVE_DEPTH_SAMPLES {
        let total_ui_objects = recursive_ui_object_count(depth, RECURSIVE_ROWS_PER_LEVEL);
        group.bench_with_input(
            BenchmarkId::new("ui_objects", total_ui_objects),
            &depth,
            |b, &depth| {
                let mut fixture = RecursiveFixture::new(depth, RECURSIVE_ROWS_PER_LEVEL, ROOT_SIZE);
                fixture.compose();
                let measurements = fixture.measure();
                let layout_tree = measurements.layout_tree();
                let renderer = HeadlessRenderer::new();
                b.iter(|| {
                    let scene = renderer.render(&layout_tree);
                    black_box(scene);
                });
            },
        );
    }
    group.finish();
}

criterion_group!(
    pipeline,
    bench_composition,
    bench_measure,
    bench_layout,
    bench_render,
    bench_full_pipeline,
    bench_recursive_composition,
    bench_recursive_measure,
    bench_recursive_layout,
    bench_recursive_render
);
criterion_main!(pipeline);
