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
/// The composition path includes content lambdas — closures that call
/// composables — whose guards resolve the composer through the thread-local
/// context. Closures without composable-shaped calls, `async` blocks, `const`
/// blocks and nested items are left alone: their code does not run while this
/// function composes.
pub(crate) fn inject_branch_groups(core_path: &TokenStream2, block: &mut Block) {
    let mut injector = BranchGroupInjector {
        core_path,
        next_branch: 0,
        in_content_closure: false,
    };
    injector.visit_block_mut(block);
}

struct BranchGroupInjector<'a> {
    core_path: &'a TokenStream2,
    next_branch: u32,
    /// Inside a content closure the composable's `__composer` binding cannot
    /// be captured (`'static`), so guards go through the thread-local
    /// composer context instead.
    in_content_closure: bool,
}

impl BranchGroupInjector<'_> {
    fn wrap_block(&mut self, block: &mut Block) {
        let needs_group = block_can_reach_composer(block);
        self.visit_block_mut(block);
        if !needs_group {
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
        let needs_group = expr_can_reach_composer(body);
        self.visit_expr_mut(body);
        if !needs_group {
            return;
        }
        let guard = self.branch_guard_stmt(body.span());
        let original = body.clone();
        *body = syn::parse_quote! {{
            #guard
            #original
        }};
    }

    /// The guard binding carries a leading underscore so an otherwise empty
    /// branch does not warn, while still living to the end of the branch —
    /// `let _ = …` would drop the group immediately.
    fn branch_guard_stmt(&mut self, span: Span) -> Stmt {
        let branch = self.next_branch;
        self.next_branch += 1;
        let core_path = self.core_path;
        if self.in_content_closure {
            syn::parse_quote_spanned! {span=>
                let __cranpose_branch_group_guard = #core_path::__branch_group_scope(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                );
            }
        } else {
            syn::parse_quote_spanned! {span=>
                let __cranpose_branch_group_guard = __composer.__branch_group(
                    #core_path::branch_location_key(file!(), line!(), column!(), #branch),
                );
            }
        }
    }
}

impl VisitMut for BranchGroupInjector<'_> {
    fn visit_expr_mut(&mut self, expr: &mut Expr) {
        match expr {
            // A closure that calls composables is a content lambda — plain
            // content, a lazy item's `|index| …`, a scope builder: its
            // branches compose and need groups like the direct body's. Its
            // guards resolve the composer through the thread-local context,
            // which yields `None` outside an active composition pass, so a
            // closure this misclassifies (an event handler that happens to
            // call a CamelCase constructor) keeps its old behavior instead
            // of breaking.
            Expr::Closure(closure) if expr_calls_composable(&closure.body) => {
                let previous = std::mem::replace(&mut self.in_content_closure, true);
                self.visit_expr_mut(&mut closure.body);
                self.in_content_closure = previous;
            }
            // Not on this function's composition path: other closures may run
            // long after composition, async and const blocks never see the
            // composer.
            Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) => {}
            Expr::If(expr_if) => {
                self.visit_expr_mut(&mut expr_if.cond);
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
                        self.visit_expr_mut(guard);
                    }
                    self.wrap_arm_body(&mut arm.body);
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

    fn visit_item_mut(&mut self, _item: &mut syn::Item) {
        // A nested item owns its own scope; it is not this composable's body.
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

/// Whether a branch could interact with the composer. Composition always goes
/// through a free-function call or a macro: composables are free functions by
/// construction (the composer travels through `with_current_composer`, never
/// through a receiver), so a branch whose only calls are method calls —
/// `value.to_string()`, `text.clone()` — cannot create slots and needs no
/// group. Std's value macros (`format!`, `vec!`, …) cannot compose either and
/// are skipped by name; every other macro wraps, which is what keeps
/// `DisposableEffect!` and friends grouped. Closures, async and const blocks
/// and nested items are skipped for the same reason the transform skips them.
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

/// Whether an expression contains a direct call shaped like composition: a
/// call or macro whose name is CamelCase (the repo reserves CamelCase for
/// `#[composable]` functions) or a `remember*` hook. This classifies content
/// closures; a false positive only costs a no-op guard, a false negative
/// keeps the pre-transform behavior.
fn expr_calls_composable(expr: &Expr) -> bool {
    let mut scan = ComposableCallScan { found: false };
    scan.visit_expr(expr);
    scan.found
}

struct ComposableCallScan {
    found: bool,
}

fn path_is_composable_shaped(path: &syn::Path) -> bool {
    let Some(segment) = path.segments.last() else {
        return false;
    };
    let name = segment.ident.to_string();
    name.chars().next().is_some_and(char::is_uppercase) || name.starts_with("remember")
}

impl<'ast> Visit<'ast> for ComposableCallScan {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.found {
            return;
        }
        match expr {
            Expr::Call(call) => {
                if let Expr::Path(path) = call.func.as_ref() {
                    if path_is_composable_shaped(&path.path) {
                        self.found = true;
                        return;
                    }
                }
                syn::visit::visit_expr_call(self, call);
            }
            Expr::Macro(expr_macro) => {
                if path_is_composable_shaped(&expr_macro.mac.path)
                    || tokens_contain_composable_call(&expr_macro.mac.tokens)
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
            if path_is_composable_shaped(&stmt_macro.mac.path)
                || tokens_contain_composable_call(&stmt_macro.mac.tokens)
            {
                self.found = true;
            }
            return;
        }
        syn::visit::visit_stmt(self, stmt);
    }
}

/// Whether raw macro tokens contain what looks like a composable call: a
/// CamelCase or `remember*` identifier immediately followed by a
/// parenthesized group. Macro arguments are token soup to a proc macro, so
/// `format!("{}", Title())` needs this to be seen at all.
fn tokens_contain_composable_call(tokens: &TokenStream2) -> bool {
    let mut previous_composable_ident = false;
    for tree in tokens.clone() {
        match &tree {
            proc_macro2::TokenTree::Ident(ident) => {
                let name = ident.to_string();
                previous_composable_ident = name.chars().next().is_some_and(char::is_uppercase)
                    || name.starts_with("remember");
            }
            proc_macro2::TokenTree::Group(group) => {
                if previous_composable_ident
                    && group.delimiter() == proc_macro2::Delimiter::Parenthesis
                {
                    return true;
                }
                if tokens_contain_composable_call(&group.stream()) {
                    return true;
                }
                previous_composable_ident = false;
            }
            _ => previous_composable_ident = false,
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

struct ComposerReachScan {
    found: bool,
}

impl<'ast> Visit<'ast> for ComposerReachScan {
    fn visit_expr(&mut self, expr: &'ast Expr) {
        if self.found {
            return;
        }
        match expr {
            Expr::Closure(_) | Expr::Async(_) | Expr::Const(_) => {}
            Expr::Call(_) => self.found = true,
            Expr::Macro(expr_macro) => {
                if !macro_is_value_shaped(&expr_macro.mac.path)
                    || tokens_contain_composable_call(&expr_macro.mac.tokens)
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
                || tokens_contain_composable_call(&stmt_macro.mac.tokens)
            {
                self.found = true;
            }
            return;
        }
        syn::visit::visit_stmt(self, stmt);
    }

    fn visit_item(&mut self, _item: &'ast syn::Item) {}
}
