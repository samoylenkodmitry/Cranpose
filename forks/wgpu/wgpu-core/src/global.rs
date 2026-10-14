use alloc::{borrow::ToOwned as _, sync::Arc};
use core::fmt;

use crate::{
    hub::{Hub, HubReport},
    instance::{Instance, Surface},
    registry::{Registry, RegistryReport},
    resource_log,
};

#[derive(Debug, PartialEq, Eq)]
pub struct GlobalReport {
    pub surfaces: RegistryReport,
    pub hub: HubReport,
}

impl GlobalReport {
    pub fn surfaces(&self) -> &RegistryReport {
        &self.surfaces
    }
    pub fn hub_report(&self) -> &HubReport {
        &self.hub
    }
}

pub struct Global {
    pub(crate) surfaces: Registry<Arc<Surface>>,
    pub(crate) hub: Hub,
    // the instance must be dropped last
    pub instance: Instance,
}

impl Global {
    pub fn new(
        name: &str,
        instance_desc: wgt::InstanceDescriptor,
        telemetry: Option<hal::Telemetry>,
    ) -> Self {
        profiling::scope!("Global::new");
        Self {
            instance: Instance::new(name, instance_desc, telemetry),
            surfaces: Registry::new(),
            hub: Hub::new(),
        }
    }

    /// # Safety
    ///
    /// Refer to the creation of wgpu-hal Instance for every backend.
    pub unsafe fn from_hal_instance<A: hal::Api>(name: &str, hal_instance: A::Instance) -> Self {
        profiling::scope!("Global::new");

        Self {
            instance: Instance::from_hal_instance::<A>(name.to_owned(), hal_instance),
            surfaces: Registry::new(),
            hub: Hub::new(),
        }
    }

    /// # Safety
    ///
    /// - The raw instance handle returned must not be manually destroyed.
    pub unsafe fn instance_as_hal<A: hal::Api>(&self) -> Option<&A::Instance> {
        unsafe { self.instance.as_hal::<A>() }
    }

    /// # Safety
    ///
    /// - The raw handles obtained from the Instance must not be manually destroyed
    pub unsafe fn from_instance(instance: Instance) -> Self {
        profiling::scope!("Global::new");
        Self {
            instance,
            surfaces: Registry::new(),
            hub: Hub::new(),
        }
    }

    pub fn generate_report(&self) -> GlobalReport {
        GlobalReport {
            surfaces: self.surfaces.generate_report(),
            hub: self.hub.generate_report(),
        }
    }
}

impl fmt::Debug for Global {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Global").finish()
    }
}

impl Drop for Global {
    fn drop(&mut self) {
        profiling::scope!("Global::drop");
        resource_log!("Global::drop");
    }
}

// Cranpose fork: the compiler proves an auto trait through every field, and
// for `Global` that proof runs through the nested resource registries of
// `Hub`, deeper than the default recursion limit, in every crate that needs a
// wgpu object to be `Send` or `Sync` (nightly reports it as a future error).
// These impls end the proof here. `_test_send_sync` checks each field, so
// they hold exactly where the derived impls would.
#[cfg(send_sync)]
// SAFETY: every field is `Send`, which `_test_send_sync` checks.
unsafe impl Send for Global {}
#[cfg(send_sync)]
// SAFETY: every field is `Sync`, which `_test_send_sync` checks.
unsafe impl Sync for Global {}

#[cfg(send_sync)]
fn _test_send_sync(global: &Global) {
    fn test_internal<T: Send + Sync>(_: &T) {}
    let Global {
        surfaces,
        hub,
        instance,
    } = global;
    test_internal(surfaces);
    test_internal(hub);
    test_internal(instance);
}
