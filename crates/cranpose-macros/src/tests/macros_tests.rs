use super::*;

#[test]
fn definition_key_does_not_monomorphise_the_once_lock_initializer() {
    let core_path = quote!(::cranpose_core);
    let ident = Ident::new("__cranpose_caller_key", Span::mixed_site());
    let name = Ident::new("Card", Span::call_site());
    let tokens = definition_key_stmt(&core_path, &ident, &name).to_string();
    if cfg!(feature = "hot-reload") {
        assert!(
            tokens.contains("hot_definition_key") && !tokens.contains("OnceLock"),
            "a development definition key is a constant, got: {tokens}"
        );
        return;
    }

    assert!(
        tokens.contains("cached_composable_definition_key"),
        "the definition key must be latched through the outlined core \
         helper, got: {tokens}"
    );
    assert!(
        !tokens.contains("get_or_init"),
        "no initializer closure may reach the expansion site, got: {tokens}"
    );
}
