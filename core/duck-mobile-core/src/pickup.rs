//! `[pickup]`: robotd's pause-while-held watch, compiled from upstream.
//!
//! The classifier, its feature row, the hysteresis and the pause ramp are
//! `duck_control::pickup`; `robotd/src/pickup.rs` is the loop's side. Which
//! ticks are allowed to be scored is this crate's loop, exactly as it is
//! `robotd`'s. A digital duck held in a hand must stop thrashing for the same
//! reason the physical one does.

#[path = "../../../upstream/microduck/robotd/src/pickup.rs"]
mod inner;

pub use inner::{Edge, Watch};
