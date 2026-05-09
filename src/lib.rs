// rustc 1.94+ trips a query-depth overflow when computing async layouts in
// the matrix-sdk timeline future graph. matrix-rust-sdk PR #6489 raises the
// limit, but `recursion_limit` is per-crate and applies to the crate currently
// being compiled — so the consumer has to repeat it.
#![recursion_limit = "256"]

mod entity;
pub mod helpers;
mod init;
mod matrixlink;
mod persistence;
mod utils;

pub use entity::*;
pub use init::{InitConfig, InitError, LoginError, RestoreSessionError, init};
pub use matrixlink::CallbackError;
pub use matrixlink::MatrixLink;
pub use matrixlink::media::{Media, MediaAttachmentUploadPrepError};
pub use matrixlink::messaging::Messaging;
pub use matrixlink::reacting::Reacting;
pub use matrixlink::rooms::{JoinError, Rooms, TypingNoticeGuard};
pub use matrixlink::syncing::SyncError;
pub use matrixlink::threads::{ThreadGetMessagesParams, Threads};
pub use persistence::SessionPersistenceError;

// Re-exports

pub use matrix_sdk;
pub use mime;
