// Which functions of the crate can run code the caller does not see: a
// callback, a `dyn` method, or the drop of a value that holds a trait object.

use rustc_data_structures::fx::{FxHashMap, FxHashSet};
use rustc_hir::def::DefKind;
use rustc_middle::{
    mir::TerminatorKind,
    ty::{self, GenericArgsRef, Instance, InstanceKind, ShimKind, Ty, TyCtxt, TypingEnv},
};
use rustc_span::{def_id::DefId, sym};
use rustc_type_ir::Unnormalized;

// The crates whose `dyn` traits run no caller code of their own.
const STD_CRATES: [&str; 3] = ["core", "alloc", "std"];

// What one call or one drop can reach.
#[derive(Default)]
pub(crate) struct Effects {
    // Code no summary covers: a `dyn` method, a function pointer, the drop of
    // a trait object.
    foreign: bool,
    // Bodies of this crate it runs: functions, closures and `Drop::drop`s.
    callees: Vec<DefId>,
}

impl Effects {
    pub(crate) fn reenters(&self, reentrant: &FxHashSet<DefId>) -> bool {
        self.foreign || self.callees.iter().any(|callee| reentrant.contains(callee))
    }
}

pub(crate) struct Walker<'tcx> {
    tcx: TyCtxt<'tcx>,
    typing_env: TypingEnv<'tcx>,
    drops: FxHashMap<Ty<'tcx>, Effects>,
}

impl<'tcx> Walker<'tcx> {
    pub(crate) fn new(tcx: TyCtxt<'tcx>, typing_env: TypingEnv<'tcx>) -> Self {
        Self {
            tcx,
            typing_env,
            drops: FxHashMap::default(),
        }
    }

    pub(crate) fn call(&mut self, did: DefId, args: GenericArgsRef<'tcx>, out: &mut Effects) {
        let tcx = self.tcx;
        if tcx.is_diagnostic_item(sym::mem_drop, did) {
            if let Some(ty) = args.types().next() {
                self.drop(ty, out);
            }
            return;
        }
        // A closure of this crate handed to any function can run there.
        for ty in args.types() {
            if let ty::Closure(closure, _) = ty.kind()
                && closure.is_local()
            {
                out.callees.push(*closure);
            }
        }
        let Ok(args) =
            tcx.try_normalize_erasing_regions(self.typing_env, Unnormalized::new_wip(args))
        else {
            return;
        };
        match Instance::try_resolve(tcx, self.typing_env, did, args) {
            Ok(Some(instance)) => match instance.def {
                InstanceKind::Item(item) if item.is_local() => out.callees.push(item),
                InstanceKind::Virtual(item, _) => self.virtual_call(item, out),
                InstanceKind::Shim(ShimKind::FnPtr(..)) => out.foreign = true,
                InstanceKind::Shim(ShimKind::DropGlue(_, Some(ty))) => self.drop(ty, out),
                _ => {}
            },
            // A trait method on a generic type: any impl of this crate.
            Ok(None) => {
                if let Some(trait_did) = self.trait_of(did)
                    && trait_did.is_local()
                {
                    self.local_impls(trait_did, did, out);
                }
            }
            Err(_) => {}
        }
    }

    pub(crate) fn callee(&mut self, callee: Ty<'tcx>, out: &mut Effects) {
        match callee.kind() {
            ty::FnDef(did, args) => self.call(*did, args.skip_binder(), out),
            ty::FnPtr(..) => out.foreign = true,
            _ => {}
        }
    }

    pub(crate) fn drop(&mut self, ty: Ty<'tcx>, out: &mut Effects) {
        if let Some(known) = self.drops.get(&ty) {
            merge(out, known);
            return;
        }
        let mut effects = Effects::default();
        let mut seen = FxHashSet::default();
        self.drop_parts(ty, &mut seen, &mut effects);
        merge(out, &effects);
        self.drops.insert(ty, effects);
    }

    fn drop_parts(&self, ty: Ty<'tcx>, seen: &mut FxHashSet<Ty<'tcx>>, out: &mut Effects) {
        if !ty.needs_drop(self.tcx, self.typing_env) || !seen.insert(ty) {
            return;
        }
        match ty.kind() {
            ty::Dynamic(..) => out.foreign = true,
            ty::Adt(adt, args) => {
                if let Some(boxed) = ty.boxed_ty() {
                    self.drop_parts(boxed, seen, out);
                    return;
                }
                if adt.is_phantom_data() {
                    self.drop_parts(args.type_at(0), seen, out);
                    return;
                }
                if let Some(destructor) = self.tcx.adt_destructor(adt.did())
                    && destructor.did.is_local()
                {
                    out.callees.push(destructor.did);
                }
                for field in adt.all_fields() {
                    self.drop_parts(field.ty(self.tcx, args).skip_norm_wip(), seen, out);
                }
            }
            ty::Tuple(tys) => {
                for part in tys.iter() {
                    self.drop_parts(part, seen, out);
                }
            }
            ty::Array(part, _) | ty::Slice(part) => self.drop_parts(*part, seen, out),
            ty::Closure(_, args) => {
                for part in args.as_closure().upvar_tys() {
                    self.drop_parts(part, seen, out);
                }
            }
            _ => {}
        }
    }

    fn virtual_call(&self, item: DefId, out: &mut Effects) {
        let Some(trait_did) = self.trait_of(item) else {
            out.foreign = true;
            return;
        };
        if trait_did.is_local() {
            self.local_impls(trait_did, item, out);
        } else if self.tcx.is_fn_trait(trait_did)
            || !STD_CRATES.contains(&self.tcx.crate_name(trait_did.krate).as_str())
        {
            out.foreign = true;
        }
    }

    fn local_impls(&self, trait_did: DefId, item: DefId, out: &mut Effects) {
        for impl_did in self.tcx.all_impls(trait_did) {
            if !impl_did.is_local() {
                continue;
            }
            if let Some(&method) = self.tcx.impl_item_implementor_ids(impl_did).get(&item) {
                out.callees.push(method);
            }
        }
        // The trait's default body, for impls that keep it.
        out.callees.push(item);
    }

    fn trait_of(&self, item: DefId) -> Option<DefId> {
        let parent = self.tcx.parent(item);
        (self.tcx.def_kind(parent) == DefKind::Trait).then_some(parent)
    }
}

fn merge(out: &mut Effects, effects: &Effects) {
    out.foreign |= effects.foreign;
    out.callees.extend_from_slice(&effects.callees);
}

// The functions, closures and `Drop::drop`s of the crate that can run code
// their caller does not see, directly or through other functions of the crate.
pub(crate) fn reentrant_fns(tcx: TyCtxt<'_>) -> FxHashSet<DefId> {
    let mut direct = Vec::new();
    let mut callers: FxHashMap<DefId, Vec<DefId>> = FxHashMap::default();
    for owner in tcx.hir_body_owners() {
        let did = owner.to_def_id();
        if !matches!(
            tcx.def_kind(did),
            DefKind::Fn | DefKind::AssocFn | DefKind::Closure
        ) || !tcx.is_mir_available(did)
        {
            continue;
        }
        let body = tcx.optimized_mir(did);
        let mut walker = Walker::new(tcx, body.typing_env(tcx));
        let mut effects = Effects::default();
        // Unwinding drops run only after a panic.
        for block in body.basic_blocks.iter().filter(|block| !block.is_cleanup) {
            match &block.terminator().kind {
                TerminatorKind::Call { func, .. } | TerminatorKind::TailCall { func, .. } => {
                    walker.callee(func.ty(body, tcx), &mut effects);
                }
                TerminatorKind::Drop { place, .. } => {
                    walker.drop(place.ty(body, tcx).ty, &mut effects);
                }
                _ => {}
            }
        }
        if effects.foreign {
            direct.push(did);
        }
        for callee in effects.callees {
            callers.entry(callee).or_default().push(did);
        }
    }
    let mut reentrant: FxHashSet<DefId> = direct.iter().copied().collect();
    let mut queue = direct;
    while let Some(did) = queue.pop() {
        for &caller in callers.get(&did).into_iter().flatten() {
            if reentrant.insert(caller) {
                queue.push(caller);
            }
        }
    }
    reentrant
}
