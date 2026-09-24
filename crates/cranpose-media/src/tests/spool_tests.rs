use std::time::Instant;

use symphonia::core::io::MediaSource;

use super::*;

fn directory(tag: &str) -> PathBuf {
    cranpose_core::test_scratch_dir(env!("CARGO_MANIFEST_DIR"), tag)
}

struct Trickle {
    bytes: Vec<u8>,
    position: usize,
    step: usize,
    stall_after: Option<usize>,
}

impl Read for Trickle {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        if self.stall_after.is_some_and(|limit| self.position >= limit) {
            std::thread::sleep(Duration::from_secs(60));
            return Ok(0);
        }
        if self.position >= self.bytes.len() {
            return Ok(0);
        }
        let take = self
            .step
            .min(buffer.len())
            .min(self.bytes.len() - self.position);
        buffer[..take].copy_from_slice(&self.bytes[self.position..self.position + take]);
        self.position += take;
        std::thread::sleep(Duration::from_millis(1));
        Ok(take)
    }
}

fn trickle(bytes: Vec<u8>) -> Box<dyn Read + Send> {
    Box::new(Trickle {
        position: 0,
        step: 7,
        bytes,
        stall_after: None,
    })
}

fn payload(len: usize) -> Vec<u8> {
    (0..=255u8).cycle().take(len).collect()
}

fn spool(bytes: &[u8], tag: &str) -> (Spool, SourceCancel) {
    Spool::start(
        trickle(bytes.to_vec()),
        &directory(tag),
        Some(bytes.len() as u64),
    )
    .expect("spool starts")
}

#[test]
fn everything_written_comes_back_in_order() {
    let source = payload(4096);
    let (mut spool, _cancel) = spool(&source, "order");
    let mut read = Vec::new();
    spool.read_to_end(&mut read).expect("read to end");
    assert_eq!(read, source);
}

#[test]
fn a_seek_backwards_rereads_what_was_already_spooled() {
    let source = payload(2048);
    let (mut spool, _cancel) = spool(&source, "back");
    let mut head = [0u8; 64];
    spool.read_exact(&mut head).expect("read the head");
    spool.seek(SeekFrom::Start(0)).expect("seek to the start");
    let mut again = [0u8; 64];
    spool.read_exact(&mut again).expect("read the head again");
    assert_eq!(head, again);
}

#[test]
fn a_seek_forward_waits_for_the_download_to_reach_it() {
    let source = payload(8192);
    let (mut spool, _cancel) = spool(&source, "forward");
    spool.seek(SeekFrom::Start(8000)).expect("seek forward");
    let mut tail = [0u8; 16];
    spool.read_exact(&mut tail).expect("read the tail");
    assert_eq!(tail, source[8000..8016]);
}

#[test]
fn seeking_from_a_stated_end_does_not_wait_for_the_download() {
    let source = payload(1_048_576);
    let (mut spool, _cancel) = spool(&source, "stated-end");
    let started = Instant::now();
    let position = spool.seek(SeekFrom::End(-4)).expect("seek from the end");
    assert_eq!(position, 1_048_572);
    assert!(
        started.elapsed() < Duration::from_millis(500),
        "the seek waited {:?} for a length the provider had already stated",
        started.elapsed()
    );
}

#[test]
fn a_spool_never_reports_a_length_even_when_the_provider_stated_one() {
    let (spool, _cancel) = spool(&payload(4096), "no-stated-len");
    assert_eq!(spool.byte_len(), None);
    assert_eq!(
        spool.len,
        Some(4096),
        "but it still knows the one it was told"
    );
}

#[test]
fn a_stream_with_no_stated_length_is_measured_by_reading_it() {
    let source = payload(1024);
    let (mut spool, _cancel) =
        Spool::start(trickle(source.clone()), &directory("unstated-end"), None)
            .expect("spool starts");
    assert_eq!(spool.seek(SeekFrom::End(-4)).expect("seek"), 1020);
    let mut tail = [0u8; 4];
    spool.read_exact(&mut tail).expect("read the tail");
    assert_eq!(tail, source[1020..]);
}

#[test]
fn a_spooled_stream_reports_itself_as_seekable() {
    let (spool, _cancel) = spool(&payload(16), "shape");
    assert!(spool.is_seekable());
}

#[test]
fn the_spool_file_is_deleted_when_the_last_reader_goes() {
    let (spool, _cancel) = spool(&payload(512), "cleanup");
    let path = spool.shared.path.clone();
    assert!(path.exists());
    drop(spool);
    assert!(!path.exists(), "{} survived its reader", path.display());
}

#[test]
fn seeking_before_the_start_is_an_error_rather_than_a_wrap() {
    let (mut spool, _cancel) = spool(&payload(64), "underflow");
    assert!(spool.seek(SeekFrom::Current(-1)).is_err());
}

#[test]
fn a_stream_that_stops_delivering_fails_instead_of_waiting_for_ever() {
    let stalled: Box<dyn Read + Send> = Box::new(Trickle {
        bytes: payload(4096),
        position: 0,
        step: 7,
        stall_after: Some(64),
    });
    let (mut spool, _cancel) = Spool::start_with(
        stalled,
        &directory("stall"),
        Some(4096),
        Duration::from_millis(200),
    )
    .expect("spool starts");
    spool.seek(SeekFrom::Start(2048)).expect("seek");
    let error = spool.read(&mut [0u8; 16]).expect_err("the stream is dead");
    assert_eq!(error.kind(), io::ErrorKind::TimedOut);
}

#[test]
fn cancelling_ends_a_wait_that_would_otherwise_not_return() {
    let stalled: Box<dyn Read + Send> = Box::new(Trickle {
        bytes: payload(4096),
        position: 0,
        step: 7,
        stall_after: Some(64),
    });
    let (mut spool, cancel) = Spool::start_with(
        stalled,
        &directory("cancel"),
        Some(4096),
        Duration::from_secs(30),
    )
    .expect("spool starts");
    spool.seek(SeekFrom::Start(2048)).expect("seek");
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(50));
        cancel.cancel();
    });
    let started = Instant::now();
    assert_eq!(spool.read(&mut [0u8; 16]).expect("cancelled"), 0);
    assert!(started.elapsed() < Duration::from_secs(30));
}
