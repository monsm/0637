//! Re-export of the official `robotd` skill scheduler.
//!
//! The file is compiled directly from the upstream checkout. `#[path]` is
//! relative to this file's directory (`src/`), so three levels up is the
//! workspace root (`D:\AI\Duck`).

#[path = "../../../upstream/microduck/robotd/src/control.rs"]
mod inner;

pub use inner::*;

/// The idle look-around: the real duck glances, tilts its head and breathes
/// when nothing is happening. `robotd` owns the behaviour; compiling the same
/// file keeps the app's duck doing what the physical one does.
#[path = "../../../upstream/microduck/robotd/src/idle_head.rs"]
mod idle_head;

pub use idle_head::{IdleHead, IdleOffset};
