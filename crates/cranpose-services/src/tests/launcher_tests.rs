use super::*;

#[test]
fn an_in_flight_request_outlives_the_process_that_started_it() {
    crate::preferences::set_platform_preferences(std::sync::Arc::new(
        crate::preferences::MemoryPreferences::new(),
    ));

    begin_request("test.pick");
    assert_eq!(in_flight_request().as_deref(), Some("test.pick"));

    IN_FLIGHT.with(|slot| *slot.borrow_mut() = None);
    assert_eq!(
        in_flight_request().as_deref(),
        Some("test.pick"),
        "a restarted process must still know which request was outstanding"
    );

    finish_request("test.other");
    assert_eq!(
        in_flight_request().as_deref(),
        Some("test.pick"),
        "one launcher resolving must not clear a different launcher's record"
    );

    finish_request("test.pick");
    assert_eq!(
        in_flight_request(),
        None,
        "a resolved request is not still in flight"
    );

    crate::preferences::clear_platform_preferences();
}
