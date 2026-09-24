use super::*;

#[test]
fn a_buffer_reports_the_shape_it_was_built_with() {
    let buffer = SamplesBuffer::new(2, 48_000, vec![0.0; 96_000]);
    assert_eq!(buffer.channels().get(), 2);
    assert_eq!(buffer.sample_rate().get(), 48_000);
    assert_eq!(buffer.total_duration(), Some(Duration::from_secs(1)));
}

#[test]
fn a_buffer_yields_every_sample_once() {
    let buffer = SamplesBuffer::new(1, 8_000, vec![0.25, -0.5, 1.0]);
    assert_eq!(buffer.collect::<Vec<_>>(), vec![0.25, -0.5, 1.0]);
}

#[test]
fn cancelling_calls_the_stop_the_source_supplied() {
    let stopped = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = Arc::clone(&stopped);
    let cancel = SourceCancel::new(move || {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });

    cancel.cancel();
    cancel.cancel();

    assert_eq!(stopped.load(std::sync::atomic::Ordering::Relaxed), 2);
}

#[test]
fn cancelling_a_source_that_needs_no_stop_does_nothing() {
    SourceCancel::default().cancel();
}

#[test]
fn a_seek_error_says_which_kind_it_is() {
    assert_eq!(SeekError::Unsupported.to_string(), "this item cannot seek");
    assert_eq!(
        SeekError::Failed("no index".to_owned()).to_string(),
        "no index"
    );
}
