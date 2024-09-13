use std::sync::Arc;
use std::collections::HashMap;

use tokio::sync::Mutex;

use matrix_sdk::Client;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId};

use thiserror::Error;

use crate::persistence::Manager as PersistenceManager;
use crate::SyncError;

pub(crate) mod media;
pub(crate) mod messaging;
pub(crate) mod reacting;
pub(crate) mod rooms;
pub(crate) mod syncing;
pub(crate) mod threads;

#[derive(Error, Debug)]
pub enum CallbackError {
    #[error("Error from the matrix SDK: {0}")]
    Sdk(#[from] matrix_sdk::Error),

    #[error("Unknown error: {0}")]
    Unknown(Box<dyn std::error::Error + Send + Sync>),
}

#[derive(Debug)]
struct MatrixLinkInner {
    user_id: OwnedUserId,
    client: Client,
    initial_sync_token: Option<String>,
    persistence_manager: PersistenceManager,

    typing_notices: Mutex<HashMap<OwnedRoomId, Arc<Mutex<u32>>>>,
}

/// MatrixLink represents a connection to a Matrix server.
/// It wraps a matrix_sdk Client and provides some convenience functions for working with it.
///
/// All of the state is held in an `Arc` so the `MatrixLink` can be cloned freely.
#[derive(Debug, Clone)]
pub struct MatrixLink {
    inner: Arc<MatrixLinkInner>,
}

impl MatrixLink {
    pub fn new(
        user_id: OwnedUserId,
        client: Client,
        initial_sync_token: Option<String>,
        persistence_manager: PersistenceManager,
    ) -> Self {
        Self {
            inner: Arc::new(MatrixLinkInner {
                user_id,
                client,
                initial_sync_token,
                persistence_manager,
                typing_notices: Mutex::new(HashMap::new()),
            }),
        }
    }

    pub fn user_id(&self) -> &OwnedUserId {
        &self.inner.user_id
    }

    pub fn client(&self) -> Client {
        self.inner.client.clone()
    }

    pub fn messaging(&self) -> messaging::Messaging {
        messaging::Messaging::new(self.clone())
    }

    pub fn media(&self) -> media::Media {
        media::Media::new()
    }

    pub fn reacting(&self) -> reacting::Reacting {
        reacting::Reacting::new(self.clone())
    }

    pub fn rooms(&self) -> rooms::Rooms {
        rooms::Rooms::new(self.clone())
    }

    pub fn threads(&self) -> threads::Threads {
        threads::Threads::new(self.clone())
    }

    /// Starts the client (listening for events, etc.)
    pub async fn start(&self) -> Result<(), SyncError> {
        syncing::Syncing::new(self.clone()).start().await
    }
}
