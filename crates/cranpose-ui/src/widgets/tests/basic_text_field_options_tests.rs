use super::*;

#[test]
fn default_options_use_default_line_limits() {
    let options = BasicTextFieldOptions::default();
    assert_eq!(options.line_limits, TextFieldLineLimits::default());
}

#[test]
fn decoration_scope_invokes_the_inner_field() {
    let scope = BasicTextFieldDecorationScope {
        inner: Rc::new(|| 73),
    };
    assert_eq!(scope.inner_text_field(), 73);
}
