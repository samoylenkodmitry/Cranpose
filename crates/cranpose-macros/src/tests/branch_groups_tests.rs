use quote::{ToTokens, quote};

use super::*;

fn nested_fn_tokens(stmt: &Stmt) -> Option<String> {
    match stmt {
        Stmt::Item(syn::Item::Fn(item)) => Some(item.to_token_stream().to_string()),
        _ => None,
    }
}

#[test]
fn a_naked_nested_fn_stays_untouched() {
    let mut block: Block = syn::parse_quote!({
        #[unsafe(naked)]
        unsafe extern "C" fn trampoline() {
            core::arch::naked_asm!("ret");
        }
        #[naked]
        unsafe extern "C" fn older_spelling() {
            core::arch::naked_asm!("ret");
        }
        let _ = trampoline as unsafe extern "C" fn();
    });
    let reference = block.clone();
    let core_path = quote!(::cranpose_core);
    inject_branch_groups(&core_path, &mut block);

    let before: Vec<String> = reference
        .stmts
        .iter()
        .filter_map(nested_fn_tokens)
        .collect();
    let after: Vec<String> = block.stmts.iter().filter_map(nested_fn_tokens).collect();
    assert_eq!(before.len(), 2, "the probe declares both naked spellings");
    assert_eq!(
        before, after,
        "a naked body must stay a single naked_asm! call; instrumentation \
         inside it is a compile error for the user"
    );
    assert!(
        block.stmts.len() > reference.stmts.len(),
        "the sibling statements around the naked items are still instrumented"
    );
}

#[test]
fn branch_keys_do_not_monomorphise_the_once_lock_initializer() {
    let mut block: Block = syn::parse_quote!({
        if flag {
            first();
        } else {
            second();
        }
    });
    let core_path = quote!(::cranpose_core);
    inject_branch_groups(&core_path, &mut block);
    let tokens = block.to_token_stream().to_string();

    assert!(
        tokens.contains("cached_branch_location_key"),
        "branch keys must be latched through the outlined core helper, \
         got: {tokens}"
    );
    assert!(
        !tokens.contains("get_or_init"),
        "no initializer closure may reach the expansion site, got: {tokens}"
    );
}
