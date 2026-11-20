//! The handle that a tracker's create call makes.

use openxr as xr;
use xr::sys::Handle;

// Calls `create` with a null handle for it to fill, and returns the handle or the result that
// refused it. A runtime refuses a tracker whose permission the user has not granted.
pub(crate) fn created_handle<H: Handle>(
    create: impl FnOnce(&mut H) -> xr::sys::Result,
) -> Result<H, xr::sys::Result> {
    let mut handle = H::NULL;
    let result = create(&mut handle);
    if result.into_raw() < 0 {
        Err(result)
    } else {
        Ok(handle)
    }
}
