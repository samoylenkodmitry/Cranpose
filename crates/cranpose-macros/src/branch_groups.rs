use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::{
    Block, Expr, Stmt,
    spanned::Spanned,
    visit::Visit,
    visit_mut::{self, VisitMut},
};

pub(crate) fn inject_branch_groups(core_path: &TokenStream2, block: &mut Block) {
    let mut injector = BranchGroupInjector {
        core_path,
        next_branch: 0,
        in_content_closure: false,
        uses_composer_alias: false,
        branch_depth: 0,
    };
    injector.visit_block_mut(block);
    if injector.uses_composer_alias {
        let alias = composer_alias_ident();
        let composer = syn::Ident::new("__composer", Span::mixed_site());
        block
            .stmts
            .insert(0, syn::parse_quote! { let #alias = #composer; });
    }
}

struct SuspendingChildren<'a, 'b> {
    injector: &'a mut BranchGroupInjector<'b>,
}

impl SuspendingChildren<'_, '_> {
    fn visit_expr_children(&mut self, expr: &mut Expr) {
        visit_mut::visit_expr_mut(self, expr);
    }
}

impl VisitMut for SuspendingChildren<'_, '_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        self.injector.instrument_suspending_child(expr);
    }

    fn visit_item_mut(&mut self, item: &mut syn::Item) {
        self.injector.visit_item_mut(item);
    }

    fn visit_type_mut(&mut self, _ty: &mut syn::Type) {}

    fn visit_angle_bracketed_generic_arguments_mut(
        &mut self,
        _args: &mut syn::AngleBracketedGenericArguments,
    ) {
    }
}

struct SyncInteriors<'a, 'b> {
    injector: &'a mut BranchGroupInjector<'b>,
}

impl VisitMut for SyncInteriors<'_, '_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) => {
                self.injector.visit_expr_mut(expr)
            }
            _ => visit_mut::visit_expr_mut(self, expr),
        }
    }

    fn visit_item_mut(&mut self, item: &mut syn::Item) {
        self.injector.visit_item_mut(item);
    }
}

fn wants_sandwich(stmt: &Stmt, is_tail: bool) -> bool {
    match stmt {
        Stmt::Local(local) => local.init.is_some(),
        Stmt::Macro(invocation) => invocation.semi_token.is_some() || !is_tail,
        _ => false,
    }
}

fn stmt_suspends(stmt: &Stmt) -> bool {
    let mut scan = AwaitScan { found: false };
    scan.visit_stmt(stmt);
    scan.found
}

fn is_naked_attr(attr: &syn::Attribute) -> bool {
    if attr.path().is_ident("naked") {
        return true;
    }
    if let syn::Meta::List(list) = &attr.meta
        && list.path.is_ident("unsafe")
    {
        return list.tokens.clone().into_iter().any(
            |token| matches!(&token, proc_macro2::TokenTree::Ident(ident) if ident == "naked"),
        );
    }
    false
}

fn composer_alias_ident() -> syn::Ident {
    syn::Ident::new("__cranpose_branch_composer", Span::mixed_site())
}

struct BranchGroupInjector<'a> {
    core_path: &'a TokenStream2,
    next_branch: u32,
    in_content_closure: bool,
    uses_composer_alias: bool,
    branch_depth: u32,
}

impl BranchGroupInjector<'_> {
    fn wrap_block(&mut self, block: &mut Block) {
        self.branch_depth += 1;
        for stmt in &mut block.stmts {
            self.visit_stmt_mut(stmt);
        }
        self.branch_depth -= 1;
        self.fold_local_statements(block);
        let guard = self.branch_guard_stmt(block.brace_token.span.join());
        block.stmts.insert(0, guard);
    }

    fn fold_local_statements(&mut self, block: &mut Block) {
        let count = block.stmts.len();
        if !block
            .stmts
            .iter()
            .enumerate()
            .any(|(index, stmt)| wants_sandwich(stmt, index + 1 == count))
        {
            return;
        }
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        let mut rebuilt = Vec::with_capacity(block.stmts.len());
        for (index, stmt) in block.stmts.drain(..).enumerate() {
            if wants_sandwich(&stmt, index + 1 == count) {
                rebuilt.push(self.branch_guard_stmt(stmt.span()));
                rebuilt.push(stmt);
                rebuilt.push(syn::parse_quote! { drop(#guard); });
            } else {
                rebuilt.push(stmt);
            }
        }
        block.stmts = rebuilt;
    }

    fn wrap_arm_body(&mut self, body: &mut Expr) {
        if let Expr::Block(block_expr) = body {
            self.wrap_block(&mut block_expr.block);
            return;
        }
        self.branch_depth += 1;
        self.visit_expr_mut(body);
        self.branch_depth -= 1;
        let guard = self.branch_guard_stmt(body.span());
        let original = body.clone();
        *body = syn::parse_quote! {{
            #guard
            #original
        }};
    }

    fn wrap_condition(&mut self, condition: &mut Expr) {
        self.wrap_condition_inner(condition);
    }

    fn wrap_condition_inner(&mut self, condition: &mut Expr) {
        match condition {
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.wrap_condition_inner(&mut binary.left);
                self.wrap_condition_inner(&mut binary.right);
            }
            Expr::Paren(paren) => self.wrap_condition_inner(&mut paren.expr),
            Expr::Let(let_expr) => {
                let scrutinee = &mut let_expr.expr;
                if expr_is_place(scrutinee) {
                    self.wrap_place_value_parts(scrutinee);
                } else {
                    self.visit_expr_mut(scrutinee);
                }
            }
            leaf => {
                let needs_group = !expr_contains_let(leaf);
                self.visit_expr_mut(leaf);
                if !needs_group {
                    return;
                }
                let guard_stmt = self.branch_guard_stmt(leaf.span());
                let original = leaf.clone();
                *leaf = syn::parse_quote! {{
                    #guard_stmt
                    #original
                }};
            }
        }
    }

    fn branch_guard_stmt(&mut self, span: Span) -> Stmt {
        let branch = self.next_branch;
        self.next_branch += 1;
        let core_path = self.core_path;
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        let key = syn::Ident::new("__CRANPOSE_BRANCH_KEY", Span::mixed_site());
        let cached_key = quote::quote! {{
            static #key: ::std::sync::OnceLock<#core_path::Key> = ::std::sync::OnceLock::new();
            *#key.get_or_init(|| {
                #core_path::branch_location_key(file!(), line!(), column!(), #branch)
            })
        }};
        if self.in_content_closure {
            syn::parse_quote_spanned! {span=>
                let #guard = #core_path::__branch_group_scope_deferred(#cached_key);
            }
        } else {
            self.uses_composer_alias = true;
            let composer = composer_alias_ident();
            syn::parse_quote_spanned! {span=>
                let #guard = #composer.__branch_group_deferred(#cached_key);
            }
        }
    }

    fn wrap_place_value_parts(&mut self, place: &mut Expr) {
        match place {
            Expr::Index(index) => {
                self.wrap_place_or_value(&mut index.expr);
                self.wrap_value_part(&mut index.index);
            }
            Expr::Field(field) => self.wrap_place_or_value(&mut field.base),
            Expr::Paren(paren) => self.wrap_place_value_parts(&mut paren.expr),
            Expr::Group(group) => self.wrap_place_value_parts(&mut group.expr),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.wrap_place_or_value(&mut unary.expr);
            }
            _ => {}
        }
    }

    fn wrap_place_or_value(&mut self, expr: &mut Expr) {
        if expr_is_place(expr) {
            self.wrap_place_value_parts(expr);
        } else {
            self.wrap_value_part(expr);
        }
    }

    fn wrap_value_part(&mut self, expr: &mut Expr) {
        self.visit_expr_mut(expr);
        let guard_stmt = self.branch_guard_stmt(expr.span());
        let original = expr.clone();
        *expr = syn::parse_quote! {{
            #guard_stmt
            #original
        }};
    }

    fn instrument_sync_interiors(&mut self, expr: &mut Expr) {
        SyncInteriors { injector: self }.visit_expr_mut(expr);
    }

    fn instrument_block_by_suspension(&mut self, block: &mut Block) {
        if block_contains_await(block) {
            self.instrument_nonsuspending_statements(block);
        } else {
            let previous = std::mem::replace(&mut self.in_content_closure, true);
            self.wrap_block(block);
            self.in_content_closure = previous;
        }
    }

    fn instrument_suspending_condition(&mut self, condition: &mut Expr) {
        if !expr_contains_await(condition) {
            self.wrap_condition(condition);
            return;
        }
        match condition {
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.instrument_suspending_condition(&mut binary.left);
                self.instrument_suspending_condition(&mut binary.right);
            }
            Expr::Paren(paren) => self.instrument_suspending_condition(&mut paren.expr),
            leaf => self.instrument_suspending_expr(leaf),
        }
    }

    fn suspending_place_value_parts(&mut self, place: &mut Expr) {
        match place {
            Expr::Index(index) => {
                self.suspending_place_or_value(&mut index.expr);
                self.suspending_value_part(&mut index.index);
            }
            Expr::Field(field) => self.suspending_place_or_value(&mut field.base),
            Expr::Paren(paren) => self.suspending_place_value_parts(&mut paren.expr),
            Expr::Group(group) => self.suspending_place_value_parts(&mut group.expr),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.suspending_place_or_value(&mut unary.expr);
            }
            _ => {}
        }
    }

    fn suspending_place_or_value(&mut self, expr: &mut Expr) {
        if expr_is_place(expr) {
            self.suspending_place_value_parts(expr);
        } else {
            self.suspending_value_part(expr);
        }
    }

    fn suspending_value_part(&mut self, expr: &mut Expr) {
        if expr_contains_await(expr) {
            self.instrument_suspending_expr(expr);
        } else {
            self.wrap_value_part(expr);
        }
    }

    fn instrument_suspending_child(&mut self, expr: &mut Expr) {
        if expr_contains_await(expr) {
            self.instrument_suspending_expr(expr);
        } else {
            self.visit_expr_mut(expr);
        }
    }

    fn instrument_suspending_expr(&mut self, expr: &mut Expr) {
        match expr {
            Expr::If(expr_if) => {
                self.instrument_suspending_condition(&mut expr_if.cond);
                self.instrument_block_by_suspension(&mut expr_if.then_branch);
                if let Some((_, else_expr)) = &mut expr_if.else_branch {
                    match else_expr.as_mut() {
                        Expr::Block(block_expr) => {
                            self.instrument_block_by_suspension(&mut block_expr.block);
                        }
                        other => self.instrument_suspending_expr(other),
                    }
                }
            }
            Expr::Match(expr_match) => {
                self.instrument_suspending_child(&mut expr_match.expr);
                for arm in &mut expr_match.arms {
                    if let Some((_, guard)) = &mut arm.guard {
                        self.instrument_suspending_condition(guard);
                    }
                    if expr_contains_await(&arm.body) {
                        self.instrument_suspending_expr(&mut arm.body);
                    } else {
                        self.wrap_arm_body(&mut arm.body);
                    }
                }
            }
            Expr::While(while_loop) => {
                self.instrument_suspending_condition(&mut while_loop.cond);
                self.instrument_block_by_suspension(&mut while_loop.body);
            }
            Expr::ForLoop(for_loop) => {
                self.instrument_suspending_child(&mut for_loop.expr);
                self.instrument_block_by_suspension(&mut for_loop.body);
            }
            Expr::Loop(loop_expr) => self.instrument_block_by_suspension(&mut loop_expr.body),
            Expr::Block(block_expr) => {
                self.instrument_block_by_suspension(&mut block_expr.block);
            }
            Expr::Unsafe(inner) => {
                self.instrument_block_by_suspension(&mut inner.block);
            }
            Expr::Paren(paren) => self.instrument_suspending_expr(&mut paren.expr),
            Expr::Group(group) => self.instrument_suspending_expr(&mut group.expr),
            Expr::Await(await_expr) => self.instrument_suspending_child(&mut await_expr.base),
            Expr::Let(let_expr) => self.instrument_suspending_child(&mut let_expr.expr),
            Expr::Assign(assign) => {
                self.instrument_suspending_child(&mut assign.right);
                if expr_contains_await(&assign.left) {
                    self.suspending_place_value_parts(&mut assign.left);
                } else {
                    self.wrap_place_value_parts(&mut assign.left);
                }
            }
            Expr::Return(expr_return) => {
                if let Some(value) = &mut expr_return.expr {
                    self.instrument_suspending_child(value);
                }
            }
            Expr::Break(expr_break) => {
                if let Some(value) = &mut expr_break.expr {
                    self.instrument_suspending_child(value);
                }
            }
            other => SuspendingChildren { injector: self }.visit_expr_children(other),
        }
    }

    fn instrument_nonsuspending_statements(&mut self, block: &mut Block) {
        let previous = std::mem::replace(&mut self.in_content_closure, true);
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        let count = block.stmts.len();
        let mut rebuilt = Vec::with_capacity(block.stmts.len());
        for (index, mut stmt) in block.stmts.drain(..).enumerate() {
            if stmt_suspends(&stmt) {
                match &mut stmt {
                    Stmt::Expr(expr, _) => self.instrument_suspending_expr(expr),
                    Stmt::Local(local) => {
                        if let Some(init) = &mut local.init {
                            self.instrument_suspending_expr(&mut init.expr);
                            if let Some((_, diverge)) = &mut init.diverge {
                                if let Expr::Block(block_expr) = diverge.as_mut() {
                                    self.instrument_block_by_suspension(&mut block_expr.block);
                                } else {
                                    self.instrument_suspending_expr(diverge);
                                }
                            }
                        }
                    }
                    _ => {}
                }
                rebuilt.push(stmt);
                continue;
            }
            self.visit_stmt_mut(&mut stmt);
            if wants_sandwich(&stmt, index + 1 == count) {
                rebuilt.push(self.branch_guard_stmt(stmt.span()));
                rebuilt.push(stmt);
                rebuilt.push(syn::parse_quote! { drop(#guard); });
            } else if index + 1 == count && matches!(&stmt, Stmt::Expr(_, None)) {
                rebuilt.push(self.branch_guard_stmt(stmt.span()));
                rebuilt.push(stmt);
            } else {
                rebuilt.push(stmt);
            }
        }
        block.stmts = rebuilt;
        self.in_content_closure = previous;
    }

    fn instrument_sync_interiors_block(&mut self, block: &mut Block) {
        SyncInteriors { injector: self }.visit_block_mut(block);
    }

    fn visit_nested_fn(
        &mut self,
        signature: &syn::Signature,
        attrs: &[syn::Attribute],
        block: &mut Block,
    ) {
        let runs_during_composition =
            signature.constness.is_none() && signature.asyncness.is_none();
        if attrs.iter().any(is_naked_attr) {
            return;
        }
        let expands_itself = attrs.iter().any(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "composable")
        });
        if !runs_during_composition || expands_itself {
            if expands_itself {
                return;
            }
            if signature.asyncness.is_some() {
                self.instrument_block_by_suspension(block);
            } else {
                self.instrument_sync_interiors_block(block);
            }
            return;
        }
        let previous = std::mem::replace(&mut self.in_content_closure, true);
        self.visit_block_mut(block);
        let guard = self.branch_guard_stmt(block.brace_token.span.join());
        block.stmts.insert(0, guard);
        self.in_content_closure = previous;
    }
}

impl VisitMut for BranchGroupInjector<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        let folds_whole_statement = match &*expr {
            Expr::If(expr_if) => expr_contains_let(&expr_if.cond),
            Expr::While(while_loop) => expr_contains_let(&while_loop.cond),
            Expr::ForLoop(_) => true,
            _ => false,
        };
        match expr {
            Expr::Closure(closure) => {
                if closure.asyncness.is_some() && expr_contains_await(&closure.body) {
                    let previous = std::mem::replace(&mut self.in_content_closure, true);
                    self.instrument_suspending_expr(&mut closure.body);
                    self.in_content_closure = previous;
                    return;
                }
                let previous = std::mem::replace(&mut self.in_content_closure, true);
                self.visit_expr_mut(&mut closure.body);
                let guard = self.branch_guard_stmt(closure.span());
                let original = closure.body.clone();
                closure.body = syn::parse_quote! {{
                    #guard
                    #original
                }};
                self.in_content_closure = previous;
            }
            Expr::Async(async_block) => {
                self.instrument_block_by_suspension(&mut async_block.block);
            }
            Expr::Const(const_block) => {
                self.instrument_sync_interiors_block(&mut const_block.block);
            }
            Expr::If(expr_if) => {
                self.wrap_condition(&mut expr_if.cond);
                self.wrap_block(&mut expr_if.then_branch);
                if let Some((_, else_expr)) = &mut expr_if.else_branch {
                    match else_expr.as_mut() {
                        Expr::If(_) => self.visit_expr_mut(else_expr),
                        Expr::Block(block_expr) => self.wrap_block(&mut block_expr.block),
                        other => self.visit_expr_mut(other),
                    }
                }
            }
            Expr::Match(expr_match) => {
                self.visit_expr_mut(&mut expr_match.expr);
                for arm in &mut expr_match.arms {
                    if let Some((_, guard)) = &mut arm.guard {
                        self.wrap_condition(guard);
                    }
                    self.wrap_arm_body(&mut arm.body);
                }
            }
            Expr::ForLoop(for_loop) => {
                self.visit_expr_mut(&mut for_loop.expr);
                self.wrap_block(&mut for_loop.body);
            }
            Expr::While(while_loop) => {
                self.wrap_condition(&mut while_loop.cond);
                self.wrap_block(&mut while_loop.body);
            }
            Expr::Loop(loop_expr) => {
                self.wrap_block(&mut loop_expr.body);
            }
            Expr::Repeat(repeat) => self.visit_expr_mut(&mut repeat.expr),
            _ => visit_mut::visit_expr_mut(self, expr),
        }
        if folds_whole_statement {
            let guard = self.branch_guard_stmt(expr.span());
            let original = expr.clone();
            *expr = syn::parse_quote! {{
                #guard
                #original
            }};
        }
    }

    fn visit_block_mut(&mut self, block: &mut Block) {
        for stmt in &mut block.stmts {
            self.visit_stmt_mut(stmt);
        }
        self.fold_local_statements(block);
    }

    fn visit_stmt_mut(&mut self, stmt: &mut Stmt) {
        visit_mut::visit_stmt_mut(self, stmt);
        if let Stmt::Expr(expr, Some(semi)) = stmt {
            let guard = self.branch_guard_stmt(expr.span());
            let original = expr.clone();
            let semi = *semi;
            *stmt = Stmt::Expr(
                syn::parse_quote! {{
                    #guard
                    #original #semi
                }},
                None,
            );
        }
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        let Some(init) = &mut local.init else {
            return;
        };
        self.visit_expr_mut(&mut init.expr);
        if let Some((_, diverge)) = &mut init.diverge {
            if let Expr::Block(block_expr) = diverge.as_mut() {
                self.wrap_block(&mut block_expr.block);
            } else {
                self.visit_expr_mut(diverge);
            }
        }
    }

    fn visit_item_mut(&mut self, item: &mut syn::Item) {
        match item {
            syn::Item::Fn(item_fn) => {
                self.visit_nested_fn(&item_fn.sig, &item_fn.attrs, &mut item_fn.block);
            }
            syn::Item::Impl(item_impl) => {
                for impl_item in &mut item_impl.items {
                    match impl_item {
                        syn::ImplItem::Fn(method) => {
                            self.visit_nested_fn(&method.sig, &method.attrs, &mut method.block);
                        }
                        syn::ImplItem::Const(assoc_const) => {
                            self.instrument_sync_interiors(&mut assoc_const.expr);
                        }
                        _ => {}
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &mut item_trait.items {
                    match trait_item {
                        syn::TraitItem::Fn(method) => {
                            if let Some(default_body) = &mut method.default {
                                self.visit_nested_fn(&method.sig, &method.attrs, default_body);
                            }
                        }
                        syn::TraitItem::Const(assoc_const) => {
                            if let Some((_, default)) = &mut assoc_const.default {
                                self.instrument_sync_interiors(default);
                            }
                        }
                        _ => {}
                    }
                }
            }
            syn::Item::Mod(item_mod) => {
                if let Some((_, items)) = &mut item_mod.content {
                    for nested in items {
                        self.visit_item_mut(nested);
                    }
                }
            }
            syn::Item::Static(item_static) => {
                self.instrument_sync_interiors(&mut item_static.expr);
            }
            syn::Item::Const(item_const) => {
                self.instrument_sync_interiors(&mut item_const.expr);
            }
            _ => {}
        }
    }

    fn visit_type_mut(&mut self, _ty: &mut syn::Type) {}

    fn visit_angle_bracketed_generic_arguments_mut(
        &mut self,
        _args: &mut syn::AngleBracketedGenericArguments,
    ) {
    }
}

fn expr_is_place(expr: &Expr) -> bool {
    match expr {
        Expr::Path(_) | Expr::Field(_) | Expr::Index(_) => true,
        Expr::Unary(unary) => matches!(unary.op, syn::UnOp::Deref(_)),
        Expr::Paren(paren) => expr_is_place(&paren.expr),
        Expr::Group(group) => expr_is_place(&group.expr),
        _ => false,
    }
}

struct AwaitScan {
    found: bool,
}

impl<'ast> Visit<'ast> for AwaitScan {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.found {
            return;
        }
        match expr {
            Expr::Await(_) => self.found = true,
            Expr::Async(_) => {}
            Expr::Closure(closure) if closure.asyncness.is_some() => {}
            _ => syn::visit::visit_expr(self, expr),
        }
    }

    fn visit_macro(&mut self, _mac: &'ast syn::Macro) {
        self.found = true;
    }

    fn visit_item(&mut self, _item: &'ast syn::Item) {}
}

fn block_contains_await(block: &Block) -> bool {
    let mut scan = AwaitScan { found: false };
    scan.visit_block(block);
    scan.found
}

fn expr_contains_await(expr: &Expr) -> bool {
    let mut scan = AwaitScan { found: false };
    scan.visit_expr(expr);
    scan.found
}

fn expr_contains_let(expr: &Expr) -> bool {
    struct LetScan {
        found: bool,
    }
    impl<'ast> Visit<'ast> for LetScan {
        fn visit_expr(&mut self, expr: &'ast Expr) {
            if self.found {
                return;
            }
            match expr {
                Expr::Let(_) => self.found = true,
                Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) => {}
                _ => syn::visit::visit_expr(self, expr),
            }
        }

        fn visit_item(&mut self, _item: &'ast syn::Item) {}
    }
    let mut scan = LetScan { found: false };
    scan.visit_expr(expr);
    scan.found
}

#[cfg(test)]
mod tests {
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
}
