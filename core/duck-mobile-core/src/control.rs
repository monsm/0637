//! Re-export of the official `robotd` skill scheduler.
//!
//! The file is compiled directly from the upstream checkout. `#[path]` is
//! relative to this file's directory (`src/`), so three levels up is the
//! workspace root (`D:\AI\Duck`).

#[path = "../../../upstream/microduck/robotd/src/control.rs"]
mod inner;

pub use inner::*;
