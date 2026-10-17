#![deny(missing_docs)]
#![doc = include_str!("../README.md")]

mod geometry;
mod room;

pub use geometry::{Camera, Plane, RoomShape, Vec3, Wall, focal_length_for};
pub use room::{Room, RoomState, rememberRoomState};
