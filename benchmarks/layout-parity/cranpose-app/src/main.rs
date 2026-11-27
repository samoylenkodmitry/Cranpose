mod atlas;
mod stress;

use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::BufWriter,
    path::{Path, PathBuf},
    time::Duration,
};

use atlas::{AtlasConfig, ParityAtlasScreen};
use cranpose::AppLauncher;
use cranpose_app_shell::AppShell;
use cranpose_render_pixels::PixelsRenderer;
use cranpose_ui::LayoutBox;

const FRAME: Duration = Duration::from_nanos(16_666_667);

pub(crate) const FACES: [&[u8]; 3] = [
    include_bytes!("../../../compose-vs-cranpose/fonts/Roboto-Regular.ttf"),
    include_bytes!("../../../compose-vs-cranpose/fonts/Roboto-Medium.ttf"),
    include_bytes!("../../../compose-vs-cranpose/fonts/Roboto-Bold.ttf"),
];

fn arg_value(args: &[String], name: &str) -> Option<String> {
    let prefix = format!("--{name}=");
    let flag = format!("--{name}");
    for (idx, arg) in args.iter().enumerate() {
        if let Some(value) = arg.strip_prefix(&prefix) {
            return Some(value.to_owned());
        }
        if arg == &flag {
            return args.get(idx + 1).cloned();
        }
    }
    None
}

fn format_density(density: f32) -> String {
    let formatted = format!("{density:.2}");
    let trimmed = formatted.trim_end_matches('0').trim_end_matches('.');
    if trimmed.contains('.') {
        trimmed.to_owned()
    } else {
        format!("{trimmed}.0")
    }
}

fn collect_bounds(node: &LayoutBox, out: &mut BTreeMap<String, (f32, f32, f32, f32)>) {
    if let Some(tag) = node
        .node_data
        .semantics()
        .and_then(|semantics| semantics.test_tag.as_deref())
    {
        out.insert(
            tag.to_owned(),
            (node.rect.x, node.rect.y, node.rect.width, node.rect.height),
        );
    }
    for child in &node.children {
        collect_bounds(child, out);
    }
}

fn fmt_json_f32(value: f32) -> String {
    if value.is_nan() {
        "\"NaN\"".to_owned()
    } else if value.is_infinite() {
        if value.is_sign_positive() {
            "\"inf\"".to_owned()
        } else {
            "\"-inf\"".to_owned()
        }
    } else {
        format!("{value:.4}")
    }
}

fn write_bounds_json(
    path: &Path,
    config: &AtlasConfig,
    density: f32,
    bounds: &BTreeMap<String, (f32, f32, f32, f32)>,
) {
    let mut json = String::new();
    json.push_str("{\n  \"framework\": \"cranpose\",\n");
    json.push_str(&format!("  \"width_dp\": {},\n", config.width_dp));
    json.push_str(&format!("  \"height_dp\": {},\n", config.height_dp));
    json.push_str(&format!("  \"density\": {density:.4},\n"));
    json.push_str("  \"nodes\": {\n");
    let total = bounds.len();
    for (idx, (tag, (x, y, w, h))) in bounds.iter().enumerate() {
        let comma = if idx + 1 < total { "," } else { "" };
        json.push_str(&format!(
            "    \"{tag}\": {{\"x\": {}, \"y\": {}, \"width\": {}, \"height\": {}}}{comma}\n",
            fmt_json_f32(*x),
            fmt_json_f32(*y),
            fmt_json_f32(*w),
            fmt_json_f32(*h),
        ));
    }
    json.push_str("  }\n}\n");
    fs::write(path, json).expect("write cranpose_bounds.json");
}

fn write_png(path: &Path, width: u32, height: u32, rgba: &[u8]) {
    let file = File::create(path).expect("create cranpose.png");
    let mut encoder = png::Encoder::new(BufWriter::new(file), width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().expect("png header");
    writer.write_image_data(rgba).expect("png data");
}

fn run_headless(out_dir: &Path, config: &AtlasConfig, density: f32) {
    fs::create_dir_all(out_dir).expect("create output directory");
    let px_w = (config.width_dp as f32 * density).round() as u32;
    let px_h = (config.height_dp as f32 * density).round() as u32;
    let cfg = config.clone();
    let mut shell = AppShell::new_with_size_and_density(
        PixelsRenderer::with_fonts(&FACES),
        cranpose_core::location_key(file!(), line!(), column!()),
        move || ParityAtlasScreen(cfg.clone()),
        (px_w, px_h),
        (config.width_dp as f32, config.height_dp as f32),
        density,
    );
    shell.update_after_exact_interval(FRAME);
    shell.update_after_exact_interval(FRAME);

    let mut bounds = BTreeMap::new();
    if let Some(tree) = shell.layout_tree() {
        collect_bounds(tree.root(), &mut bounds);
    }

    let mut rgba = vec![0u8; (px_w as usize) * (px_h as usize) * 4];
    shell.renderer().draw_scaled(&mut rgba, px_w, px_h, density);
    write_png(&out_dir.join("cranpose.png"), px_w, px_h, &rgba);
    write_bounds_json(
        &out_dir.join("cranpose_bounds.json"),
        config,
        density,
        &bounds,
    );
    println!(
        "Cranpose headless render complete: {} tracked nodes -> {}",
        bounds.len(),
        out_dir.display()
    );
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.iter().any(|arg| arg == "--stress") {
        let json_out = arg_value(&args, "stress-json").map(PathBuf::from);
        let code = stress::run_cranpose_stress_suite(json_out.as_deref());
        std::process::exit(code);
    }

    let width_dp: u32 = arg_value(&args, "width")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1760);
    let height_dp: u32 = arg_value(&args, "height")
        .and_then(|v| v.parse().ok())
        .unwrap_or(980);
    let density: f32 = arg_value(&args, "density")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1.0);
    let config = AtlasConfig {
        width_dp,
        height_dp,
        density_label: format_density(density),
    };

    if let Some(headless_out) = arg_value(&args, "headless") {
        run_headless(&PathBuf::from(headless_out), &config, density);
        return;
    }

    let cfg = config.clone();
    if let Err(err) = AppLauncher::new()
        .with_title("Layout Parity Atlas - Cranpose")
        .with_size(width_dp, height_dp)
        .with_fonts(&FACES)
        .try_run(move || ParityAtlasScreen(cfg.clone()))
    {
        eprintln!("Failed to launch Cranpose: {err}");
        std::process::exit(1);
    }
}
