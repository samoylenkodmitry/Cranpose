use super::*;

struct Numbers {
    content: ScopeContent<u32>,
}

impl Numbers {
    fn number(&self, value: u32) {
        self.content.push(value);
    }
}

#[test]
fn a_scope_keeps_the_order_its_content_declared() {
    let collected = ScopeContent::collect(
        |content| Numbers { content },
        |scope| {
            scope.number(3);
            scope.number(1);
            scope.number(2);
        },
    );
    assert_eq!(collected, vec![3, 1, 2]);
}

#[test]
fn content_that_declares_nothing_collects_nothing() {
    let collected = ScopeContent::collect(|content| Numbers { content }, |_| {});
    assert!(collected.is_empty());
}

#[test]
fn each_collection_starts_from_an_empty_list() {
    let build = |content| Numbers { content };
    let first = ScopeContent::collect(build, |scope| scope.number(7));
    let second = ScopeContent::collect(build, |scope| scope.number(8));
    assert_eq!(first, vec![7]);
    assert_eq!(second, vec![8]);
}
