use quote::{ToTokens, quote};

use super::*;

fn kept(mut block: syn::Block) -> String {
    keep_constant_modifiers(&quote!(::cranpose_core), &mut block);
    block.to_token_stream().to_string()
}

#[test]
fn a_chain_of_constants_is_kept_once() {
    let tokens = kept(syn::parse_quote!({
        Box(
            Modifier::empty()
                .fill_max_width()
                .height(16.0)
                .background(Color::from_rgb_u8(40, 44, 64))
                .padding(-2.0)
                .align(Alignment::CENTER)
                .rounded_corners(8.0),
            BoxSpec::default(),
            || {},
        );
    }));
    assert_eq!(
        tokens.matches("__constant").count(),
        1,
        "the whole chain is one kept value: {tokens}"
    );
}

#[test]
fn a_chain_that_reads_anything_else_is_built_each_time() {
    for block in [
        syn::parse_quote!({
            Box(Modifier::empty().height(height), BoxSpec::default(), || {});
        }),
        syn::parse_quote!({
            Box(
                Modifier::empty().height(self.height),
                BoxSpec::default(),
                || {},
            );
        }),
        syn::parse_quote!({
            Box(
                Modifier::empty().height(dp(16.0)),
                BoxSpec::default(),
                || {},
            );
        }),
        syn::parse_quote!({
            Box(
                Modifier::empty().padding_relative(1.0, 2.0, 3.0, 4.0),
                BoxSpec::default(),
                || {},
            );
        }),
        syn::parse_quote!({
            Box(
                Modifier::empty().clickable(|| {}),
                BoxSpec::default(),
                || {},
            );
        }),
        syn::parse_quote!({
            Box(base.height(16.0), BoxSpec::default(), || {});
        }),
        syn::parse_quote!({
            Box(Modifier::empty(), BoxSpec::default(), || {});
        }),
    ] {
        let tokens = kept(block);
        assert!(
            !tokens.contains("__constant"),
            "a variable, a call, an impure builder or no builder at all keeps \
             nothing: {tokens}"
        );
    }
}

#[test]
fn chains_inside_closures_are_left_alone() {
    let tokens = kept(syn::parse_quote!({
        Box(Modifier::empty(), BoxSpec::default(), move || {
            Box(Modifier::empty().height(16.0), BoxSpec::default(), || {});
        });
        let make = || Modifier::empty().height(4.0);
    }));
    assert!(
        !tokens.contains("__constant"),
        "a closure may run outside the composition or inside a slot's \
         initializer: {tokens}"
    );
}

#[test]
fn each_kept_chain_takes_its_own_key() {
    let tokens = kept(syn::parse_quote!({
        let first = Modifier::empty().height(1.0);
        let second = Modifier::empty().height(2.0);
    }));
    assert!(
        tokens.contains("column ! () , 0u32") && tokens.contains("column ! () , 1u32"),
        "two kept chains at one source position get two keys: {tokens}"
    );
}
