use super::{__source_scope, current_source_trace};

#[test]
fn nested_origins_restore_after_return_and_unwind() {
    assert!(current_source_trace().is_empty());
    let outer = __source_scope("Screen", "screen.rs", 12, "/project");
    assert_eq!(current_source_trace()[0].name, "Screen");
    let captured = {
        let _inner = __source_scope("Card", "card.rs", 8, "/project");
        current_source_trace()
    };
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[1].file, "card.rs");
    assert_eq!(current_source_trace().len(), 1);
    let result = std::panic::catch_unwind(|| {
        let _inner = __source_scope("Panic", "panic.rs", 1, "/project");
        panic!("test unwind");
    });
    assert!(result.is_err());
    assert_eq!(current_source_trace().len(), 1);
    drop(outer);
    assert!(current_source_trace().is_empty());
    assert_eq!(captured[0].line, 12);
}
