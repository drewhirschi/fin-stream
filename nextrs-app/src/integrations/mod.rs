pub mod actions;
mod error;
pub mod http;
mod models;
mod repository;
mod tmo_write;

pub use error::{IntegrationError, IntegrationResult};
pub use models::{
    CapturedProviderRecord, IntegrationConnection, IntegrationConnectionView,
    MonarchCredentialRecord, NormalizedTmoPayment, PortfolioSnapshot, Setting, TmoAccount,
    TmoCredentialRecord, TmoImportLoan, TmoImportOverview, TmoImportPayment, TmoLoanListItem,
    TmoPaymentEventLink,
};
pub use repository::IntegrationRepository;
pub use tmo_write::{IntegrationWriteRepository, TmoSyncCapture, TmoSyncPersistence};

#[cfg(all(test, feature = "local-db"))]
mod tests;
