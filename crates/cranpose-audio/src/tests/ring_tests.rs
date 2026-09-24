use std::sync::atomic::AtomicUsize as TestCounter;

use super::*;

#[test]
fn capacity_rounds_up_to_a_power_of_two() {
    let (producer, _consumer) = channel::<u32>(5);
    assert_eq!(producer.capacity(), 8);
    let (producer, _consumer) = channel::<u32>(0);
    assert_eq!(producer.capacity(), 2);
    let (producer, _consumer) = channel::<u32>(64);
    assert_eq!(producer.capacity(), 64);
}

#[test]
fn round_trips_in_order() {
    let (mut producer, mut consumer) = channel::<u32>(4);
    assert!(consumer.is_empty());
    for value in 0..4 {
        producer.push(value).expect("space available");
    }
    assert!(!consumer.is_empty());
    for value in 0..4 {
        assert_eq!(consumer.pop(), Some(value));
    }
    assert_eq!(consumer.pop(), None);
}

#[test]
fn returns_the_value_when_full() {
    let (mut producer, mut consumer) = channel::<u32>(2);
    producer.push(1).expect("space available");
    producer.push(2).expect("space available");
    assert_eq!(producer.push(3), Err(3));
    assert_eq!(consumer.pop(), Some(1));
    producer.push(3).expect("space freed by the pop");
    assert_eq!(consumer.pop(), Some(2));
    assert_eq!(consumer.pop(), Some(3));
}

#[test]
fn wraps_around_many_times() {
    let (mut producer, mut consumer) = channel::<usize>(4);
    for round in 0..1000usize {
        producer.push(round).expect("space available");
        assert_eq!(consumer.pop(), Some(round));
    }
    assert!(consumer.is_empty());
}

#[test]
fn drops_pending_values_exactly_once() {
    static ALIVE: TestCounter = TestCounter::new(0);

    #[derive(Debug)]
    struct Tracked;
    impl Tracked {
        fn new() -> Tracked {
            ALIVE.fetch_add(1, Ordering::SeqCst);
            Tracked
        }
    }
    impl Drop for Tracked {
        fn drop(&mut self) {
            ALIVE.fetch_sub(1, Ordering::SeqCst);
        }
    }

    let (mut producer, mut consumer) = channel::<Tracked>(4);
    for _ in 0..4 {
        producer.push(Tracked::new()).expect("space available");
    }
    assert_eq!(ALIVE.load(Ordering::SeqCst), 4);
    drop(consumer.pop());
    assert_eq!(ALIVE.load(Ordering::SeqCst), 3);
    drop(producer);
    drop(consumer);
    assert_eq!(ALIVE.load(Ordering::SeqCst), 0);
}

#[test]
fn crosses_threads_without_losing_values() {
    let (mut producer, mut consumer) = channel::<usize>(16);
    let total = 10_000usize;
    let reader = std::thread::spawn(move || {
        let mut seen = 0usize;
        let mut sum = 0usize;
        while seen < total {
            if let Some(value) = consumer.pop() {
                sum += value;
                seen += 1;
            } else {
                std::hint::spin_loop();
            }
        }
        sum
    });
    for value in 0..total {
        while producer.push(value).is_err() {
            std::hint::spin_loop();
        }
    }
    assert_eq!(
        reader.join().expect("reader finishes"),
        total * (total - 1) / 2
    );
}
