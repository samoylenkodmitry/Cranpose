use std::time::{Duration, Instant};

const WIDTH: u32 = 1080;
const HEIGHT: u32 = 2244;
const PASS_COUNTS: &[u32] = &[0, 1, 2, 3, 5, 9, 13, 17, 25, 33];
const WARMUP: usize = 10;
const BATCH: usize = 20;

fn iterations() -> usize {
    std::env::var("PASS_BENCH_ITERATIONS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(60)
}

struct Sample {
    encode: Duration,
    submit: Duration,
    finish: Duration,
}

fn median(mut values: Vec<Duration>) -> f64 {
    values.sort_unstable();
    values[values.len() / 2].as_secs_f64() * 1000.0
}

fn linear_fit(points: &[(f64, f64)]) -> (f64, f64) {
    let n = points.len() as f64;
    let sum_x: f64 = points.iter().map(|(x, _)| x).sum();
    let sum_y: f64 = points.iter().map(|(_, y)| y).sum();
    let sum_xx: f64 = points.iter().map(|(x, _)| x * x).sum();
    let sum_xy: f64 = points.iter().map(|(x, y)| x * y).sum();
    let denominator = n * sum_xx - sum_x * sum_x;
    let slope = (n * sum_xy - sum_x * sum_y) / denominator;
    let intercept = (sum_y - slope * sum_x) / n;
    (slope, intercept)
}

fn main() {
    let mut instance_descriptor = wgpu::InstanceDescriptor::new_without_display_handle();
    instance_descriptor.backends = wgpu::Backends::all();
    let instance = wgpu::Instance::new(instance_descriptor);
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .expect("an adapter is required to time passes");
    let info = adapter.get_info();
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("pass count submit cost"),
        required_features: cranpose_render_wgpu::optional_device_features(&adapter),
        required_limits: wgpu::Limits::default(),
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::default(),
        trace: wgpu::Trace::Off,
    }))
    .expect("device request failed");

    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("full surface attachment"),
        size: wgpu::Extent3d {
            width: WIDTH,
            height: HEIGHT,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Bgra8UnormSrgb,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("initial clear"),
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let index = queue.submit(Some(encoder.finish()));
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .expect("initial clear");
    }

    let encode_one = |pass_count: u32, load: wgpu::LoadOp<wgpu::Color>, store: wgpu::StoreOp| {
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("timed passes"),
        });
        for _ in 0..pass_count {
            encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("empty full-surface pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load, store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
        }
        encoder.finish()
    };

    let run = |pass_count: u32, load: wgpu::LoadOp<wgpu::Color>, store: wgpu::StoreOp| -> Sample {
        let encode_start = Instant::now();
        let buffers: Vec<_> = (0..BATCH)
            .map(|_| encode_one(pass_count, load, store))
            .collect();
        let encode = encode_start.elapsed() / BATCH as u32;

        let submit_start = Instant::now();
        let index = queue.submit(buffers);
        let submit = submit_start.elapsed() / BATCH as u32;

        let finish_start = Instant::now();
        device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(index),
                timeout: None,
            })
            .expect("passes complete");
        let finish = finish_start.elapsed() / BATCH as u32;

        Sample {
            encode,
            submit,
            finish,
        }
    };

    let iterations = iterations();
    println!(
        "adapter: {} ({:?}, {:?})",
        info.name, info.backend, info.device_type
    );
    println!("surface: {WIDTH}x{HEIGHT}, {iterations} batches of {BATCH} submissions per point\n");
    const ARMS: usize = 2;
    let arms = [
        (
            "empty (Load/Store)",
            wgpu::LoadOp::Load,
            wgpu::StoreOp::Store,
        ),
        (
            "clearing (Clear/Store)",
            wgpu::LoadOp::Clear(wgpu::Color::BLACK),
            wgpu::StoreOp::Store,
        ),
    ];

    for (_, load, store) in arms {
        for _ in 0..WARMUP {
            let _ = run(13, load, store);
        }
    }

    let mut fits = Vec::new();
    for (label, load, store) in arms {
        println!("-- {label} --");
        println!("passes   encode_ms  submit_ms  finish_ms   total_ms");
        let mut totals = Vec::new();
        for &pass_count in PASS_COUNTS {
            for _ in 0..WARMUP {
                let _ = run(pass_count, load, store);
            }
            let mut encodes = Vec::with_capacity(iterations);
            let mut submits = Vec::with_capacity(iterations);
            let mut finishes = Vec::with_capacity(iterations);
            for _ in 0..iterations {
                let sample = run(pass_count, load, store);
                encodes.push(sample.encode);
                submits.push(sample.submit);
                finishes.push(sample.finish);
            }
            let (encode, submit, finish) = (median(encodes), median(submits), median(finishes));
            let total = encode + submit + finish;
            totals.push((f64::from(pass_count), total));
            println!(
                "{pass_count:>6}   {encode:>9.3}  {submit:>9.3}  {finish:>9.3}   {total:>8.3}"
            );
        }
        let (slope, intercept) = linear_fit(&totals);
        println!(
            "  fit: cost_ms = {intercept:.3} + {slope:.4} * passes  ->  {:.1} us per pass\n",
            slope * 1000.0
        );
        fits.push((label, slope));
    }

    assert_eq!(fits.len(), ARMS, "every arm must produce a fit");
    let empty_slope = fits[0].1;
    let clear_slope = fits[1].1;
    println!("marginal cost of one pass, including its tile store:");
    for (label, slope) in &fits {
        println!("  {label:<24} {:>7.1} us", slope * 1000.0);
    }
    println!(
        "  a full-surface clear on top of the pass adds only {:.1} us,\n\
         so what a pass writes is not what a pass costs.",
        (clear_slope - empty_slope) * 1000.0
    );
    println!(
        "  fold ceiling on this adapter, 13 passes -> 5: {:.3} ms",
        empty_slope * 8.0
    );
    println!(
        "\nShape is the transferable claim; the constant belongs to this adapter.\n\
         Re-run on the target for a target number."
    );
}
