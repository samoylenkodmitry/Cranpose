#![feature(rustc_private)]

extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;

use rustc_errors::DiagDecorator;
use rustc_hir::{
    Block, Expr, ExprKind, HirId, LetStmt, PatKind, QPath, StmtKind,
    def::Res,
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, Ty};
use rustc_span::{Span, sym};

dylint_linting::declare_late_lint! {
    /// ### What it does
    ///
    /// Finds a call to a closure, a function pointer or a `dyn Fn` while a
    /// `RefCell` borrow (`Ref`, `RefMut`) or a lock guard (`MutexGuard`,
    /// `RwLockReadGuard`, `RwLockWriteGuard`, and `lock_api`'s) that a `let`
    /// holds is alive in the same block.
    ///
    /// ### Why is this bad?
    ///
    /// The callback is code the caller does not see. When it reaches the
    /// same cell or lock, the borrow panics or the lock deadlocks, at run
    /// time, on the path that happens to re-enter. AGENTS.md: a `RefCell`
    /// borrow or a lock lasts a few lines and never spans a callback.
    ///
    /// ### Known problems
    ///
    /// Only `let`-bound guards and direct calls in their block count: a
    /// temporary guard inside one expression, and a closure handed to a
    /// named function that calls it, are not seen. A call that lends the
    /// guard's data to the callback (`f(&*borrow)`, or a value a `let` took
    /// from the guard) is the `with` accessor's purpose and is left alone.
    ///
    /// ### Example
    ///
    /// ```rust
    /// # use std::cell::RefCell;
    /// fn notify(listeners: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    ///     let listeners = listeners.borrow();
    ///     on_change();
    ///     drop(listeners);
    /// }
    /// ```
    ///
    /// Use instead:
    ///
    /// ```rust
    /// # use std::cell::RefCell;
    /// fn notify(listeners: &RefCell<Vec<i32>>, on_change: impl Fn()) {
    ///     let count = listeners.borrow().len();
    ///     on_change();
    ///     let _ = count;
    /// }
    /// ```
    pub GUARD_ACROSS_CALLBACK,
    Warn,
    "a RefCell borrow or a lock guard is alive while a callback runs"
}

impl<'tcx> LateLintPass<'tcx> for GuardAcrossCallback {
    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        let mut guards: Vec<(HirId, Span)> = Vec::new();
        // The guards and the locals a `let` took from them.
        let mut lent: Vec<HirId> = Vec::new();
        for stmt in block.stmts {
            if let Some(&(_, span)) = guards.first() {
                let mut calls = CallbackCalls::new(cx, &mut guards, &lent);
                match stmt.kind {
                    StmtKind::Let(local) => {
                        if let Some(init) = local.init {
                            calls.visit_expr(init);
                        }
                    }
                    StmtKind::Expr(expr) | StmtKind::Semi(expr) => calls.visit_expr(expr),
                    StmtKind::Item(_) => {}
                }
                let found = calls.found;
                report(cx, found, span);
            }
            if let StmtKind::Let(local) = stmt.kind {
                if let Some(guard) = guard_binding(cx, local) {
                    guards.push(guard);
                    lent.push(guard.0);
                } else if let (PatKind::Binding(_, id, _, _), Some(init)) =
                    (local.pat.kind, local.init)
                    && mentions(init, &lent)
                {
                    lent.push(id);
                }
            }
        }
        if let (Some(tail), Some(&(_, span))) = (block.expr, guards.first()) {
            let mut calls = CallbackCalls::new(cx, &mut guards, &lent);
            calls.visit_expr(tail);
            let found = calls.found;
            report(cx, found, span);
        }
    }
}

fn report(cx: &LateContext<'_>, found: Vec<Span>, guard: Span) {
    for call in found {
        cx.emit_span_lint(
            GUARD_ACROSS_CALLBACK,
            call,
            DiagDecorator(|diag| {
                diag.primary_message(
                    "a callback runs while a RefCell borrow or lock guard is alive",
                );
                diag.span_note(guard, "the guard, alive until the end of its block");
                diag.help("end the borrow first: copy what the callback needs, or drop the guard");
            }),
        );
    }
}

// The binding of a `let` whose value is a borrow or lock guard.
fn guard_binding(cx: &LateContext<'_>, local: &LetStmt<'_>) -> Option<(HirId, Span)> {
    let PatKind::Binding(_, id, _, _) = local.pat.kind else {
        return None;
    };
    let init = local.init?;
    is_guard(cx, cx.typeck_results().expr_ty(init)).then_some((id, local.span))
}

fn is_guard(cx: &LateContext<'_>, ty: Ty<'_>) -> bool {
    let ty::Adt(adt, _) = ty.kind() else {
        return false;
    };
    let did = adt.did();
    if let Some(name) = cx.tcx.get_diagnostic_name(did)
        && matches!(
            name.as_str(),
            "RefCellRef" | "RefCellRefMut" | "MutexGuard" | "RwLockReadGuard" | "RwLockWriteGuard"
        )
    {
        return true;
    }
    let path = cx.tcx.def_path_str(did);
    path.starts_with("lock_api::") && path.ends_with("Guard")
}

// The calls of closures, function pointers and `dyn Fn` values in an
// expression, outside the bodies of closures it defines, while a guard is
// alive: a `drop(guard)` ends that guard.
struct CallbackCalls<'a, 'tcx> {
    cx: &'a LateContext<'tcx>,
    guards: &'a mut Vec<(HirId, Span)>,
    lent: &'a [HirId],
    found: Vec<Span>,
}

impl<'a, 'tcx> CallbackCalls<'a, 'tcx> {
    fn new(
        cx: &'a LateContext<'tcx>,
        guards: &'a mut Vec<(HirId, Span)>,
        lent: &'a [HirId],
    ) -> Self {
        Self {
            cx,
            guards,
            lent,
            found: Vec::new(),
        }
    }

    fn dropped_guard(&self, callee: &Expr<'_>, args: &[Expr<'_>]) -> Option<HirId> {
        let ExprKind::Path(QPath::Resolved(_, path)) = callee.kind else {
            return None;
        };
        let Res::Def(_, did) = path.res else {
            return None;
        };
        if !self.cx.tcx.is_diagnostic_item(sym::mem_drop, did) {
            return None;
        }
        let [arg] = args else {
            return None;
        };
        let ExprKind::Path(QPath::Resolved(None, arg_path)) = arg.kind else {
            return None;
        };
        match arg_path.res {
            Res::Local(id) => Some(id),
            _ => None,
        }
    }
}

impl<'tcx> Visitor<'tcx> for CallbackCalls<'_, 'tcx> {
    fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
        match expr.kind {
            ExprKind::Closure(_) => return,
            ExprKind::Call(callee, args) => {
                if let Some(id) = self.dropped_guard(callee, args) {
                    self.guards.retain(|&(guard, _)| guard != id);
                } else if !self.guards.is_empty()
                    && !args.iter().any(|arg| mentions(arg, self.lent))
                {
                    let callee_ty = self.cx.typeck_results().expr_ty(callee).peel_refs();
                    if !matches!(callee_ty.kind(), ty::FnDef(..)) {
                        self.found.push(expr.span);
                    }
                }
            }
            _ => {}
        }
        intravisit::walk_expr(self, expr);
    }
}

// Whether `expr` reads one of the locals `ids`.
fn mentions(expr: &Expr<'_>, ids: &[HirId]) -> bool {
    struct Mentions<'a> {
        ids: &'a [HirId],
        found: bool,
    }
    impl<'tcx> Visitor<'tcx> for Mentions<'_> {
        fn visit_expr(&mut self, expr: &'tcx Expr<'tcx>) {
            if let ExprKind::Path(QPath::Resolved(None, path)) = expr.kind
                && let Res::Local(id) = path.res
                && self.ids.contains(&id)
            {
                self.found = true;
            }
            intravisit::walk_expr(self, expr);
        }
    }
    let mut visitor = Mentions { ids, found: false };
    visitor.visit_expr(expr);
    visitor.found
}

#[test]
fn ui() {
    dylint_testing::ui_test(env!("CARGO_PKG_NAME"), "ui");
}
