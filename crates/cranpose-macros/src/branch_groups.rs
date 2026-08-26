//! The branch-group transform: what Compose's compiler plugin does for
//! conditionals, performed on the parsed body of a `#[composable]` function.
//!
//! Every `if`/`else` branch and `match` arm that can reach the composer is
//! given a group of its own, keyed on the branch's source location plus a
//! per-function branch index. Without this, two branches emitting the same
//! slot shape are one slot: the arriving branch is handed the node,
//! `remember` values and effects the departing branch was using.
//!
//! The guard is an RAII value rather than a closure so `return`, `?`, `break`
//! and `continue` inside a branch keep working; dropping the guard closes the
//! group on every exit path.

use proc_macro2::{Span, TokenStream as TokenStream2};
use syn::{
    spanned::Spanned,
    visit::Visit,
    visit_mut::{self, VisitMut},
    Block, Expr, Stmt,
};

/// Wrap every conditional branch of `block` in a branch group.
///
/// The composition path includes content lambdas — closures whose bodies
/// could reach the composer — and plain nested `fn`s, both of whose guards
/// resolve the composer through the thread-local context. Closures that
/// cannot reach the composer, `async` and `const` blocks and non-`fn` nested
/// items are left alone: their code does not run while this function
/// composes.
pub(crate) fn inject_branch_groups(core_path: &TokenStream2, block: &mut Block) {
    let mut injector = BranchGroupInjector {
        core_path,
        next_branch: 0,
        in_content_closure: false,
        uses_composer_alias: false,
        branch_depth: 0,
        closure_in_arg_position: false,
    };
    injector.visit_block_mut(block);
    if injector.uses_composer_alias {
        // Captured before any user statement runs, under `mixed_site` hygiene:
        // a user binding or shadowing of `__composer` later in the body can
        // neither be seen by the guards nor break them.
        let alias = composer_alias_ident();
        block
            .stmts
            .insert(0, syn::parse_quote! { let #alias = __composer; });
    }
}

/// The guards' reference to the composable's composer parameter. `mixed_site`
/// spans from one expansion share a hygiene context, so the binding this
/// names in the body prologue is the one every guard resolves, and user code
/// can neither read nor shadow it.
fn composer_alias_ident() -> syn::Ident {
    syn::Ident::new("__cranpose_branch_composer", Span::mixed_site())
}

struct BranchGroupInjector<'a> {
    core_path: &'a TokenStream2,
    next_branch: u32,
    /// Inside a content closure or nested item the composable's composer
    /// binding cannot be captured (`'static`), so guards go through the
    /// thread-local composer context instead.
    in_content_closure: bool,
    /// Whether any guard referenced the hygienic composer alias, which then
    /// must be bound in the body prologue.
    uses_composer_alias: bool,
    /// How many conditional branches enclose the visit position. A content
    /// closure defined inside a branch may be invoked after the branch guard
    /// closes — `let render = if c { || A(1) } else { || A(2) }; render();` —
    /// so its body gets a group anchored at the closure's own definition
    /// site, carrying the branch identity to wherever it runs.
    branch_depth: u32,
    /// The closure being visited sits directly in call-argument position: it
    /// is consumed by that call — run inline under the still-open branch
    /// guard (content lambdas), or stored as a handler whose guards no-op —
    /// so it needs no body group of its own.
    closure_in_arg_position: bool,
}

impl BranchGroupInjector<'_> {
    fn wrap_block(&mut self, block: &mut Block) {
        let reaches = block_can_reach_composer(block);
        self.branch_depth += 1;
        self.visit_block_mut(block);
        self.branch_depth -= 1;
        if !reaches {
            return;
        }
        let guard = self.branch_guard_stmt(block.brace_token.span.join());
        block.stmts.insert(0, guard);
    }

    fn wrap_arm_body(&mut self, body: &mut Expr) {
        if let Expr::Block(block_expr) = body {
            self.wrap_block(&mut block_expr.block);
            return;
        }
        let reaches = expr_can_reach_composer(body);
        self.branch_depth += 1;
        self.visit_expr_mut(body);
        self.branch_depth -= 1;
        if !reaches {
            return;
        }
        let guard = self.branch_guard_stmt(body.span());
        let original = body.clone();
        *body = syn::parse_quote! {{
            #guard
            #original
        }};
    }

    /// A match guard or `if` condition runs before any branch group opens,
    /// and how much of it runs varies — guards by the scrutinee, conditions
    /// by `&&`/`||` short-circuiting — so a composing one needs a group of
    /// its own: without one its slots land in the parent and shift everything
    /// composed after it. `let` operands cannot be moved into a block — their
    /// bindings must flow into the arm or branch — so the walk descends the
    /// `&&`/`||` spine and wraps each composing operand individually,
    /// wrapping a `let` operand's scrutinee rather than the `let` itself.
    fn wrap_condition(&mut self, condition: &mut Expr) {
        match condition {
            Expr::Binary(binary) if matches!(binary.op, syn::BinOp::And(_) | syn::BinOp::Or(_)) => {
                self.wrap_condition(&mut binary.left);
                self.wrap_condition(&mut binary.right);
            }
            Expr::Paren(paren) => self.wrap_condition(&mut paren.expr),
            Expr::Unary(unary) => self.wrap_condition(&mut unary.expr),
            // A `let` scrutinee's temporaries live into the branch body —
            // that is what lets `if let Some(x) = make().first()` keep the
            // borrow — so it can never be moved into a block. The guard rides
            // along as one of those temporaries instead: a tuple holds the
            // guard while the scrutinee's value is projected out, and the
            // group closes when the `if let`'s temporaries drop, after the
            // branch body.
            Expr::Let(let_expr) => {
                let scrutinee = &mut let_expr.expr;
                if expr_is_place(scrutinee) {
                    // A place must stay a place — a `ref` pattern binds into
                    // it — but its value sub-expressions (an index, a deref
                    // operand) are wrappable, so a composing
                    // `values[stateful_index()]` still gets its group.
                    self.wrap_place_value_parts(scrutinee);
                } else {
                    let needs_group = expr_can_reach_composer(scrutinee);
                    self.visit_expr_mut(scrutinee);
                    if needs_group {
                        let guard_expr = self.branch_guard_expr(scrutinee.span());
                        let original = scrutinee.clone();
                        **scrutinee = syn::parse_quote! { (#guard_expr, #original).1 };
                    }
                }
            }
            leaf => {
                let needs_group = expr_can_reach_composer(leaf) && !expr_contains_let(leaf);
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

    /// The guard binding carries a leading underscore so an otherwise empty
    /// branch does not warn, while still living to the end of the branch —
    /// `let _ = …` would drop the group immediately. The identifier uses
    /// `mixed_site` hygiene, so user code can never see it and a user binding
    /// of the same name is never shadowed. Every guard is a deferred shell:
    /// a reservation that materializes into a real bracket on the branch's
    /// first composing operation and costs a push and a pop otherwise, so
    /// over-matching a branch that never composes is free, and the real
    /// `with_key` keeps its unbracketed keyed sibling structure by passing
    /// through.
    fn branch_guard_stmt(&mut self, span: Span) -> Stmt {
        let branch = self.next_branch;
        self.next_branch += 1;
        let core_path = self.core_path;
        let guard = syn::Ident::new("__cranpose_branch_group_guard", Span::mixed_site());
        if self.in_content_closure {
            syn::parse_quote_spanned! {span=>
                let #guard = #core_path::__branch_group_scope_deferred(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                );
            }
        } else {
            self.uses_composer_alias = true;
            let composer = composer_alias_ident();
            syn::parse_quote_spanned! {span=>
                let #guard = #composer.__branch_group_deferred(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                );
            }
        }
    }

    /// Walk a place expression and wrap its composing value sub-expressions:
    /// the index of an indexing, the operand of a deref, the base chain of a
    /// field access. The place structure itself is untouched.
    fn wrap_place_value_parts(&mut self, place: &mut Expr) {
        match place {
            Expr::Index(index) => {
                self.wrap_place_value_parts(&mut index.expr);
                self.wrap_value_part(&mut index.index);
            }
            Expr::Field(field) => self.wrap_place_value_parts(&mut field.base),
            Expr::Paren(paren) => self.wrap_place_value_parts(&mut paren.expr),
            Expr::Group(group) => self.wrap_place_value_parts(&mut group.expr),
            Expr::Unary(unary) if matches!(unary.op, syn::UnOp::Deref(_)) => {
                self.wrap_value_part(&mut unary.expr);
            }
            _ => {}
        }
    }

    /// Block-wrap one value-position sub-expression when it composes.
    fn wrap_value_part(&mut self, expr: &mut Expr) {
        let needs_group = expr_can_reach_composer(expr);
        self.visit_expr_mut(expr);
        if !needs_group {
            return;
        }
        let guard_stmt = self.branch_guard_stmt(expr.span());
        let original = expr.clone();
        *expr = syn::parse_quote! {{
            #guard_stmt
            #original
        }};
    }

    /// The guard as a bare expression, for positions where a `let` statement
    /// cannot go — a `let` scrutinee's tuple ride-along.
    fn branch_guard_expr(&mut self, span: Span) -> Expr {
        let branch = self.next_branch;
        self.next_branch += 1;
        let core_path = self.core_path;
        if self.in_content_closure {
            syn::parse_quote_spanned! {span=>
                #core_path::__branch_group_scope_deferred(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                )
            }
        } else {
            self.uses_composer_alias = true;
            let composer = composer_alias_ident();
            syn::parse_quote_spanned! {span=>
                #composer.__branch_group_deferred(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                )
            }
        }
    }

    /// A `const` fn must stay const-evaluable and an `async` fn's guard would
    /// live across `.await`; a nested `#[composable]` runs this transform on
    /// itself. Everything else — `unsafe` and `extern` fns included, whose
    /// bodies are ordinary Rust — is transformed like a content closure,
    /// resolving the composer through the thread-local context.
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
        self.in_content_closure = previous;
    }
}

impl VisitMut for BranchGroupInjector<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            // A closure that could reach the composer — any free call or
            // non-value macro in its body, the same rule branches use — is
            // treated as a content lambda: plain content, a lazy item's
            // `|index| …`, a scope builder. Its branches compose and need
            // groups like the direct body's. Guards inside closures resolve
            // the composer through the thread-local context, which yields
            // `None` outside an active composition pass, so an event handler
            // this over-matches keeps its old behavior instead of breaking.
            Expr::Closure(closure) if expr_can_reach_composer(&closure.body) => {
                let previous = std::mem::replace(&mut self.in_content_closure, true);
                let was_arg = std::mem::replace(&mut self.closure_in_arg_position, false);
                self.visit_expr_mut(&mut closure.body);
                // A closure born in a branch can outlive the branch guard and
                // compose at its call site; anchor its body to the closure's
                // definition site so each branch's closure keeps its own
                // slots. One sitting directly in call-argument position is
                // consumed by that call and needs none; outside branches
                // there is no sibling to collide with.
                if self.branch_depth > 0 && !was_arg {
                    let guard = self.branch_guard_stmt(closure.span());
                    let original = closure.body.clone();
                    closure.body = syn::parse_quote! {{
                        #guard
                        #original
                    }};
                }
                self.in_content_closure = previous;
            }
            // Not on this function's composition path: other closures may run
            // long after composition, async and const blocks never see the
            // composer.
            Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) => {}
            Expr::If(expr_if) => {
                self.wrap_condition(&mut expr_if.cond);
                self.wrap_block(&mut expr_if.then_branch);
                if let Some((_, else_expr)) = &mut expr_if.else_branch {
                    match else_expr.as_mut() {
                        // An `else if` chain: recurse so every arm gets its
                        // own branch index.
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
            Expr::Call(call) => {
                self.visit_expr_mut(&mut call.func);
                for argument in &mut call.args {
                    let flag = matches!(argument, Expr::Closure(_));
                    let previous = std::mem::replace(&mut self.closure_in_arg_position, flag);
                    self.visit_expr_mut(argument);
                    self.closure_in_arg_position = previous;
                }
            }
            Expr::MethodCall(method) => {
                self.visit_expr_mut(&mut method.receiver);
                for argument in &mut method.args {
                    let flag = matches!(argument, Expr::Closure(_));
                    let previous = std::mem::replace(&mut self.closure_in_arg_position, flag);
                    self.visit_expr_mut(argument);
                    self.closure_in_arg_position = previous;
                }
            }
            // The repeat count is a const context; only the element repeats
            // at runtime.
            Expr::Repeat(repeat) => self.visit_expr_mut(&mut repeat.expr),
            _ => visit_mut::visit_expr_mut(self, expr),
        }
    }

    fn visit_local_mut(&mut self, local: &mut syn::Local) {
        let Some(init) = &mut local.init else {
            return;
        };
        self.visit_expr_mut(&mut init.expr);
        // A `let … else` diverge block runs only when the pattern refutes:
        // it is a branch like any other.
        if let Some((_, diverge)) = &mut init.diverge {
            if let Expr::Block(block_expr) = diverge.as_mut() {
                self.wrap_block(&mut block_expr.block);
            } else {
                self.visit_expr_mut(diverge);
            }
        }
    }

    fn visit_item_mut(&mut self, item: &mut syn::Item) {
        // A nested `fn`, a local `impl`'s methods, a local trait's default
        // bodies and anything inside a nested `mod` all run under whatever
        // composer is current when they are called — composables resolve the
        // composer through the thread-local context, not a capture — so their
        // branches need groups like a content closure's. Every other item is
        // not executable code.
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
                    if let syn::TraitItem::Fn(method) = trait_item {
                        if let Some(default_body) = &mut method.default {
                            self.visit_nested_fn(&method.sig, &method.attrs, default_body);
                        }
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

    fn visit_type_mut(&mut self, _ty: &mut syn::Type) {
        // Types are const contexts: an array length like `[f32; if …]` must
        // not receive a runtime guard.
    }

    fn visit_angle_bracketed_generic_arguments_mut(
        &mut self,
        _args: &mut syn::AngleBracketedGenericArguments,
    ) {
        // Turbofish arguments are const contexts too.
    }
}

/// Whether a branch could interact with the composer. Every free call could
/// be a composable; method calls compose only through `Composer`'s own
/// surface — the handle is `Clone`, so `with_current_composer(Clone::clone)`
/// can carry it into a branch that then composes purely through methods — and
/// those methods are recognized by name, leaving ordinary calls like
/// `value.to_string()` inert. Std's value macros (`format!`, `vec!`, …)
/// cannot compose either and are skipped by name; every other macro wraps,
/// which is what keeps `DisposableEffect!` and friends grouped. Closures,
/// async and const blocks and nested items are skipped for the same reason
/// the transform skips them.
fn block_can_reach_composer(block: &Block) -> bool {
    let mut scan = ComposerReachScan { found: false };
    scan.visit_block(block);
    scan.found
}

fn expr_can_reach_composer(expr: &Expr) -> bool {
    let mut scan = ComposerReachScan { found: false };
    scan.visit_expr(expr);
    scan.found
}

/// Whether raw macro tokens contain what looks like a call: an identifier or
/// parenthesized callee immediately followed by a parenthesized group. Macro
/// arguments are token soup to a proc macro and any call can transitively
/// compose, so every call shape counts. Over-matching costs a branch a shell
/// reservation that never materializes; missing a composable would silently
/// share its slots across branches.
fn tokens_contain_free_call(tokens: &TokenStream2) -> bool {
    let mut call_candidate = false;
    for tree in tokens.clone() {
        match &tree {
            proc_macro2::TokenTree::Ident(_) => call_candidate = true,
            proc_macro2::TokenTree::Group(group) => {
                if call_candidate && group.delimiter() == proc_macro2::Delimiter::Parenthesis {
                    return true;
                }
                if tokens_contain_free_call(&group.stream()) {
                    return true;
                }
                // A parenthesized callee — `(stateful_label)(1)` — is a group
                // followed by its argument group.
                call_candidate = matches!(
                    group.delimiter(),
                    proc_macro2::Delimiter::Parenthesis | proc_macro2::Delimiter::None
                );
            }
            proc_macro2::TokenTree::Punct(_) | proc_macro2::TokenTree::Literal(_) => {
                call_candidate = false;
            }
        }
    }
    false
}

/// Std macros that produce values or diverge and can never compose.
fn macro_is_value_shaped(path: &syn::Path) -> bool {
    let Some(segment) = path.segments.last() else {
        return false;
    };
    matches!(
        segment.ident.to_string().as_str(),
        "format"
            | "format_args"
            | "vec"
            | "print"
            | "println"
            | "eprint"
            | "eprintln"
            | "write"
            | "writeln"
            | "matches"
            | "assert"
            | "assert_eq"
            | "assert_ne"
            | "debug_assert"
            | "debug_assert_eq"
            | "debug_assert_ne"
            | "todo"
            | "unimplemented"
            | "unreachable"
            | "panic"
            | "include_str"
            | "include_bytes"
            | "concat"
            | "stringify"
            | "file"
            | "line"
            | "column"
            | "cfg"
            | "env"
            | "option_env"
            | "dbg"
            | "trace"
            | "debug"
            | "info"
            | "warn"
            | "error"
    )
}

/// Whether the expression is a place expression — a path, field access,
/// index, or dereference (through parens). A `ref` pattern binds into the
/// place, so a place scrutinee can never be replaced by a value-producing
/// wrapper: `if let Some(ref v) = values[i()]` must keep indexing the
/// vector, not move out of it.
fn expr_is_place(expr: &Expr) -> bool {
    match expr {
        Expr::Path(_) | Expr::Field(_) | Expr::Index(_) => true,
        Expr::Unary(unary) => matches!(unary.op, syn::UnOp::Deref(_)),
        Expr::Paren(paren) => expr_is_place(&paren.expr),
        Expr::Group(group) => expr_is_place(&group.expr),
        _ => false,
    }
}

/// Whether the expression contains a `let` outside closures — a match guard
/// with one cannot be wrapped in a block without severing the bindings the
/// arm body relies on.
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

struct ComposerReachScan {
    found: bool,
}

impl<'ast> Visit<'ast> for ComposerReachScan {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.found {
            return;
        }
        match expr {
            // Closures are scanned through: `opt.map(|_| Child())` runs its
            // closure during composition, and one that truly runs later only
            // costs its branch a guard that no-ops outside a pass. Async and
            // const blocks never run on the composition path.
            Expr::Closure(closure) => self.visit_expr(&closure.body),
            Expr::Async(_) | Expr::Const(_) => {}
            Expr::Call(_) => self.found = true,
            // Any method can transitively reach the composer through the
            // thread-local context — `Helper.render()` calling a composable
            // inside — so every call shape counts. The shell this earns is a
            // reservation that costs nothing unless something composes.
            Expr::MethodCall(_) => self.found = true,
            Expr::Macro(expr_macro) => {
                if !macro_is_value_shaped(&expr_macro.mac.path)
                    || tokens_contain_free_call(&expr_macro.mac.tokens)
                {
                    self.found = true;
                }
            }
            _ => syn::visit::visit_expr(self, expr),
        }
    }

    fn visit_stmt(&mut self, stmt: &'ast Stmt) {
        if self.found {
            return;
        }
        if let Stmt::Macro(stmt_macro) = stmt {
            if !macro_is_value_shaped(&stmt_macro.mac.path)
                || tokens_contain_free_call(&stmt_macro.mac.tokens)
            {
                self.found = true;
            }
            return;
        }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_item(&mut self, _item: &'ast syn::Item) {}
}
