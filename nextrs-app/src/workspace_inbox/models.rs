use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LoanWorkspace {
    pub id: i64,
    pub connection_id: i64,
    pub loan_account: String,
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub decision_status: Option<String>,
    pub target_contribution: Option<f64>,
    pub actual_contribution: Option<f64>,
    pub notes: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoanWorkspaceDraft {
    pub connection_id: i64,
    pub loan_account: String,
    pub redfin_url: Option<String>,
    pub zillow_url: Option<String>,
    pub decision_status: Option<String>,
    pub target_contribution: Option<f64>,
    pub actual_contribution: Option<f64>,
    pub notes: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct LoanWorkspacePhoto {
    pub id: i64,
    pub connection_id: i64,
    pub loan_account: String,
    pub provider: String,
    pub caption: Option<String>,
    pub source_url: String,
    pub image_url: String,
    pub sort_order: i64,
    pub is_featured: bool,
    pub created_at: String,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LoanWorkspacePhotoDraft {
    pub connection_id: i64,
    pub loan_account: String,
    pub provider: String,
    pub caption: Option<String>,
    pub source_url: String,
    pub image_url: String,
    pub sort_order: i64,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReceivedEmail {
    pub id: i64,
    pub resend_email_id: String,
    pub from_address: String,
    pub to_addresses: String,
    pub subject: Option<String>,
    pub received_at: String,
    pub body_s3_key: Option<String>,
    pub body_content_type: Option<String>,
    pub loan_account: Option<String>,
    pub processing_state: String,
    pub error_message: Option<String>,
    pub raw_webhook_payload: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Inbox list projection with its attachment count computed in the same
/// database query. The underlying email remains the canonical typed record so
/// list and detail pages cannot drift on processing-state semantics.
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct InboxEmailListItem {
    pub email: ReceivedEmail,
    pub attachment_count: i64,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedEmailDraft {
    pub resend_email_id: String,
    pub from_address: String,
    /// The exact JSON array string received from the provider.
    pub to_addresses: String,
    pub subject: Option<String>,
    pub received_at: String,
    /// The exact provider webhook JSON. It is validated but never rewritten.
    pub raw_webhook_payload: Option<String>,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
pub struct ReceivedEmailAttachment {
    pub id: i64,
    pub email_id: i64,
    pub resend_attachment_id: String,
    pub filename: String,
    pub content_type: String,
    pub size_bytes: Option<i64>,
    pub s3_key: Option<String>,
    pub processing_state: String,
    pub created_at: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedEmailAttachmentDraft {
    pub email_id: i64,
    pub resend_attachment_id: String,
    pub filename: String,
    pub content_type: String,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct EmailUpsert {
    pub email: ReceivedEmail,
    pub inserted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AttachmentUpsert {
    pub attachment: ReceivedEmailAttachment,
    pub inserted: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReceivedEmailDetail {
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
}

/// Metadata returned after the database transaction commits. Object storage
/// cleanup is deliberately a separate provider-boundary operation.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct DeletedEmailMetadata {
    pub email: ReceivedEmail,
    pub attachments: Vec<ReceivedEmailAttachment>,
}
