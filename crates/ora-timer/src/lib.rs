//! An async timer implementation based on <https://github.com/Bathtor/rust-hash-wheel-timer>
//! used by the Ora scheduler.
#![cfg_attr(not(feature = "std"), no_std)]
#![warn(clippy::pedantic, missing_docs)]
#![allow(clippy::module_name_repetitions, clippy::ignored_unit_patterns)]

extern crate alloc;

pub mod resolution;
#[cfg(feature = "std")]
pub mod timer;
pub mod wheel;

#[cfg(feature = "std")]
pub use timer::{Timer, TimerHandle};
