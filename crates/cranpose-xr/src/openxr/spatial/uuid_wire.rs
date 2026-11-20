//! The identifiers of the runtime and of the app, which hold the same 16 bytes.

use openxr as xr;

use crate::Uuid;

pub(crate) const fn uuid_of(wire: &xr::sys::Uuid) -> Uuid {
    Uuid::from_bytes(wire.data)
}

pub(crate) const fn wire_of(uuid: Uuid) -> xr::sys::Uuid {
    xr::sys::Uuid {
        data: *uuid.as_bytes(),
    }
}
