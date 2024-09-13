use std::path::PathBuf;

use tokio::fs;

use thiserror::Error;

use crate::entity::session::FullSession;
use crate::helpers::encryption::Manager as EncryptionManager;
use crate::PersistenceConfig;

#[derive(Error, Debug)]
pub enum SessionPersistenceError {
    #[error("IO error: {0}")]
    Io(std::io::Error),

    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Serialization/deserialization error: {0}")]
    SerializeDeserialize(serde_json::Error),
}

impl From<SessionPersistenceError> for matrix_sdk::Error {
    fn from(val: SessionPersistenceError) -> Self {
        matrix_sdk::Error::UnknownError(Box::new(val))
    }
}

#[derive(Debug)]
pub struct Manager {
    config: PersistenceConfig,

    encryption_manager: EncryptionManager,
}

impl Manager {
    pub fn new(config: PersistenceConfig) -> Self {
        let encryption_manager = EncryptionManager::new(config.session_encryption_key.clone());

        Self {
            config,
            encryption_manager,
        }
    }

    pub(crate) fn session_file_path(&self) -> PathBuf {
        self.config.session_file_path.clone()
    }

    pub(crate) fn db_state_file_path(&self) -> PathBuf {
        self.config.db_dir_path.join("matrix-sdk-state.sqlite3")
    }

    pub(crate) fn has_existing_session(&self) -> bool {
        self.session_file_path().exists()
    }

    pub(crate) fn has_existing_db_state_file(&self) -> bool {
        self.db_state_file_path().exists()
    }

    pub(crate) fn purge_database(&self) -> Result<(), std::io::Error> {
        let base_path = self.config.db_dir_path.clone();

        for entry in std::fs::read_dir(base_path)? {
            let entry = entry?;
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            // Out of precaution, we'll only be deleting *.sqlite3 files
            if !path.extension().map_or(false, |ext| ext == "sqlite3") {
                continue;
            }

            std::fs::remove_file(path)?;
        }

        Ok(())
    }

    pub(crate) async fn read_full_session(&self) -> Result<FullSession, SessionPersistenceError> {
        let serialized_potentially_encrypted_session =
            fs::read_to_string(&self.config.session_file_path)
                .await
                .map_err(SessionPersistenceError::Io)?;

        let serialized_session = self
            .encryption_manager
            .decrypt_string(&serialized_potentially_encrypted_session)
            .map_err(SessionPersistenceError::Encryption)?;

        let full_sesson: FullSession = serde_json::from_str(&serialized_session)
            .map_err(SessionPersistenceError::SerializeDeserialize)?;

        Ok(full_sesson)
    }

    /// Persist the sync token for a future session.
    /// Note that this is needed only when using `sync_once`. Other sync methods get
    /// the sync token from the store.
    pub(crate) async fn persist_sync_token(
        &self,
        sync_token: String,
    ) -> Result<(), SessionPersistenceError> {
        let mut full_session = self.read_full_session().await?;

        full_session.sync_token = Some(sync_token);

        self.persist_full_session(&full_session).await?;

        Ok(())
    }

    pub(crate) async fn persist_full_session(
        &self,
        full_session: &FullSession,
    ) -> Result<(), SessionPersistenceError> {
        let serialized_session = serde_json::to_string(full_session)
            .map_err(SessionPersistenceError::SerializeDeserialize)?;

        let serialized_potentially_encrypted_session = self
            .encryption_manager
            .encrypt_string(&serialized_session)
            .map_err(SessionPersistenceError::Encryption)?;

        fs::write(
            &self.config.session_file_path,
            serialized_potentially_encrypted_session,
        )
        .await
        .map_err(SessionPersistenceError::Io)?;

        Ok(())
    }
}
