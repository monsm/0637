//! The parameter crate, named the way `robotd`'s own modules import it.
//!
//! `robotd/src/pickup.rs` speaks `crate::params::PickupParams`. This crate
//! compiles that file verbatim, so the alias has to exist here too — the
//! alternative (a copied struct) would drift the moment a threshold moved.

pub use robotd_params::*;
