#![feature(rustc_private)]

extern crate rustc_data_structures;
extern crate rustc_errors;
extern crate rustc_hir;
extern crate rustc_middle;
extern crate rustc_span;
extern crate rustc_type_ir;

mod reentry;

use reentry::{Effects, Walker};
use rustc_data_structures::fx::FxHashSet;
use rustc_errors::DiagDecorator;
use rustc_hir::{
    Block, Expr, ExprKind, HirId, LetStmt, PatKind, QPath, StmtKind,
    def::Res,
    intravisit::{self, Visitor},
};
use rustc_lint::{LateContext, LateLintPass, LintContext};
use rustc_middle::ty::{self, GenericArgsRef, Ty};
use rustc_span::{Span, def_id::DefId, sym};

dylint_linting::impl_late_lint! {
    /// ### What it does
    ///
    /// Finds code the caller does not see that runs while a `RefCell` borrow
    /// (`Ref`, `RefMut`) or a lock guard (`MutexGuard`, `RwLockReadGuard`,
    /// `RwLockWriteGuard`, and `lock_api`'s) that a `let` holds is alive in
    /// the same block. That code is one of these:
    ///
    /// - a call to a closure, a function pointer or a `dyn Fn`
    /// - a call to a function of the crate whose body reaches a callback, a
    ///   `dyn` method or the drop of a value that holds a trait object
    ///   (`Box<dyn Any>`, `Rc<dyn Fn()>`), directly or through other
    ///   functions, closures and `Drop` impls of the crate
    /// - a `drop` of a value whose drop reaches such code
    ///
    /// ### Why is this bad?
    ///
    /// The callback is code the caller does not see. When it reaches the
    /// same cell or lock, the borrow panics or the lock deadlocks, at run
    /// time, on the path that happens to re-enter. A `Drop` impl added far
    /// away makes an old call re-enter, so the lint follows each call into
    /// its body. AGENTS.md: a `RefCell` borrow or a lock lasts a few lines
    /// and never spans a callback.
    ///
    /// ### Known problems
    ///
    /// Only `let`-bound guards and calls in their block count: a temporary
    /// guard inside one expression is not seen. The analysis stays inside
    /// one crate. A function of another crate counts as plain code, and only
    /// `dyn` methods of traits outside the standard library count as
    /// callbacks there. A drop inside a standard library function
    /// (`Vec::clear`) or at the end of a scope is not seen. A call that lends
    /// the guard's data to a closure (`f(&*borrow)`, or a value a `let` took
    /// from the guard) is the `with` accessor's purpose and is left alone. A
    /// named function that takes the guard's data still counts: its body can
    /// reach the cell another way.
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
    "a RefCell borrow or a lock guard is alive while a callback runs",
    GuardAcrossCallback::default()
}

#[derive(Default)]
pub struct GuardAcrossCallback {
    // The functions of the crate that can run code their caller does not see.
    reentrant: FxHashSet<DefId>,
}

impl<'tcx> LateLintPass<'tcx> for GuardAcrossCallback {
    fn check_crate(&mut self, cx: &LateContext<'tcx>) {
        if cx.tcx.sess.is_test_crate() {
            return;
        }
        self.reentrant = reentry::reentrant_fns(cx.tcx);
    }

    fn check_block(&mut self, cx: &LateContext<'tcx>, block: &'tcx Block<'tcx>) {
        // A test runs its re-entry, so the failure shows. Tests also hold
        // a global test lock for their whole body.
        if cx.tcx.sess.is_test_crate() {
            return;
        }
        let mut walker = Walker::new(cx.tcx, cx.typing_env());
        let mut guards: Vec<(HirId, Span)> = Vec::new();
        // The guards and the locals a `let` took from them.
        let mut lent: Vec<HirId> = Vec::new();
        for stmt in block.stmts {
            if let Some(&(_, span)) = guards.first() {
                let mut calls =
                    CallbackCalls::new(cx, &mut guards, &lent, &mut walker, &self.reentrant);
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
            let mut calls =
                CallbackCalls::new(cx, &mut guards, &lent, &mut walker, &self.reentrant);
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
    walker: &'a mut Walker<'tcx>,
    reentrant: &'a FxHashSet<DefId>,
    found: Vec<Span>,
}

impl<'a, 'tcx> CallbackCalls<'a, 'tcx> {
    fn new(
        cx: &'a LateContext<'tcx>,
        guards: &'a mut Vec<(HirId, Span)>,
        lent: &'a [HirId],
        walker: &'a mut Walker<'tcx>,
        reentrant: &'a FxHashSet<DefId>,
    ) -> Self {
        Self {
            cx,
            guards,
            lent,
            walker,
            reentrant,
            found: Vec::new(),
        }
    }

    // Whether a call of `did` can reach code the caller does not see. A named
    // function counts even when it borrows the guard's data: its body can
    // reach the cell another way.
    fn reenters(&mut self, did: DefId, args: GenericArgsRef<'tcx>) -> bool {
        let mut effects = Effects::default();
        self.walker.call(did, args, &mut effects);
        effects.reenters(self.reentrant)
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
                if let Some(id) = self.dropped_guard(callee, args)
                    && self.guards.iter().any(|&(guard, _)| guard == id)
                {
                    self.guards.retain(|&(guard, _)| guard != id);
                } else if !self.guards.is_empty() {
                    let callee_ty = self.cx.typeck_results().expr_ty(callee).peel_refs();
                    if let ty::FnDef(did, fn_args) = callee_ty.kind() {
                        if self.reenters(*did, fn_args.skip_binder()) {
                            self.found.push(expr.span);
                        }
                    } else if !args.iter().any(|arg| mentions(arg, self.lent)) {
                        self.found.push(expr.span);
                    }
                }
            }
            ExprKind::MethodCall(..) if !self.guards.is_empty() => {
                let typeck = self.cx.typeck_results();
                if let Some(did) = typeck.type_dependent_def_id(expr.hir_id)
                    && self.reenters(did, typeck.node_args(expr.hir_id))
                {
                    self.found.push(expr.span);
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
