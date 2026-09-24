use std::sync::Mutex;

use cranpose_ui_graphics::BlendMode;
use web_time::{Duration, Instant};

use super::*;
use crate::render::RunTier;

fn key(blend: BlendMode) -> ShapePipelineKey {
    ShapePipelineKey::general_for(blend, RunTier::Store)
}

#[test]
fn an_inactive_pipeline_compiler_leaves_shapes_synchronous() {
    assert!(Compiler::new(&PipelineCompiler::inactive(), |key| key).is_none());
}

#[test]
fn duplicate_and_excess_work_is_bounded_and_drop_cancels_queued_work() {
    let (started, observed) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let shared = PipelineCompiler::spawn();
    let mut compiler = Compiler::new(&shared, move |key| {
        started.send(key).unwrap();
        blocked.lock().unwrap().recv().unwrap();
        key
    })
    .expect("test compiler");
    let first = key(BlendMode::SrcOver);
    let second = key(BlendMode::DstOut);
    compiler.request(first);
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(2)).unwrap(),
        first
    );
    compiler.request(first);
    compiler.request(second);
    compiler.request(key(BlendMode::Plus));
    assert_eq!(compiler.pending.as_slice(), &[first, second]);
    drop(compiler);
    release.send(()).unwrap();
    assert_eq!(
        observed.recv_timeout(Duration::from_secs(2)),
        Err(mpsc::RecvTimeoutError::Disconnected)
    );
}

#[test]
fn results_keep_their_keys_and_publish_only_when_collected() {
    let mut compiler =
        Compiler::new(&PipelineCompiler::spawn(), |key| key.blend_mode).expect("test compiler");
    let expected = key(BlendMode::DstOut);
    compiler.request(expected);
    assert_eq!(compiler.pending.as_slice(), &[expected]);
    let deadline = Instant::now() + Duration::from_secs(2);
    let mut published = Vec::new();
    while published.is_empty() {
        compiler.collect(|key, value| published.push((key, value)));
        assert!(Instant::now() < deadline, "compiler did not finish");
        std::thread::yield_now();
    }
    assert_eq!(published, [(expected, BlendMode::DstOut)]);
    assert!(compiler.pending.is_empty());
}
