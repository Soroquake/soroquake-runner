//! Soroquake: stateful fuzzing and invariant testing for Soroban smart contracts.
//!
//! Write a [`Harness`]: how to set up your contracts, how to pick the next call,
//! how to apply it, and which [`Invariant`]s must hold after every step. Then
//! call [`run`] from a `#[test]`. Soroquake explores random call sequences until
//! the time budget runs out, shrinks every failure to the fewest calls that
//! still break the invariant, and streams the campaign to the dashboard when
//! `SOROQUAKE_TOKEN` is set.

pub mod campaign;
pub mod check;
pub mod config;
pub mod fork;
pub mod lab;
#[cfg(feature = "report")]
mod report;
pub mod rng;

pub use campaign::{run, Failure, Harness, Invariant, Report, Step};
pub use check::Check;
pub use config::Config;
pub use fork::Fork;
pub use lab::{Actor, LabAction, LabCase, LabResult, LabStatus};
pub use rng::SeededRng;

/// Convenience re-exports for harness authors.
pub mod prelude {
    pub use crate::campaign::{run, Harness, Invariant, Step};
    pub use crate::check;
    pub use crate::check::Check;
    pub use crate::lab::{Actor, LabAction};
    pub use crate::rng::SeededRng;
}
