mod error;
pub mod http;
mod models;
mod repository;

pub use error::{WorkspaceInboxError, WorkspaceInboxResult};
pub use models::{
    AttachmentUpsert, DeletedEmailMetadata, EmailUpsert, InboxEmailListItem, LoanWorkspace,
    LoanWorkspaceDraft, LoanWorkspacePhoto, LoanWorkspacePhotoDraft, ReceivedEmail,
    ReceivedEmailAttachment, ReceivedEmailAttachmentDraft, ReceivedEmailDetail, ReceivedEmailDraft,
};
pub use repository::WorkspaceInboxRepository;

#[cfg(all(test, feature = "local-db"))]
mod tests;
