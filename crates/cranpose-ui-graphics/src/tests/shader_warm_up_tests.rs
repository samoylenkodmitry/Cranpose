use super::{request_shader_warm_ups, shader_warm_ups_after};
use crate::{RuntimeShader, ShaderTarget, ShaderWarmUp};

fn warm_up(tag: &str, target: ShaderTarget) -> ShaderWarmUp {
    ShaderWarmUp {
        shader: RuntimeShader::new(&format!("// shader_warm_up_tests {tag}")),
        target,
    }
}

fn position(requests: &[ShaderWarmUp], warm_up: &ShaderWarmUp) -> Option<usize> {
    requests.iter().position(|requested| requested == warm_up)
}

#[test]
fn requests_are_kept_once_each_in_request_order() {
    let first = warm_up("order-first", ShaderTarget::Page);
    let second = warm_up("order-second", ShaderTarget::Layer);
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
fn a_target_is_part_of_what_is_warmed() {
    let page = warm_up("target", ShaderTarget::Page);
    let layer = warm_up("target", ShaderTarget::Layer);
    request_shader_warm_ups([page.clone(), layer.clone()]);

    let requests = shader_warm_ups_after(0);
    assert!(position(&requests, &page).is_some());
    assert!(position(&requests, &layer).is_some());
}

#[test]
fn a_renderer_reads_only_the_requests_it_has_not_queued() {
    let early = warm_up("after-early", ShaderTarget::Page);
    request_shader_warm_ups([early.clone()]);
    let seen = shader_warm_ups_after(0).len();
    let late = warm_up("after-late", ShaderTarget::Page);
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
