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
        block
            .stmts
            .insert(0, syn::parse_quote! { let #alias = __composer; });
    }
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
        let guard = self.branch_guard_stmt(block.brace_token.span.join());
        block.stmts.insert(0, guard);
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

    fn visit_nested_fn(
        &mut self,
        signature: &syn::Signature,
        attrs: &[syn::Attribute],
        block: &mut Block,
    ) {
        let runs_during_composition =
            signature.constness.is_none() && signature.asyncness.is_none();
        let expands_itself = attrs.iter().any(|attr| {
            attr.path()
                .segments
                .last()
                .is_some_and(|segment| segment.ident == "composable")
        });
        if !runs_during_composition || expands_itself {
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
                if closure.asyncness.is_some() {
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
            Expr::Async(_) | Expr::Const(_) => {}
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
                    if let syn::ImplItem::Fn(method) = impl_item {
                        self.visit_nested_fn(&method.sig, &method.attrs, &mut method.block);
                    }
                }
            }
            syn::Item::Trait(item_trait) => {
                for trait_item in &mut item_trait.items {
                    if let syn::TraitItem::Fn(method) = trait_item
                        && let Some(default_body) = &mut method.default
                    {
                        self.visit_nested_fn(&method.sig, &method.attrs, default_body);
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
    }
    let mut scan = LetScan { found: false };
    scan.visit_expr(expr);
    scan.found
}
