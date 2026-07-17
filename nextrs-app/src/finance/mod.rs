//! Canonical Streams/Forecast domain and libSQL repository.
//!
//! Integration from `AppContext` is deliberately small:
//!
//! 1. include `migrations/0002_streams_forecast.sql` in the central,
//!    checksum-verified migration runner;
//! 2. obtain a connection from `AppContext::connection()` and create
//!    [`FinanceRepository::new`] per operation;
//! 3. run [`FinanceRepository::bootstrap_defaults`] from an explicit bootstrap
//!    command, never from a Vercel cold start.

mod date;
mod error;
pub mod http;
mod models;
mod repository;

pub use date::IsoDate;
pub use error::{FinanceError, FinanceResult};
pub use models::*;
pub use repository::{FinanceRepository, verify_foreign_keys};

#[cfg(all(test, feature = "local-db"))]
mod tests;
