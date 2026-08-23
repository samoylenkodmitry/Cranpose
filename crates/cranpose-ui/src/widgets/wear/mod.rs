//! Wear OS widgets.
//!
//! A round watch is not a small phone. Its list scales and fades its rows
//! towards the bezel, its scroll indicator is a curved arc at 3 o'clock rather
//! than a straight bar, its buttons are 52dp capsules with two label slots, and
//! all of it is laid out on a whole-pixel grid because half a pixel of drift
//! antialiases an edge the platform draws crisp.
//!
//! These are built against Cranpose's own text and colour model, not against
//! any app's. There is no theme system here and no ambient lookup: every widget
//! takes its colours in its spec (see [`theme::WearColors`]) and its geometry
//! from [`crate::density::Density`].
//!
//! The pieces, in dependency order:
//!
//! - [`density`] — the integral-layout grid everything else rounds against;
//! - [`color_appearance`] — `setLuminance`, the CAM16 round trip Wear derives
//!   its scroll-indicator colours with;
//! - [`theme`] — colour roles and the four Wear type-scale entries;
//! - [`scaling_list`] — the shrink-and-fade list, and the transform channel
//!   that makes it possible at all;
//! - [`scroll_indicator`] — the curved indicator;
//! - [`scaffold`] — a screen that owns one of each;
//! - [`list_header`], [`button`], [`switch_button`] — the row widgets.

pub mod button;
pub mod color_appearance;
pub mod list_header;
pub mod scaffold;
pub mod scaling_list;
pub mod scroll_indicator;
pub mod switch_button;
pub mod theme;

pub use button::*;
pub use color_appearance::*;
pub use list_header::*;
pub use scaffold::*;
pub use scaling_list::*;
pub use scroll_indicator::*;
pub use switch_button::*;
pub use theme::*;
