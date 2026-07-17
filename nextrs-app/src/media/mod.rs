mod error;
pub mod http;
mod key;
mod models;
mod service;

pub use error::{MediaError, MediaResult};
pub use key::{PhotoLocation, classify_photo, photo_route_url, safe_external_url};
pub use models::{WorkspaceFormValues, WorkspacePhotoView};
#[cfg(all(test, feature = "local-db"))]
pub(crate) use service::MediaBackend;
pub use service::{HeadObject, MediaService, PresignedUpload, UploadIntent, UploadIntentDraft};

#[cfg(all(test, feature = "local-db"))]
mod tests;
