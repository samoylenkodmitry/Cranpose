use quote::quote;

use crate::preview::expand;

#[test]
fn preview_rejects_invalid_signatures_and_options() {
    for item in [
        quote!(
            fn Bad(value: u32) {}
        ),
        quote!(
            async fn Bad() {}
        ),
        quote!(
            fn Bad<T>() {}
        ),
        quote!(
            fn Bad() -> u32 {
                0
            }
        ),
    ] {
        assert!(expand(quote!(), syn::parse2(item).expect("valid syntax")).is_err());
    }
    for args in [
        quote!(width = 0),
        quote!(height = 8193),
        quote!(unknown = true),
    ] {
        assert!(
            expand(
                args,
                syn::parse2(quote!(
                    fn Fine() {}
                ))
                .expect("valid function")
            )
            .is_err()
        );
    }
}

#[test]
fn preview_emits_registration_and_preserves_composable_attribute() {
    let result = expand(
        quote!(name = "Card", width = 320, dark = true),
        syn::parse2(quote!(
            #[composable]
            pub fn Fixture() {}
        ))
        .expect("valid function"),
    )
    .expect("preview expands")
    .to_string();
    assert!(result.contains("__submit"));
    assert!(result.contains("composable"));
    assert!(result.contains("320"));
    assert!(result.contains("Fixture"));
}
