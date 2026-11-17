//! The platform's monotonic clock, as the runtime's times show it.

#[cfg(windows)]
mod performance_counter;
#[cfg(unix)]
mod timespec;

#[cfg(windows)]
pub(crate) use performance_counter::{monotonic, now};
#[cfg(unix)]
pub(crate) use timespec::{monotonic, now};
