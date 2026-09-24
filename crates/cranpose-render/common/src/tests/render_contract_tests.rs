use std::collections::HashSet;

use super::*;

#[test]
fn shared_render_cases_have_unique_names() {
    let names: HashSet<_> = ALL_SHARED_RENDER_CASES
        .into_iter()
        .map(SharedRenderCase::name)
        .collect();
    assert_eq!(names.len(), ALL_SHARED_RENDER_CASES.len());
}

#[test]
fn shared_render_cases_build_non_empty_graphs() {
    for case in ALL_SHARED_RENDER_CASES {
        for fixture in case.fixtures() {
            assert!(fixture.width > 0);
            assert!(fixture.height > 0);
            assert!(
                !fixture.graph.root.children.is_empty(),
                "shared render case {} should emit at least one render node",
                case.name()
            );
        }
    }
}
