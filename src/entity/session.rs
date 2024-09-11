use std::path::PathBuf;

use matrix_sdk::matrix_auth::MatrixSession;
use serde::{Deserialize, Serialize};

/// The data needed to re-build a client.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct ClientSession {
    /// The URL of the homeserver of the user.
    pub(crate) homeserver: String,

    /// The path of the database.
    pub(crate) db_path: PathBuf,

    /// The passphrase of the database.
    pub(crate) passphrase: String,
}

/// The full session to persist.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct FullSession {
    /// The data to re-build the client.
    pub(crate) client_session: ClientSession,

    /// The Matrix user session.
    pub(crate) user_session: MatrixSession,

    /// The latest sync token.
    ///
    /// It is only needed to persist it when using `Client::sync_once()` and we
    /// want to make our syncs faster by not receiving all the initial sync
    /// again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sync_token: Option<String>,
}
