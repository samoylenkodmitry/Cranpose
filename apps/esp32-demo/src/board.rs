//! The boards the demo runs on, one for each cargo feature. Each opens its
//! display and touch panel and gives the display settings that suit its chip.
//! Without a board feature the demo runs on QEMU's virtual panel.

#[cfg(all(feature = "es3c35p", feature = "e32r35t"))]
compile_error!("choose one board feature: es3c35p or e32r35t");

#[cfg(feature = "e32r35t")]
mod e32r35t;
#[cfg(feature = "es3c35p")]
mod es3c35p;
#[cfg(not(any(feature = "es3c35p", feature = "e32r35t")))]
mod qemu;

#[cfg(feature = "e32r35t")]
pub use e32r35t::{TITLE, config, open};
#[cfg(feature = "es3c35p")]
pub use es3c35p::{TITLE, config, open};
#[cfg(not(any(feature = "es3c35p", feature = "e32r35t")))]
pub use qemu::{TITLE, config, open};
