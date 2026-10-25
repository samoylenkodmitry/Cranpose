use std::sync::Arc;

use super::*;

#[test]
fn ordering_and_coalescing_contract() {
    let queue = Arc::new(HapticQueue::new(4));

    assert!(
        queue
            .enqueue(HapticCommand::Perform(HapticFeedback::ImpactLight))
            .is_ok()
    );
    assert!(
        queue
            .enqueue(HapticCommand::OneShot {
                duration_ms: 10,
                amplitude: 0,
            })
            .is_ok()
    );
    assert!(queue.enqueue(HapticCommand::Cancel).is_ok());
    assert!(
        queue
            .enqueue(HapticCommand::Waveform {
                timings_ms: vec![1],
                amplitudes: vec![1],
                repeat: -1,
            })
            .is_ok()
    );

    for step in [2i64, 3] {
        assert!(
            queue
                .enqueue(HapticCommand::Waveform {
                    timings_ms: vec![step],
                    amplitudes: vec![step as i32],
                    repeat: if step == 3 { 0 } else { -1 },
                })
                .is_ok()
        );
    }

    let enqueuer = std::thread::spawn({
        let queue = Arc::clone(&queue);
        move || {
            assert!(
                queue
                    .enqueue(HapticCommand::Effect(HapticEffect::Tick))
                    .is_ok()
            );
        }
    });
    std::thread::sleep(std::time::Duration::from_millis(50));

    let mut delivered = Vec::new();
    for _ in 0..5 {
        let command = queue.dequeue().expect("queue is not shut down");
        queue.note_delivered();
        delivered.push(command);
    }
    enqueuer.join().expect("blocked enqueue completes");

    assert!(matches!(
        delivered[0],
        HapticCommand::Perform(HapticFeedback::ImpactLight)
    ));
    assert!(matches!(
        delivered[1],
        HapticCommand::OneShot {
            duration_ms: 10,
            amplitude: 0,
        }
    ));
    assert!(matches!(delivered[2], HapticCommand::Cancel));
    match &delivered[3] {
        HapticCommand::Waveform {
            timings_ms,
            amplitudes,
            repeat,
        } => {
            assert_eq!(timings_ms, &[3]);
            assert_eq!(amplitudes, &[3]);
            assert_eq!(*repeat, 0);
        }
        _ => panic!("expected the coalesced waveform"),
    }
    assert!(matches!(
        delivered[4],
        HapticCommand::Effect(HapticEffect::Tick)
    ));

    assert!(queue.enqueue(HapticCommand::Cancel).is_ok());
    queue.shut_down();
    assert!(matches!(
        queue.enqueue(HapticCommand::Perform(HapticFeedback::Success)),
        Err(HapticCommand::Perform(HapticFeedback::Success))
    ));
    assert!(matches!(queue.dequeue(), Some(HapticCommand::Cancel)));
    queue.note_delivered();
    assert!(queue.dequeue().is_none());

    let (enqueued, coalesced, delivered) = queue.stats();
    assert_eq!(enqueued, 8);
    assert_eq!(coalesced, 2);
    assert_eq!(delivered, enqueued - coalesced);
}
