use cranpose_services::clear_platform_device_info;

use super::*;

struct Inner {
    total: Option<u64>,
}

impl DeviceInfo for Inner {
    fn total_memory_bytes(&self) -> Option<u64> {
        self.total
    }
}

fn wrapping(total: Option<u64>) -> ProcessDeviceInfo {
    ProcessDeviceInfo {
        inner: Rc::new(Inner { total }),
    }
}

#[test]
fn the_wrapped_device_info_still_answers_what_it_knew() {
    assert_eq!(wrapping(Some(6 << 30)).total_memory_bytes(), Some(6 << 30));
    assert_eq!(wrapping(None).total_memory_bytes(), None);
}

#[test]
fn installing_wraps_whatever_was_registered_rather_than_replacing_it() {
    clear_platform_device_info();
    set_platform_device_info(Rc::new(Inner {
        total: Some(3 << 30),
    }));

    install();

    let info = device_info();
    assert_eq!(
        info.total_memory_bytes(),
        Some(3 << 30),
        "the platform's own answer must survive the wrap"
    );
    assert!(
        info.process_cpu_time().is_some(),
        "unix reports processor time"
    );
    clear_platform_device_info();
}

#[test]
fn processor_time_only_goes_forwards() {
    let info = wrapping(None);
    let first = info
        .process_cpu_time()
        .expect("unix reports processor time");

    let mut sum = 0u64;
    for value in 0..2_000_000u64 {
        sum = sum.wrapping_add(value * value);
    }
    assert_ne!(sum, u64::MAX);

    let second = info
        .process_cpu_time()
        .expect("unix reports processor time");
    assert!(
        second >= first,
        "processor time went backwards: {first:?} then {second:?}"
    );
}

#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn the_resident_set_is_a_whole_number_of_pages() {
    let resident = wrapping(None)
        .resident_memory_bytes()
        .expect("linux reports a resident set");
    assert!(
        resident > 0 && resident.is_multiple_of(page_size_bytes()),
        "a resident set of {resident} bytes is not a whole number of pages"
    );
}

#[test]
fn releasing_free_memory_reports_whether_the_platform_has_the_call() {
    let info = wrapping(None);
    assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
    assert_eq!(info.release_free_memory(), cfg!(target_os = "android"));
}
