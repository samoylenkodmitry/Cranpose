use super::*;

#[test]
fn registered_device_info_takes_precedence() {
    clear_platform_device_info();
    struct Fake;
    impl DeviceInfo for Fake {
        fn total_memory_bytes(&self) -> Option<u64> {
            Some(8 * 1024 * 1024 * 1024)
        }
    }
    set_platform_device_info(Rc::new(Fake));
    assert_eq!(device_info().total_memory_bytes(), Some(8 << 30));
    clear_platform_device_info();
}

#[test]
fn a_platform_that_will_not_say_reports_nothing_rather_than_zero() {
    clear_platform_device_info();
    struct Silent;
    impl DeviceInfo for Silent {
        fn total_memory_bytes(&self) -> Option<u64> {
            None
        }
    }
    set_platform_device_info(Rc::new(Silent));

    let info = device_info();
    assert_eq!(info.total_memory_bytes(), None);
    assert_eq!(info.resident_memory_bytes(), None);
    assert_eq!(info.available_memory_bytes(), None);
    assert_eq!(info.process_cpu_time(), None);
    assert!(!info.release_free_memory());
    assert!(!release_free_memory());
    clear_platform_device_info();
}

#[test]
fn a_platform_that_can_answer_is_asked_through_the_free_function() {
    clear_platform_device_info();
    struct Rich;
    impl DeviceInfo for Rich {
        fn total_memory_bytes(&self) -> Option<u64> {
            Some(4 << 30)
        }
        fn resident_memory_bytes(&self) -> Option<u64> {
            Some(256 << 20)
        }
        fn available_memory_bytes(&self) -> Option<u64> {
            Some(512 << 20)
        }
        fn process_cpu_time(&self) -> Option<Duration> {
            Some(Duration::from_millis(1_250))
        }
        fn release_free_memory(&self) -> bool {
            true
        }
    }
    set_platform_device_info(Rc::new(Rich));

    let info = device_info();
    assert_eq!(info.resident_memory_bytes(), Some(256 << 20));
    assert_eq!(info.available_memory_bytes(), Some(512 << 20));
    assert_eq!(info.process_cpu_time(), Some(Duration::from_millis(1_250)));
    assert!(release_free_memory());
    clear_platform_device_info();
}

#[test]
fn the_resident_set_is_the_second_field_of_statm_in_pages() {
    let statm = "123456 2048 512 64 0 1024 0\n";
    assert_eq!(resident_bytes_from_statm(statm, 4096), Some(2048 * 4096));
    assert_eq!(resident_bytes_from_statm(statm, 16384), Some(2048 * 16384));
}

#[test]
fn an_unreadable_statm_line_is_unknown_rather_than_no_memory() {
    for broken in ["", "123456", "123456 notanumber 512", "   "] {
        assert_eq!(
            resident_bytes_from_statm(broken, page_size_bytes()),
            None,
            "{broken:?} should read as unknown"
        );
    }
}
