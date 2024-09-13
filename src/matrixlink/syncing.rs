use std::sync::Arc;
use std::time::Duration;

use matrix_sdk::{config::SyncSettings, ruma::api::client::filter::FilterDefinition, LoopCtrl};

use thiserror::Error;

use crate::utils::is_potentially_transient_sdk_error;
use crate::SessionPersistenceError;

const SYNC_INITIAL_DELAY_DURATION: Duration = Duration::from_secs(3);
const SYNC_MAX_DELAY_DURATION: Duration = Duration::from_secs(30);

#[derive(Error, Debug)]
pub enum SyncError {
    #[error("Error from the matrix SDK: {0}")]
    Sdk(#[from] matrix_sdk::Error),

    #[error("Error persisting/restoring session: {0}")]
    SessionPersistence(SessionPersistenceError),
}

#[derive(Clone)]
pub struct Syncing {
    matrix_link: super::MatrixLink,
}

impl Syncing {
    pub(super) fn new(matrix_link: super::MatrixLink) -> Self {
        Self { matrix_link }
    }

    /// Setup the client to listen to new messages.
    pub async fn start(&self) -> Result<(), SyncError> {
        // Enable room members lazy-loading, it will speed up the initial sync a lot
        // with accounts in lots of rooms.
        // See <https://spec.matrix.org/v1.6/client-server-api/#lazy-loading-room-members>.
        let filter = FilterDefinition::with_lazy_loading();

        let mut sync_settings = SyncSettings::default().filter(filter.into());

        // We restore the sync where we left.
        if let Some(sync_token) = &self.matrix_link.inner.initial_sync_token {
            sync_settings = sync_settings.token(sync_token);
        }

        let delay = Arc::new(tokio::sync::Mutex::new(SYNC_INITIAL_DELAY_DURATION));

        let persistence_manager = &self.matrix_link.inner.persistence_manager;

        tracing::info!("Syncing..");

        self.matrix_link
            .inner
            .client
            .sync_with_result_callback(sync_settings, {
                let delay = Arc::clone(&delay);
                move |sync_result| {
                    let delay = Arc::clone(&delay);
                    async move {
                        match sync_result {
                            Ok(response) => {
                                // Reset delay on successful sync
                                let mut current_delay = delay.lock().await;
                                *current_delay = SYNC_INITIAL_DELAY_DURATION;

                                // We persist the token each time to be able to restore our session
                                if let Err(err) = persistence_manager
                                    .persist_sync_token(response.next_batch.clone())
                                    .await
                                {
                                    return Err(matrix_sdk::Error::UnknownError(err.into()));
                                }

                                Ok(LoopCtrl::Continue)
                            }
                            Err(err) => {
                                if !is_potentially_transient_sdk_error(&err) {
                                    tracing::error!(?err, "Sync failed with a permanent error");
                                    return Err(err);
                                }

                                let mut current_delay = delay.lock().await;

                                tracing::warn!(
                                    ?err,
                                    ?current_delay,
                                    "A potentially-transient error occurred during sync. Retrying after delay.."
                                );

                                tokio::time::sleep(*current_delay).await;

                                *current_delay = std::cmp::min(*current_delay * 2, SYNC_MAX_DELAY_DURATION);

                                Ok(LoopCtrl::Continue)
                            }
                        }
                    }
                }
            })
            .await
            .map_err(SyncError::Sdk)?;

        Ok(())
    }
}
