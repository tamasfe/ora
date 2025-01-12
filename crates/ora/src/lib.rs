//! Client library for the Ora job scheduler.
//!
//! It currently just re-exports everything from [`ora_client`].

pub use ora_client::*;

#[cfg(feature = "macros")]
pub use ora_client_macros::JobType;
