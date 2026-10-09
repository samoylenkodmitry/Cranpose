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
fn branch_keys_are_constants_of_their_guard_site() {
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
        tokens.contains("const __CRANPOSE_BRANCH_KEY"),
        "a branch key must be a constant of its guard site, got: {tokens}"
    );
    assert!(
        !tokens.contains("OnceLock"),
        "a guard must not check a lazily initialized key, got: {tokens}"
    );
}

fn paths(mut block: Block) -> Vec<String> {
    hot_guard_paths(&mut block)
}

#[test]
fn hot_paths_survive_structural_edits_elsewhere() {
    let before = paths(syn::parse_quote!({
        let count = remember(|| 0);
        Text("title");
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Text("body");
            if show_details {
                Counter();
            }
            for item in items {
                Row(|| Text(item));
            }
        });
        Button(move || count.set(1));
    }));
    let after = paths(syn::parse_quote!({
        Text("added before everything");
        let count = remember(|| 0);
        let extra = remember(|| String::new());
        Text("title");
        Spacer(12.0);
        Column(Modifier::empty(), ColumnSpec::default(), move || {
            Divider();
            Text("body");
            if loading {
                Spinner();
            }
            if show_details {
                Counter();
                Text("more");
            }
            for item in items {
                Row(|| Text(item));
            }
        });
        Button(move || count.set(1));
        Button(move || extra.set("x".into()));
    }));
    for kept in [
        "Card/let:count#0",
        "Card/let:count#0/closure#0",
        "Card/stmt:Column#0/closure#0",
        "Card/stmt:Column#0/closure#0/stmt:Text#0",
        "Card/stmt:Column#0/closure#0/then:show_details#0/stmt:Counter#0",
        "Card/stmt:Column#0/closure#0/fold:item#0/for:item#0/stmt:Row#0/closure#0",
        "Card/stmt:Button#0/closure#0",
    ] {
        assert!(
            before.iter().any(|p| p == kept),
            "missing {kept} in {before:#?}"
        );
        assert!(
            after.iter().any(|p| p == kept),
            "edit renamed {kept}: {after:#?}"
        );
    }
}

#[test]
fn hot_paths_are_unique_and_distinguish_branches() {
    let all = paths(syn::parse_quote!({
        if a {
            Text("a");
        } else {
            Text("not a");
        }
        if a {
            Text("again");
        }
        match mode {
            Mode::One => Text("one"),
            Mode::Two => Text("two"),
        }
        Text("x");
        Text("y");
    }));
    let unique: std::collections::HashSet<_> = all.iter().collect();
    assert_eq!(unique.len(), all.len(), "duplicate paths: {all:#?}");
    for expected in [
        "Card/then:a#0",
        "Card/else:a#0",
        "Card/then:a#1",
        "Card/arm:Mode::One#0",
        "Card/arm:Mode::Two#0",
        "Card/stmt:Text#0",
        "Card/stmt:Text#1",
    ] {
        assert!(
            all.iter().any(|p| p == expected),
            "missing {expected} in {all:#?}"
        );
    }
}

#[test]
fn release_expansion_does_not_use_hot_keys() {
    let mut block: Block = syn::parse_quote!({
        let count = remember(|| 0);
        if show {
            Text("a");
        }
    });
    inject_branch_groups(&quote!(::cranpose_core), &mut block);
    let tokens = block.to_token_stream().to_string();
    assert!(tokens.contains("branch_location_key"));
    assert!(!tokens.contains("hot_branch_key"));
}
