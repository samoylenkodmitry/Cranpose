//! The value that `#[composable]` puts at the top of each composable body.
//! `tr!` reads it, so `tr!` compiles only where a composer runs the code: in
//! a composable body, or in a closure written inside one. Elsewhere the build
//! stops with "cannot find value `tr_runs_only_in_composables`". Code outside
//! composition uses `message!`, which the screen translates when it shows it.
//!
//! `tr!` reads the value with `let _`, which names it without a capture, so a
//! content closure that uses `tr!` stays `'static`.

pub(crate) fn declared(body: &syn::Block) -> Box<syn::Block> {
    let mut body = body.clone();
    body.stmts.insert(
        0,
        syn::parse_quote! {
            #[allow(unused_variables, reason = "tr! reads it to check that it runs in a composable")]
            let tr_runs_only_in_composables = ::core::marker::PhantomData::<()>;
        },
    );
    Box::new(body)
}

#[cfg(feature = "localization")]
pub(crate) fn check() -> proc_macro2::TokenStream {
    quote::quote! {
        let _ = tr_runs_only_in_composables;
    }
}
