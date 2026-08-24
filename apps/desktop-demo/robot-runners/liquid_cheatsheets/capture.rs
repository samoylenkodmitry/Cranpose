//! Real-X11 capture support for the static Liquid cheatsheet fixtures.

use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::{Arc, Condvar, Mutex},
    thread::JoinHandle,
    time::{Duration, Instant},
};

use anyhow::{bail, Context, Result};
use cranpose::{Robot, RobotScreenshot};
use image::{imageops::crop_imm, RgbaImage};

const WORKER_READY_TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Clone, Copy, Debug)]
pub(crate) struct Crop {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Clone, Debug)]
pub(crate) struct Keyframe {
    pub capture_ms: u64,
    pub label: String,
}

pub(crate) fn save_exact_robot_keyframe_crops<Label: AsRef<str>>(
    screenshots: &[RobotScreenshot],
    case_dir: &Path,
    crop: Crop,
    labels: &[Label],
) -> Result<Vec<PathBuf>> {
    if screenshots.len() != labels.len() {
        bail!(
            "exact-clock capture returned {} screenshots for {} labels",
            screenshots.len(),
            labels.len()
        );
    }
    let output_dir = case_dir.join("actual/keyframes");
    fs::create_dir_all(&output_dir).context("create exact-clock keyframe directory")?;
    let mut outputs = Vec::with_capacity(screenshots.len());
    for (index, (screenshot, label)) in screenshots.iter().zip(labels).enumerate() {
        let scale_x = screenshot.width as f32 / screenshot.logical_width.max(f32::EPSILON);
        let scale_y = screenshot.height as f32 / screenshot.logical_height.max(f32::EPSILON);
        if (scale_x - scale_y).abs() > 0.01 {
            bail!("exact-clock screenshot has non-uniform scale {scale_x}x by {scale_y}x");
        }
        let x = (crop.x * scale_x).round() as u32;
        let y = (crop.y * scale_y).round() as u32;
        let width = (crop.width * scale_x).round() as u32;
        let height = (crop.height * scale_y).round() as u32;
        if width == 0
            || height == 0
            || x.saturating_add(width) > screenshot.width
            || y.saturating_add(height) > screenshot.height
        {
            bail!(
                "exact-clock crop {width}x{height}+{x}+{y} exceeds screenshot {}x{}",
                screenshot.width,
                screenshot.height
            );
        }
        let image = RgbaImage::from_raw(
            screenshot.width,
            screenshot.height,
            screenshot.pixels.clone(),
        )
        .context("decode exact-clock screenshot buffer")?;
        let output = output_dir.join(format!("{:02}-{}.png", index + 1, label.as_ref()));
        crop_imm(&image, x, y, width, height)
            .to_image()
            .save(&output)
            .with_context(|| format!("save exact-clock keyframe {}", output.display()))?;
        outputs.push(output);
    }
    Ok(outputs)
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct CaptureEpoch(Instant);

impl CaptureEpoch {
    pub(crate) fn sleep_until(self, elapsed_ms: u64) {
        let target = Duration::from_millis(elapsed_ms);
        std::thread::sleep(target.saturating_sub(self.0.elapsed()));
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ComparisonGrid {
    pub columns: usize,
    pub tile_width: usize,
    pub gap: usize,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ActualTiming {
    PresentedInteraction,
    SettledState,
    MixedPhases,
}

pub(crate) struct CaptureRequest<'a> {
    pub window_id: &'a str,
    pub case_dir: &'a Path,
    pub logical_window_size: (f32, f32),
    pub crop: Crop,
    pub total_duration: Duration,
    pub keyframes: &'a [Keyframe],
}

struct RawCapture {
    index: usize,
    actual_ms: u64,
    source_label: String,
    path: PathBuf,
}

type CaptureWorker = JoinHandle<Result<Option<RawCapture>>>;

#[derive(Clone, Copy)]
enum CaptureCommand {
    Pending,
    Start(Instant),
    Cancel,
}

struct CaptureGateState {
    ready_workers: usize,
    command: CaptureCommand,
}

struct CaptureGate {
    state: Mutex<CaptureGateState>,
    changed: Condvar,
}

struct CancelCaptureOnDrop(Arc<CaptureGate>);

impl Drop for CancelCaptureOnDrop {
    fn drop(&mut self) {
        self.0.cancel_best_effort();
    }
}

impl CaptureGate {
    fn new() -> Self {
        Self {
            state: Mutex::new(CaptureGateState {
                ready_workers: 0,
                command: CaptureCommand::Pending,
            }),
            changed: Condvar::new(),
        }
    }

    fn ready_and_wait(&self) -> Result<Option<Instant>> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
        state.ready_workers += 1;
        self.changed.notify_all();
        loop {
            match state.command {
                CaptureCommand::Pending => {
                    state = self
                        .changed
                        .wait(state)
                        .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
                }
                CaptureCommand::Start(epoch) => return Ok(Some(epoch)),
                CaptureCommand::Cancel => return Ok(None),
            }
        }
    }

    fn wait_until_deadline(&self, epoch: Instant, elapsed_ms: u64) -> Result<bool> {
        let deadline = epoch
            .checked_add(Duration::from_millis(elapsed_ms))
            .context("compute X11 keyframe deadline")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
        loop {
            match state.command {
                CaptureCommand::Cancel => return Ok(false),
                CaptureCommand::Pending => {
                    bail!("X11 capture worker passed an unresolved start gate")
                }
                CaptureCommand::Start(_) => {
                    let remaining = deadline.saturating_duration_since(Instant::now());
                    if remaining.is_zero() {
                        return Ok(true);
                    }
                    let (next_state, _) = self
                        .changed
                        .wait_timeout(state, remaining)
                        .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
                    state = next_state;
                }
            }
        }
    }

    fn wait_until_ready(&self, expected_workers: usize) -> Result<()> {
        let deadline = Instant::now()
            .checked_add(WORKER_READY_TIMEOUT)
            .context("compute X11 worker-readiness deadline")?;
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
        while state.ready_workers < expected_workers {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!(
                    "only {} of {expected_workers} X11 capture workers became ready",
                    state.ready_workers
                );
            }
            let (next_state, timeout) = self
                .changed
                .wait_timeout(state, remaining)
                .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
            state = next_state;
            if timeout.timed_out() && state.ready_workers < expected_workers {
                bail!(
                    "only {} of {expected_workers} X11 capture workers became ready",
                    state.ready_workers
                );
            }
        }
        Ok(())
    }

    fn start(&self, epoch: Instant) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
        if !matches!(state.command, CaptureCommand::Pending) {
            bail!("X11 capture start gate was already resolved");
        }
        state.command = CaptureCommand::Start(epoch);
        self.changed.notify_all();
        Ok(())
    }

    fn cancel(&self) -> Result<()> {
        let mut state = self
            .state
            .lock()
            .map_err(|_| anyhow::anyhow!("X11 capture start gate was poisoned"))?;
        if !matches!(state.command, CaptureCommand::Cancel) {
            state.command = CaptureCommand::Cancel;
            self.changed.notify_all();
        }
        Ok(())
    }

    fn cancel_best_effort(&self) {
        if let Ok(mut state) = self.state.lock() {
            if !matches!(state.command, CaptureCommand::Cancel) {
                state.command = CaptureCommand::Cancel;
                self.changed.notify_all();
            }
        }
    }
}

/// Captures fresh cursor-free X11 snapshots at absolute interaction-relative
/// deadlines. A new X11 client is used for every frame because persistent X11
/// drawable capture does not observe Cranpose's transient overlay surfaces.
pub(crate) fn capture_x11_keyframes<Trigger, Continue>(
    robot: &Robot,
    request: CaptureRequest<'_>,
    trigger: Trigger,
    continue_interaction: Continue,
) -> Result<Vec<PathBuf>>
where
    Trigger: FnOnce(&Robot) -> Result<()>,
    Continue: FnOnce(&Robot, CaptureEpoch) -> Result<()>,
{
    capture_prepared_x11_frames(
        request,
        || {
            trigger(robot).context("perform captured interaction trigger")?;
            robot
                .wait_for_present_frame()
                .map_err(anyhow::Error::msg)
                .context("present captured interaction trigger")
        },
        |epoch| continue_interaction(robot, epoch).context("continue captured interaction"),
    )
}

/// Captures one settled fixture state without implying that an interaction
/// triggered a new presentation immediately before the snapshot.
pub(crate) fn capture_x11_static_keyframe(request: CaptureRequest<'_>) -> Result<PathBuf> {
    if request.keyframes.len() != 1 {
        bail!("a static X11 capture requires exactly one keyframe");
    }
    capture_prepared_x11_frames(request, || Ok(()), |_| Ok(()))?
        .into_iter()
        .next()
        .context("static X11 capture produced no frame")
}

fn capture_prepared_x11_frames<BeforeStart, AfterStart>(
    request: CaptureRequest<'_>,
    before_start: BeforeStart,
    after_start: AfterStart,
) -> Result<Vec<PathBuf>>
where
    BeforeStart: FnOnce() -> Result<()>,
    AfterStart: FnOnce(CaptureEpoch) -> Result<()>,
{
    let CaptureRequest {
        window_id,
        case_dir,
        logical_window_size,
        crop,
        total_duration,
        keyframes,
    } = request;
    validate_request(logical_window_size, crop, total_duration, keyframes)?;
    let actual_dir = prepare_actual_directory(case_dir)?;
    let geometry = window_geometry(window_id)?;
    let pixel_crop = PixelCrop::from_logical(crop, logical_window_size, geometry)?;
    let output_dir = actual_dir.join("keyframes");
    fs::create_dir_all(&output_dir).context("create X11 keyframe directory")?;
    let gate = Arc::new(CaptureGate::new());
    let _cancel_on_drop = CancelCaptureOnDrop(Arc::clone(&gate));
    let mut errors = Vec::new();
    let mut capture_workers = Vec::with_capacity(keyframes.len());
    for (index, keyframe) in keyframes.iter().cloned().enumerate() {
        let window_id = window_id.to_owned();
        let output_dir = output_dir.clone();
        let gate = Arc::clone(&gate);
        match std::thread::Builder::new()
            .name(format!("liquid-x11-capture-{}", index + 1))
            .spawn(move || -> Result<Option<RawCapture>> {
                let Some(interaction_zero) = gate.ready_and_wait()? else {
                    return Ok(None);
                };
                if !gate.wait_until_deadline(interaction_zero, keyframe.capture_ms)? {
                    return Ok(None);
                }
                let capture_started_ms = interaction_zero.elapsed().as_millis();
                let safe_source_label = safe_label(&keyframe.label);
                let raw = output_dir.join(format!("{:02}-capture.miff", index + 1));
                capture_window_raw(&window_id, &raw)?;
                let capture_finished_ms = interaction_zero.elapsed().as_millis();
                let actual_ms = u64::try_from((capture_started_ms + capture_finished_ms) / 2)
                    .context("convert measured X11 capture midpoint")?;
                Ok(Some(RawCapture {
                    index,
                    actual_ms,
                    source_label: safe_source_label,
                    path: raw,
                }))
            }) {
            Ok(worker) => capture_workers.push(worker),
            Err(error) => {
                errors.push(format!("spawn X11 capture worker: {error}"));
                break;
            }
        }
    }

    if errors.is_empty() {
        if let Err(error) = gate.wait_until_ready(keyframes.len()) {
            errors.push(format!("prepare X11 capture workers: {error:#}"));
        }
    }

    let mut interaction_zero = None;
    if errors.is_empty() {
        match before_start() {
            Ok(()) => {
                let epoch = Instant::now();
                match gate.start(epoch) {
                    Ok(()) => interaction_zero = Some(epoch),
                    Err(error) => errors.push(format!("start X11 capture workers: {error:#}")),
                }
            }
            Err(error) => errors.push(format!("prepare captured X11 state: {error:#}")),
        }
    }
    if interaction_zero.is_none() {
        if let Err(error) = gate.cancel() {
            errors.push(format!("cancel X11 capture workers: {error:#}"));
        }
    }
    if let Some(epoch) = interaction_zero {
        match after_start(CaptureEpoch(epoch)) {
            Ok(()) => {
                let remaining = total_duration.saturating_sub(epoch.elapsed());
                std::thread::sleep(remaining);
            }
            Err(error) => {
                errors.push(format!("continue captured X11 state: {error:#}"));
                if let Err(cancel_error) = gate.cancel() {
                    errors.push(format!("cancel X11 capture workers: {cancel_error:#}"));
                }
            }
        }
    }

    let mut raw_captures = Vec::with_capacity(capture_workers.len());
    collect_capture_workers(capture_workers, &mut raw_captures, &mut errors);
    if !errors.is_empty() {
        if let Err(error) = remove_raw_keyframes(&output_dir) {
            errors.push(format!("raw-keyframe cleanup failed: {error:#}"));
        }
        bail!("captured interaction failed:\n- {}", errors.join("\n- "));
    }

    raw_captures.sort_by_key(|capture| capture.index);
    let mut outputs = Vec::with_capacity(raw_captures.len());
    for capture in raw_captures {
        let output = output_dir.join(format!(
            "{:02}-{:06}ms-{}.png",
            capture.index + 1,
            capture.actual_ms,
            capture.source_label
        ));
        if let Err(error) = convert_window_crop(&capture.path, pixel_crop, &output) {
            errors.push(format!("convert {}: {error:#}", capture.path.display()));
            if output.exists() {
                if let Err(cleanup_error) = fs::remove_file(&output) {
                    errors.push(format!(
                        "remove partial output {}: {cleanup_error}",
                        output.display()
                    ));
                }
            }
        } else {
            outputs.push(output);
        }
        if let Err(error) = fs::remove_file(&capture.path) {
            errors.push(format!(
                "remove temporary MIFF {}: {error}",
                capture.path.display()
            ));
        }
    }
    if !errors.is_empty() {
        for output in &outputs {
            if let Err(error) = fs::remove_file(output) {
                errors.push(format!(
                    "remove partial output {}: {error}",
                    output.display()
                ));
            }
        }
        if let Err(error) = remove_raw_keyframes(&output_dir) {
            errors.push(format!("raw-keyframe cleanup failed: {error:#}"));
        }
        bail!("X11 keyframe conversion failed:\n- {}", errors.join("\n- "));
    }
    Ok(outputs)
}

fn collect_capture_workers(
    capture_workers: Vec<CaptureWorker>,
    raw_captures: &mut Vec<RawCapture>,
    errors: &mut Vec<String>,
) {
    for worker in capture_workers {
        match worker.join() {
            Ok(Ok(Some(capture))) => raw_captures.push(capture),
            Ok(Ok(None)) => {}
            Ok(Err(error)) => errors.push(format!("X11 capture worker failed: {error:#}")),
            Err(_) => errors.push("X11 capture worker panicked".to_string()),
        }
    }
}

/// Builds `actual-sheet.png` and appends it below the checked-in target sheet.
pub(crate) fn compose_comparison(
    target_sheet: &Path,
    actual_frames: &[PathBuf],
    output_comparison: &Path,
    grid: ComparisonGrid,
    expected_frames: usize,
    actual_timing: ActualTiming,
) -> Result<()> {
    if actual_frames.is_empty() {
        bail!("cannot compose a comparison without actual frames");
    }
    if !target_sheet.is_file() {
        bail!("target sheet does not exist: {}", target_sheet.display());
    }
    if output_comparison.file_name().and_then(|name| name.to_str()) != Some("comparison-sheet.png")
    {
        bail!("comparison output must be named comparison-sheet.png");
    }
    if grid.columns == 0 || grid.tile_width == 0 {
        bail!("comparison grid dimensions must be positive");
    }
    if actual_frames.len() != expected_frames {
        bail!(
            "comparison expected {expected_frames} actual frames but captured {}",
            actual_frames.len()
        );
    }
    let parent = output_comparison
        .parent()
        .context("comparison output has no parent")?;
    fs::create_dir_all(parent).context("create comparison output directory")?;
    let actual_sheet = parent.join("actual-sheet.png");
    let labeled_target_sheet = parent.join("target-labeled-sheet.png");
    let mut montage = Command::new("magick");
    montage.arg("montage");
    for frame in actual_frames {
        montage.args(["-label", &actual_label(frame)?]);
        montage.arg(frame);
    }
    let columns = actual_frames.len().min(grid.columns);
    montage
        .args([
            "-tile",
            &format!("{columns}x"),
            "-geometry",
            &format!("{}x+{}+{}", grid.tile_width, grid.gap, grid.gap),
        ])
        .args([
            "-background",
            "#11151d",
            "-fill",
            "#f5f7fa",
            "-pointsize",
            "14",
        ])
        .arg(&actual_sheet);
    run(&mut montage, "build actual frame montage")?;
    let actual_banner = match actual_timing {
        ActualTiming::PresentedInteraction => {
            "CRANPOSE ACTUAL | A = X11 snapshot midpoint after initiating event was presented | T = preserved source time; named phases may reset"
        }
        ActualTiming::SettledState => {
            "CRANPOSE ACTUAL | A = X11 snapshot midpoint during each settled fixture state | T = preserved source time"
        }
        ActualTiming::MixedPhases => {
            "CRANPOSE ACTUAL | A = X11 snapshot midpoint after phase start | event phases are presented; steady = settled | T = source time"
        }
    };
    run(
        Command::new("magick")
            .arg(&actual_sheet)
            .args([
                "-gravity",
                "north",
                "-background",
                "#0b0f15",
                "-splice",
                "0x52",
                "-fill",
                "#f5f7fa",
                "-pointsize",
                "16",
                "-annotate",
                "+0+13",
                actual_banner,
            ])
            .arg(&actual_sheet),
        "label actual frame montage",
    )?;
    run(
        Command::new("magick")
            .arg(target_sheet)
            .args([
                "-gravity",
                "north",
                "-background",
                "#0b0f15",
                "-splice",
                "0x52",
                "-fill",
                "#f5f7fa",
                "-pointsize",
                "16",
                "-annotate",
                "+0+13",
                "TARGET | stored video-reference keyframes | T = source time; named phases are independent clips",
            ])
            .arg(&labeled_target_sheet),
        "label stored target sheet",
    )?;
    run(
        Command::new("magick")
            .arg(&labeled_target_sheet)
            .arg(&actual_sheet)
            .args(["-background", "#11151d", "-gravity", "center", "-append"])
            .arg(output_comparison),
        "append target and actual sheets",
    )
}

fn validate_request(
    logical_window_size: (f32, f32),
    crop: Crop,
    total_duration: Duration,
    keyframes: &[Keyframe],
) -> Result<()> {
    if logical_window_size.0 <= 0.0
        || logical_window_size.1 <= 0.0
        || crop.width <= 0.0
        || crop.height <= 0.0
    {
        bail!("logical window size and crop dimensions must be positive");
    }
    if crop.x < 0.0
        || crop.y < 0.0
        || crop.x + crop.width > logical_window_size.0
        || crop.y + crop.height > logical_window_size.1
    {
        bail!("crop must be contained within the logical window");
    }
    if total_duration.is_zero()
        || keyframes.is_empty()
        || keyframes.iter().any(|frame| {
            frame.capture_ms > total_duration.as_millis() as u64 || frame.label.is_empty()
        })
        || keyframes
            .windows(2)
            .any(|pair| pair[0].capture_ms > pair[1].capture_ms)
    {
        bail!("keyframes must be ordered, labeled, non-empty, and within the requested duration");
    }
    Ok(())
}

fn prepare_actual_directory(case_dir: &Path) -> Result<PathBuf> {
    fs::create_dir_all(case_dir)
        .with_context(|| format!("create case directory {}", case_dir.display()))?;
    let actual = case_dir.join("actual");
    fs::create_dir_all(&actual).with_context(|| format!("create {}", actual.display()))?;
    let keyframes = actual.join("keyframes");
    if keyframes.is_dir() {
        for entry in fs::read_dir(&keyframes)
            .with_context(|| format!("read generated keyframes in {}", keyframes.display()))?
        {
            let entry = entry.context("read generated keyframe entry")?;
            let path = entry.path();
            if !entry
                .file_type()
                .context("read generated keyframe type")?
                .is_file()
            {
                bail!(
                    "unexpected non-file in generated keyframe directory: {}",
                    path.display()
                );
            }
            fs::remove_file(&path)
                .with_context(|| format!("replace generated keyframe {}", path.display()))?;
        }
    }
    Ok(actual)
}

fn remove_raw_keyframes(output_dir: &Path) -> Result<()> {
    if !output_dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(output_dir)
        .with_context(|| format!("read raw keyframes in {}", output_dir.display()))?
    {
        let entry = entry.context("read raw keyframe entry")?;
        let path = entry.path();
        if entry
            .file_type()
            .context("read raw keyframe type")?
            .is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("miff")
        {
            fs::remove_file(&path)
                .with_context(|| format!("remove raw keyframe {}", path.display()))?;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
struct PixelCrop {
    x: u32,
    y: u32,
    width: u32,
    height: u32,
}

impl PixelCrop {
    fn from_logical(crop: Crop, logical: (f32, f32), pixel: (u32, u32)) -> Result<Self> {
        let scale_x = pixel.0 as f32 / logical.0;
        let scale_y = pixel.1 as f32 / logical.1;
        if (scale_x - scale_y).abs() > 0.01 {
            bail!("X11 window has non-uniform scale: {scale_x}x by {scale_y}x");
        }
        Ok(Self {
            x: (crop.x * scale_x).round() as u32,
            y: (crop.y * scale_y).round() as u32,
            width: (crop.width * scale_x).round() as u32,
            height: (crop.height * scale_y).round() as u32,
        })
    }

    fn geometry(self) -> String {
        format!("{}x{}+{}+{}", self.width, self.height, self.x, self.y)
    }
}

fn window_geometry(window_id: &str) -> Result<(u32, u32)> {
    let output = Command::new("xwininfo")
        .args(["-id", window_id])
        .output()
        .context("query X11 window geometry")?;
    if !output.status.success() {
        bail!("xwininfo failed for window {window_id}");
    }
    let text = String::from_utf8(output.stdout).context("decode xwininfo output")?;
    Ok((
        xwininfo_value(&text, "Width:")?,
        xwininfo_value(&text, "Height:")?,
    ))
}

fn xwininfo_value(text: &str, field: &str) -> Result<u32> {
    text.lines()
        .find_map(|line| line.trim().strip_prefix(field))
        .map(str::trim)
        .and_then(|value| value.parse().ok())
        .with_context(|| format!("read {field} from xwininfo"))
}

fn capture_window_raw(window_id: &str, output: &Path) -> Result<()> {
    let encoded_output = format!("miff:{}", output.display());
    run(
        Command::new("import")
            .args(["-silent", "-window", window_id, "-compress", "none"])
            .arg(&encoded_output),
        "capture fresh raw X11 keyframe",
    )?;
    if !fs::metadata(output).is_ok_and(|metadata| metadata.len() > 0) {
        bail!("raw X11 keyframe is empty: {}", output.display());
    }
    Ok(())
}

fn convert_window_crop(input: &Path, crop: PixelCrop, output: &Path) -> Result<()> {
    let encoded_output = format!("png:{}", output.display());
    run(
        Command::new("magick")
            .arg(input)
            .arg("-crop")
            .arg(crop.geometry())
            .arg("+repage")
            .arg(&encoded_output),
        "crop raw X11 keyframe",
    )?;
    if !fs::metadata(output).is_ok_and(|metadata| metadata.len() > 0) {
        bail!("X11 keyframe is empty: {}", output.display());
    }
    Ok(())
}

fn actual_label(path: &Path) -> Result<String> {
    let name = path
        .file_stem()
        .and_then(|value| value.to_str())
        .context("actual frame filename is not UTF-8")?;
    let (_, payload) = name
        .split_once('-')
        .context("actual frame filename has no sequence prefix")?;
    let (elapsed, source) = payload
        .split_once('-')
        .context("actual frame filename has no source label")?;
    let elapsed = elapsed
        .strip_suffix("ms")
        .context("actual frame filename has no elapsed-ms suffix")?
        .parse::<u64>()
        .context("parse actual frame elapsed time")?;
    let source = source
        .split('-')
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ");
    Ok(format!("A +{elapsed}ms | T {source}"))
}

fn safe_label(label: &str) -> String {
    label
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character
            } else {
                '-'
            }
        })
        .collect()
}

fn run(command: &mut Command, purpose: &str) -> Result<()> {
    let status = command.status().with_context(|| purpose.to_owned())?;
    if !status.success() {
        bail!("{purpose} failed: {status}");
    }
    Ok(())
}
