use cranpose::{exit_requested, request_exit, take_exit_request};

use super::*;

#[test]
fn an_exit_request_ends_the_session_once_and_stays_until_the_host_closed() {
    let _ = take_exit_request();
    let mut exit = ExitRequest::default();
    assert_eq!(exit.poll(true), ExitStep::Wait, "the app asked for nothing");
    assert!(!leave(|| panic!("no request, so nothing to close")));

    request_exit();
    assert_eq!(exit.poll(true), ExitStep::EndSession);
    assert_eq!(exit.poll(true), ExitStep::Wait, "the runtime is asked once");
    // The runtime ended the session: the loop leaves
    assert_eq!(exit.poll(false), ExitStep::Leave);

    assert!(!leave(|| false));
    assert!(exit_requested(), "a close that failed keeps the request");
    assert!(leave(|| true));
    assert!(!exit_requested());

    // A request with no session ends the app at once
    request_exit();
    assert_eq!(ExitRequest::default().poll(false), ExitStep::Leave);
    let _ = take_exit_request();
}
