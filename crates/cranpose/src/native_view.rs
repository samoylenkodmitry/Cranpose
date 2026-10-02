use std::{cell::RefCell, rc::Rc};

use cranpose_core::{NodeId, remember};
use cranpose_ui::{LayoutBox, LayoutTree, Modifier, Rect, Spacer, composable};

thread_local! {
    static HOST: cranpose_core::CompositionLocal<NativeViewHost> =
        cranpose_core::compositionLocalOf(NativeViewHost::default);
}

pub(crate) fn current_host() -> NativeViewHost {
    HOST.with(cranpose_core::CompositionLocal::current)
}

type EventHandler = Rc<dyn Fn(&str)>;

struct Slot {
    id: u64,
    node: NodeId,
    kind: String,
    value: String,
    on_event: EventHandler,
}

#[derive(Default)]
struct Registry {
    next_id: u64,
    slots: Vec<Slot>,
}

impl Registry {
    fn allocate_id(&mut self) -> u64 {
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("native view ID exhausted");
        self.next_id
    }
}

/// Owns native-view requests for one Cranpose component.
///
/// Each platform host reconciles the snapshots from [Self::layout] with its
/// child views. IDs survive recomposition while the factory kind stays the same
/// and are never reused by this host. Changing kind retires the old identity.
/// Dropping a composable removes its request; an empty snapshot removes all
/// native children. Native children render above Cranpose and own their input.
#[derive(Clone, Default)]
pub struct NativeViewHost {
    registry: Rc<RefCell<Registry>>,
}

impl PartialEq for NativeViewHost {
    fn eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.registry, &other.registry)
    }
}

/// A platform view's identity, content and bounds in logical host coordinates.
///
/// Hosts must clip children to their Cranpose container. Only axis-aligned
/// layout is supported; do not apply graphics transforms, effects or rounded
/// clips to a native slot or its ancestors. Native children appear above all
/// Cranpose drawing, so overlapping Cranpose popups require hiding them.
#[derive(Clone, Debug, PartialEq)]
pub struct NativeViewLayout {
    /// Stable mount identity, scoped to one host.
    pub id: u64,
    /// Application-defined factory name, such as `web` or `map`.
    pub kind: String,
    /// Application-defined configuration, interpreted by the native factory.
    pub value: String,
    /// Bounds in logical points, including ancestor content offsets.
    pub bounds: Rect,
}

impl NativeViewHost {
    /// Supplies the native view host used by [`crate::WebView`] in this composition.
    pub fn provide(&self, content: impl FnOnce()) {
        HOST.with(|local| {
            cranpose_core::CompositionLocalProvider([local.provides(self.clone())], content);
        });
    }

    /// Returns whether no native children are mounted, so hosts can skip layout snapshots.
    pub fn is_empty(&self) -> bool {
        self.registry.borrow().slots.is_empty()
    }

    /// Returns mounted native children in layout order, including offscreen
    /// children so scrolling preserves their state. An absent layout mounts none.
    pub fn layout(&self, tree: Option<&LayoutTree>) -> Vec<NativeViewLayout> {
        let mut result = Vec::new();
        self.for_each_layout(tree, |id, kind, value, bounds| {
            result.push(NativeViewLayout {
                id,
                kind: kind.to_owned(),
                value: value.to_owned(),
                bounds,
            });
        });
        result
    }

    /// Visits mounted children without allocating an intermediate owned snapshot.
    ///
    /// Configuration strings remain borrowed for the duration of each callback.
    pub fn for_each_layout(
        &self,
        tree: Option<&LayoutTree>,
        mut visit: impl FnMut(u64, &str, &str, Rect),
    ) {
        let registry = self.registry.borrow();
        if !registry.slots.is_empty()
            && let Some(tree) = tree
        {
            collect_layout(tree.root(), &registry.slots, &mut visit);
        }
    }

    /// Delivers an application event from a mounted native child.
    ///
    /// Returns false for stale IDs. Call on the component's owning thread.
    pub fn dispatch(&self, id: u64, event: &str) -> bool {
        let handler = self
            .registry
            .borrow()
            .slots
            .iter()
            .find(|slot| slot.id == id)
            .map(|slot| Rc::clone(&slot.on_event));
        if let Some(handler) = handler {
            handler(event);
            true
        } else {
            false
        }
    }
}

struct Mount {
    host: NativeViewHost,
    id: u64,
}

impl Drop for Mount {
    fn drop(&mut self) {
        self.host
            .registry
            .borrow_mut()
            .slots
            .retain(|slot| slot.id != self.id);
    }
}

fn collect_layout(node: &LayoutBox, slots: &[Slot], visit: &mut impl FnMut(u64, &str, &str, Rect)) {
    if let Some(slot) = slots.iter().find(|slot| slot.node == node.node_id) {
        visit(slot.id, &slot.kind, &slot.value, node.rect);
    }
    for child in &node.children {
        collect_layout(child, slots, visit);
    }
}

/// Reserves layout space for a platform view created by `host`.
///
/// `kind` selects an application-owned native factory and `value` configures
/// it. The host calls [NativeViewHost::dispatch] to deliver native events to
/// `on_event`. Supply a size through `modifier`; no platform intrinsic size
/// is queried. Keep `host` stable for the lifetime of this composition.
#[composable]
pub fn NativeView(
    host: NativeViewHost,
    kind: &str,
    value: &str,
    modifier: Modifier,
    on_event: impl Fn(&str) + 'static,
) {
    let on_event: EventHandler = Rc::new(on_event);
    let node = Spacer(modifier);
    let mount = remember(|| {
        let mut registry = host.registry.borrow_mut();
        let id = registry.allocate_id();
        registry.slots.push(Slot {
            id,
            node,
            kind: kind.to_owned(),
            value: value.to_owned(),
            on_event: Rc::clone(&on_event),
        });
        Mount {
            host: host.clone(),
            id,
        }
    });
    mount.update(|mount| {
        let mut registry = mount.host.registry.borrow_mut();
        if let Some(index) = registry.slots.iter().position(|slot| slot.id == mount.id) {
            if registry.slots[index].kind != kind {
                mount.id = registry.allocate_id();
            }
            let slot = &mut registry.slots[index];
            slot.id = mount.id;
            slot.node = node;
            if slot.kind != kind {
                kind.clone_into(&mut slot.kind);
            }
            if slot.value != value {
                value.clone_into(&mut slot.value);
            }
            slot.on_event = on_event;
        }
    });
}
