use crate::workspace_inbox::{LoanWorkspace, LoanWorkspacePhoto};
use serde::Serialize;

use super::{PhotoLocation, classify_photo, photo_route_url, safe_external_url};

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspaceFormValues {
    pub redfin_url: String,
    pub redfin_link: Option<String>,
    pub zillow_url: String,
    pub zillow_link: Option<String>,
    pub decision_status: String,
    pub target_contribution: Option<f64>,
    pub actual_contribution: Option<f64>,
    pub notes: String,
    pub updated_at: Option<String>,
}

impl From<Option<&LoanWorkspace>> for WorkspaceFormValues {
    fn from(workspace: Option<&LoanWorkspace>) -> Self {
        let redfin_url = workspace
            .and_then(|workspace| workspace.redfin_url.clone())
            .unwrap_or_default();
        let zillow_url = workspace
            .and_then(|workspace| workspace.zillow_url.clone())
            .unwrap_or_default();
        Self {
            redfin_link: safe_external_url(&redfin_url),
            zillow_link: safe_external_url(&zillow_url),
            redfin_url,
            zillow_url,
            decision_status: workspace
                .and_then(|workspace| workspace.decision_status.clone())
                .unwrap_or_default(),
            target_contribution: workspace.and_then(|workspace| workspace.target_contribution),
            actual_contribution: workspace.and_then(|workspace| workspace.actual_contribution),
            notes: workspace
                .and_then(|workspace| workspace.notes.clone())
                .unwrap_or_default(),
            updated_at: workspace.map(|workspace| workspace.updated_at.clone()),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct WorkspacePhotoView {
    pub id: i64,
    pub provider: String,
    pub caption: Option<String>,
    pub source_url: Option<String>,
    pub image_url: Option<String>,
    pub is_featured: bool,
}

impl From<LoanWorkspacePhoto> for WorkspacePhotoView {
    fn from(photo: LoanWorkspacePhoto) -> Self {
        let image_url = match classify_photo(&photo.image_url) {
            Ok(PhotoLocation::Stored(key)) => photo_route_url(&key).ok(),
            Ok(PhotoLocation::ExternalOnly) | Err(_) => None,
        };
        Self {
            id: photo.id,
            provider: photo.provider,
            caption: photo.caption,
            source_url: safe_external_url(&photo.source_url),
            image_url,
            is_featured: photo.is_featured,
        }
    }
}
