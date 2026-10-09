use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::{
    Block, Expr, Pat, Stmt,
    spanned::Spanned,
    visit::Visit,
    visit_mut::{self, VisitMut},
};

#[cfg(test)]
pub(crate) fn inject_branch_groups(core_path: &TokenStream2, block: &mut Block) {
    let scope = syn::Ident::new("Card", Span::call_site());
    inject_branch_groups_with(core_path, block, &scope, false);
}

/// The development guard paths a body receives, in allocation order.
#[cfg(test)]
pub(crate) fn hot_guard_paths(block: &mut Block) -> Vec<String> {
    let core_path = quote::quote!(::cranpose_core);
    let mut injector = BranchGroupInjector {
        core_path: &core_path,
        next_branch: 0,
        in_content_closure: false,
        uses_composer_alias: false,
        branch_depth: 0,
        hot: HotPaths::new(true, "Card"),
    };
    injector.visit_block_mut(block);
    injector.hot.allocated
}

/// Adds branch groups to a composable body. With `hot` (the development-only
/// `hot-reload` feature) guard keys come from each guard's structural path in
/// `scope` instead of its line, column and position among all guards, so a
/// hot-patched edit keeps the state of unrelated groups. Release keys are
/// unchanged.
pub(crate) fn inject_branch_groups_with(
    core_path: &TokenStream2,
    block: &mut Block,
    scope: &syn::Ident,
    hot: bool,
) {
    let mut injector = BranchGroupInjector {
        core_path,
        next_branch: 0,
        in_content_closure: false,
        uses_composer_alias: false,
        branch_depth: 0,
        hot: HotPaths::new(hot, &scope.to_string()),
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
                self.injector.visit_expr_mut(expr);
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
    hot: HotPaths,
}

/// Structural guard paths for hot-patched development builds.
///
/// Each guard is named by its kind, a source name (a callee, binding or
/// condition) and its ordinal among same-named siblings in the enclosing
/// guard's scope, allocated before its contents. Inserting a statement, call,
/// closure or branch therefore only renames later siblings with the same name,
/// never the enclosing groups or unrelated siblings.
struct HotPaths {
    enabled: bool,
    scopes: Vec<(String, std::collections::HashMap<String, u32>)>,
    #[cfg(test)]
    allocated: Vec<String>,
}

impl HotPaths {
    fn new(enabled: bool, scope: &str) -> Self {
        Self {
            enabled,
            scopes: vec![(scope.to_owned(), Default::default())],
            #[cfg(test)]
            allocated: Vec::new(),
        }
    }

    fn open(&mut self, kind: &str, name: &str) -> Option<String> {
        if !self.enabled {
            return None;
        }
        let label = if name.is_empty() {
            kind.to_owned()
        } else {
            format!("{kind}:{name}")
        };
        let (parent, counts) = self.scopes.last_mut().expect("root hot scope");
        let ordinal = counts.entry(label.clone()).or_default();
        let path = format!("{parent}/{label}#{ordinal}");
        *ordinal += 1;
        self.scopes.push((path.clone(), Default::default()));
        #[cfg(test)]
        self.allocated.push(path.clone());
        Some(path)
    }

    fn close(&mut self, opened: &Option<String>) {
        if opened.is_some() {
            self.scopes.pop();
        }
    }
}

/// Compact, whitespace-free source text used as a stable guard name.
fn source_name(tokens: impl quote::ToTokens) -> String {
    let mut name: String = tokens
        .to_token_stream()
        .to_string()
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    if name.len() > 64 {
        let mut end = 64;
        while !name.is_char_boundary(end) {
            end -= 1;
        }
        name.truncate(end);
    }
    name
}

/// The callee of an expression statement: the call path's last segment, the
/// method name or the macro name.
fn statement_name(expr: &Expr) -> String {
    match expr {
        Expr::Call(call) => match call.func.as_ref() {
            Expr::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default(),
            other => source_name(other),
        },
        Expr::MethodCall(call) => call.method.to_string(),
        Expr::Macro(mac) => source_name(&mac.mac.path),
        Expr::Await(inner) => statement_name(&inner.base),
        Expr::Try(inner) => statement_name(&inner.expr),
        _ => String::new(),
    }
}

fn sandwich_kind(stmt: &Stmt) -> (&'static str, String) {
    match stmt {
        Stmt::Local(local) => ("let", source_name(&local.pat)),
        Stmt::Macro(invocation) => ("macro", source_name(&invocation.mac.path)),
        _ => ("stmt", String::new()),
    }
}

/// Marks `scope`'s definition as the origin for call-site keys in its body.
pub(crate) fn hot_origin_stmt(
    core_path: &TokenStream2,
    scope: &syn::Ident,
    end: Span,
) -> TokenStream2 {
    let origin = syn::Ident::new("__cranpose_hot_origin", Span::mixed_site());
    let identity = syn::Ident::new("__CRANPOSE_HOT_ORIGIN", Span::mixed_site());
    let (line, _) = line_range(scope.span());
    let (_, end) = line_range(end);
    let name = scope.to_string();
    quote::quote! {
        let #origin = {
            const #identity: #core_path::Key =
                #core_path::hot_definition_key(file!(), module_path!(), #name);
            #core_path::hot_origin(file!(), #line, #end, #identity)
        };
    }
}

/// The first and last source lines a span covers. Outside a compiler
/// expansion (unit tests) the range is left open.
fn line_range(span: Span) -> (TokenStream2, TokenStream2) {
    if proc_macro::is_available() {
        let span = span.unwrap();
        let (start, end) = (span.start().line() as u32, span.end().line() as u32);
        (quote::quote!(#start), quote::quote!(#end))
    } else {
        (
            quote::quote_spanned!(span=> line!()),
            quote::quote!(::core::primitive::u32::MAX),
        )
    }
}

/// FNV-1a of a guard path, evaluated while expanding the macro.
fn hot_path_hash(path: &str) -> u64 {
    path.bytes().fold(0xcbf2_9ce4_8422_2325, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01b3)
    })
}

impl BranchGroupInjector<'_> {
    fn wrap_block(&mut self, block: &mut Block, kind: &str, name: &str) {
        let hot = self.hot.open(kind, name);
        self.branch_depth += 1;
        let sandwiches = self.visit_block_statements(block);
        self.branch_depth -= 1;
        self.fold_local_statements(block, sandwiches);
        self.hot.close(&hot);
        let guard = self.branch_guard_stmt(block.brace_token.span.join(), hot);
        block.stmts.insert(0, guard);
    }

    /// Visits statements in order. A statement that will be sandwiched in its
    /// own group gets its development path before its contents are visited.
    fn visit_block_statements(&mut self, block: &mut Block) -> Vec<Option<String>> {
        let count = block.stmts.len();
        let mut sandwiches = Vec::with_capacity(count);
        for (index, stmt) in block.stmts.iter_mut().enumerate() {
            let hot = if wants_sandwich(stmt, index + 1 == count) {
                let (kind, name) = sandwich_kind(stmt);
                self.hot.open(kind, &name)
            } else {
                None
            };
            self.visit_stmt_mut(stmt);
            self.hot.close(&hot);
            sandwiches.push(hot);
        }
        sandwiches
    }

    fn fold_local_statements(&mut self, block: &mut Block, mut sandwiches: Vec<Option<String>>) {
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
        sandwiches.resize(count, None);
        for (index, (stmt, hot)) in block.stmts.drain(..).zip(sandwiches).enumerate() {
            if wants_sandwich(&stmt, index + 1 == count) {
                rebuilt.push(self.branch_guard_stmt(stmt.span(), hot));
                rebuilt.push(stmt);
                rebuilt.push(syn::parse_quote! { drop(#guard); });
            } else {
                rebuilt.push(stmt);
            }
        }
        block.stmts = rebuilt;
    }

    fn wrap_arm_body(&mut self, body: &mut Expr, pattern: &str) {
        if let Expr::Block(block_expr) = body {
            self.wrap_block(&mut block_expr.block, "arm", pattern);
            return;
        }
        let hot = self.hot.open("arm", pattern);
        self.branch_depth += 1;
        self.visit_expr_mut(body);
        self.branch_depth -= 1;
        self.hot.close(&hot);
        let guard = self.branch_guard_stmt(body.span(), hot);
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
                let hot = if needs_group {
                    self.hot.open("cond", &source_name(&*leaf))
                } else {
                    None
                };
                self.visit_expr_mut(leaf);
                self.hot.close(&hot);
                if !needs_group {
                    return;
                }
                let guard_stmt = self.branch_guard_stmt(leaf.span(), hot);
                let original = leaf.clone();
                *leaf = syn::parse_quote! {{
                    #guard_stmt
                    #original
                }};
            }
        }
    }

    fn branch_guard_stmt(&mut self, span: Span, hot: Option<String>) -> Stmt {
        let branch = self.next_branch;
        self.next_branch += 1;
        let core_path = self.core_path;
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        let key = syn::Ident::new("__CRANPOSE_BRANCH_KEY", Span::mixed_site());
        if let Some(path) = hot {
            return self.hot_guard_stmt(span, &path);
        }
        let cached_key = quote::quote! {{
            const #key: #core_path::Key =
                #core_path::branch_location_key(file!(), line!(), column!(), #branch);
            #core_path::noted_location_key(#key, file!(), line!(), column!())
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

    /// A development guard: a constant structural key (no static cache, whose
    /// storage a hot patch would reuse for whichever guard now carries its
    /// name), and a lexical origin so call sites inside the guarded source are
    /// keyed by their line relative to it rather than to the file.
    fn hot_guard_stmt(&mut self, span: Span, path: &str) -> Stmt {
        let core_path = self.core_path;
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        let key = syn::Ident::new("__CRANPOSE_BRANCH_KEY", Span::mixed_site());
        let hash = hot_path_hash(path);
        let (start, end) = line_range(span);
        let group = if self.in_content_closure {
            quote::quote!(#core_path::__branch_group_scope_deferred(#key))
        } else {
            self.uses_composer_alias = true;
            let composer = composer_alias_ident();
            quote::quote!(#composer.__branch_group_deferred(#key))
        };
        syn::parse_quote_spanned! {span=>
            let #guard = {
                const #key: #core_path::Key = #core_path::hot_branch_key(file!(), #hash);
                (#core_path::hot_origin(file!(), #start, #end, #key), #group)
            };
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
        let hot = self.hot.open("value", &source_name(&*expr));
        self.visit_expr_mut(expr);
        self.hot.close(&hot);
        let guard_stmt = self.branch_guard_stmt(expr.span(), hot);
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
            self.wrap_block(block, "block", "");
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
                    if let Pat::Guard(pat_guard) = &mut arm.pat {
                        self.instrument_suspending_condition(&mut pat_guard.guard);
                    }
                    if expr_contains_await(&arm.body) {
                        self.instrument_suspending_expr(&mut arm.body);
                    } else {
                        let pattern = source_name(&arm.pat);
                        self.wrap_arm_body(&mut arm.body, &pattern);
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
            let hot = if wants_sandwich(&stmt, index + 1 == count) {
                let (kind, name) = sandwich_kind(&stmt);
                self.hot.open(kind, &name)
            } else if index + 1 == count
                && let Stmt::Expr(expr, _) = &stmt
            {
                self.hot.open("tail", &statement_name(expr))
            } else {
                None
            };
            self.visit_stmt_mut(&mut stmt);
            self.hot.close(&hot);
            if wants_sandwich(&stmt, index + 1 == count) {
                rebuilt.push(self.branch_guard_stmt(stmt.span(), hot));
                rebuilt.push(stmt);
                rebuilt.push(syn::parse_quote! { drop(#guard); });
            } else if index + 1 == count && matches!(&stmt, Stmt::Expr(_, None)) {
                rebuilt.push(self.branch_guard_stmt(stmt.span(), hot));
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
            let hot = self.hot.open("fn", &signature.ident.to_string());
            if signature.asyncness.is_some() {
                self.instrument_block_by_suspension(block);
            } else {
                self.instrument_sync_interiors_block(block);
            }
            self.hot.close(&hot);
            return;
        }
        let previous = std::mem::replace(&mut self.in_content_closure, true);
        let hot = self.hot.open("fn", &signature.ident.to_string());
        self.visit_block_mut(block);
        self.hot.close(&hot);
        let guard = self.branch_guard_stmt(block.brace_token.span.join(), hot);
        block.stmts.insert(0, guard);
        self.in_content_closure = previous;
    }
}

impl BranchGroupInjector<'_> {
    fn open_fold(&mut self, expr: &Expr, folds_whole_statement: bool) -> Option<String> {
        if folds_whole_statement {
            self.hot.open("fold", &fold_name(expr))
        } else {
            None
        }
    }

    fn wrap_closure(&mut self, closure: &mut syn::ExprClosure) {
        let previous = std::mem::replace(&mut self.in_content_closure, true);
        let hot = self.hot.open("closure", "");
        self.visit_expr_mut(&mut closure.body);
        self.hot.close(&hot);
        let guard = self.branch_guard_stmt(closure.span(), hot);
        let original = closure.body.clone();
        closure.body = syn::parse_quote! {{
            #guard
            #original
        }};
        self.in_content_closure = previous;
    }
}

fn fold_name(expr: &Expr) -> String {
    match expr {
        Expr::If(expr_if) => source_name(&expr_if.cond),
        Expr::While(while_loop) => source_name(&while_loop.cond),
        Expr::ForLoop(for_loop) => source_name(&for_loop.pat),
        _ => String::new(),
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
        let fold = self.open_fold(expr, folds_whole_statement);
        match expr {
            Expr::Closure(closure) => {
                if closure.asyncness.is_some() && expr_contains_await(&closure.body) {
                    let previous = std::mem::replace(&mut self.in_content_closure, true);
                    self.instrument_suspending_expr(&mut closure.body);
                    self.in_content_closure = previous;
                    return;
                }
                self.wrap_closure(closure);
            }
            Expr::Async(async_block) => {
                self.instrument_block_by_suspension(&mut async_block.block);
            }
            Expr::Const(const_block) => {
                self.instrument_sync_interiors_block(&mut const_block.block);
            }
            Expr::If(expr_if) => {
                let condition = source_name(&expr_if.cond);
                self.wrap_condition(&mut expr_if.cond);
                self.wrap_block(&mut expr_if.then_branch, "then", &condition);
                if let Some((_, else_expr)) = &mut expr_if.else_branch {
                    match else_expr.as_mut() {
                        Expr::If(_) => self.visit_expr_mut(else_expr),
                        Expr::Block(block_expr) => {
                            self.wrap_block(&mut block_expr.block, "else", &condition);
                        }
                        other => self.visit_expr_mut(other),
                    }
                }
            }
            Expr::Match(expr_match) => {
                self.visit_expr_mut(&mut expr_match.expr);
                for arm in &mut expr_match.arms {
                    let pattern = source_name(&arm.pat);
                    if let Pat::Guard(pat_guard) = &mut arm.pat {
                        self.wrap_condition(&mut pat_guard.guard);
                    }
                    self.wrap_arm_body(&mut arm.body, &pattern);
                }
            }
            Expr::ForLoop(for_loop) => {
                let pattern = source_name(&for_loop.pat);
                self.visit_expr_mut(&mut for_loop.expr);
                self.wrap_block(&mut for_loop.body, "for", &pattern);
            }
            Expr::While(while_loop) => {
                let condition = source_name(&while_loop.cond);
                self.wrap_condition(&mut while_loop.cond);
                self.wrap_block(&mut while_loop.body, "while", &condition);
            }
            Expr::Loop(loop_expr) => {
                self.wrap_block(&mut loop_expr.body, "loop", "");
            }
            Expr::Repeat(repeat) => self.visit_expr_mut(&mut repeat.expr),
            _ => visit_mut::visit_expr_mut(self, expr),
        }
        self.hot.close(&fold);
        if folds_whole_statement {
            let guard = self.branch_guard_stmt(expr.span(), fold);
            let original = expr.clone();
            *expr = syn::parse_quote! {{
                #guard
                #original
            }};
        }
    }

    fn visit_block_mut(&mut self, block: &mut Block) {
        let sandwiches = self.visit_block_statements(block);
        self.fold_local_statements(block, sandwiches);
    }

    fn visit_stmt_mut(&mut self, stmt: &mut Stmt) {
        let hot = match &*stmt {
            Stmt::Expr(expr, Some(_)) => self.hot.open("stmt", &statement_name(expr)),
            _ => None,
        };
        visit_mut::visit_stmt_mut(self, stmt);
        self.hot.close(&hot);
        if let Stmt::Expr(expr, Some(semi)) = stmt {
            let guard = self.branch_guard_stmt(expr.span(), hot);
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
                self.wrap_block(&mut block_expr.block, "diverge", "");
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
#[path = "tests/branch_groups_tests.rs"]
mod tests;
