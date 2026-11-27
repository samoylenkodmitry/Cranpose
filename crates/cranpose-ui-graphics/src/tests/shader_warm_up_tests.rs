use super::{request_shader_warm_ups, shader_warm_ups_after};
use crate::RuntimeShader;

fn warm_up(tag: &str) -> RuntimeShader {
    RuntimeShader::new(&format!("// shader_warm_up_tests {tag}"))
}

fn position(requests: &[RuntimeShader], warm_up: &RuntimeShader) -> Option<usize> {
    requests.iter().position(|requested| requested == warm_up)
}

#[test]
fn requests_are_kept_once_each_in_request_order() {
    let first = warm_up("order-first");
    let second = warm_up("order-second");
    request_shader_warm_ups([first.clone(), second.clone(), first.clone()]);
    request_shader_warm_ups([second.clone()]);

    let requests = shader_warm_ups_after(0);
    assert_eq!(
        requests
            .iter()
            .filter(|requested| **requested == first)
            .count(),
        1,
        "a warm-up requested twice compiles once"
    );
    assert_eq!(
        requests
            .iter()
            .filter(|requested| **requested == second)
            .count(),
        1
    );
    assert!(position(&requests, &first) < position(&requests, &second));
}

#[test]
fn a_renderer_reads_only_the_requests_it_has_not_queued() {
    let early = warm_up("after-early");
    request_shader_warm_ups([early.clone()]);
    let seen = shader_warm_ups_after(0).len();
    let late = warm_up("after-late");
    request_shader_warm_ups([late.clone()]);

    let fresh = shader_warm_ups_after(seen);
    assert!(position(&fresh, &late).is_some());
    assert!(
        position(&fresh, &early).is_none(),
        "a request already queued is not handed out again"
    );
    let everything = shader_warm_ups_after(0).len();
    assert!(shader_warm_ups_after(everything).is_empty());
}
