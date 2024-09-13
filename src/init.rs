use std::path::Path;

use matrix_sdk::encryption::{
    recovery::RecoveryError as MatrixRecoveryError, secret_storage::SecretStorageError,
    EncryptionSettings,
};
use matrix_sdk::{Client, ClientBuildError};

use thiserror::Error;

use rand::Rng;

use crate::entity::session::{ClientSession, FullSession};
use crate::matrixlink::MatrixLink;
use crate::persistence::Manager as PersistenceManager;
use crate::utils::is_potentially_transient_http_error;
use crate::SessionPersistenceError;
use crate::{LoginConfig, LoginCredentials, PersistenceConfig};

pub struct InitConfig {
    pub login: LoginConfig,
    pub persistence: PersistenceConfig,
}

impl InitConfig {
    pub fn new(login: LoginConfig, persistence: PersistenceConfig) -> Self {
        Self { login, persistence }
    }
}

#[derive(Error, Debug)]
pub enum LoginError {
    #[error("Error authenticating: {0}")]
    Auth(matrix_sdk::Error),

    #[error("Error building the client: {0}")]
    ClientBuild(matrix_sdk::ClientBuildError),

    #[error("Error persisting the session: {0}")]
    SessionPersistence(SessionPersistenceError),

    #[error("Error recovering encryption keys: {0}")]
    Recovery(RecoveryError),
}

#[derive(Error, Debug)]
pub enum InitError {
    #[error("Error creating a new login session: {0}")]
    Login(LoginError),

    #[error("Error restoring existing session: {0}")]
    RestoreSession(RestoreSessionError),

    #[error("Error purging existing database: {0}")]
    PurgeDatabase(std::io::Error),

    #[error("Whoami sanity check failed due to an invalid access token. You may need to delete all persisted data (session and database) and start fresh")]
    WhoAmISanityCheckFailed,

    #[error("Session_meta information in the client is missing")]
    SessionMetaMissing,
}

#[derive(Error, Debug)]
pub enum RestoreSessionError {
    #[error("Error persisting/restoring session: {0}")]
    SessionPersistence(SessionPersistenceError),

    #[error("Error building the client from the restored session: {0}")]
    ClientBuild(matrix_sdk::ClientBuildError),

    #[error("Error from the matrix SDK: {0}")]
    Sdk(matrix_sdk::Error),
}

#[derive(Error, Debug)]
pub enum RecoveryError {
    #[error(
        "Recovery resetting is not configured to be allowed, so there is nothing to do but give up"
    )]
    SecretMismatchWhileResetDisallowed,

    #[error("Error setting up encryption keys recovery: {0}")]
    InitialSetup(MatrixRecoveryError),

    #[error("Error resetting the backup: {0}")]
    Reset(MatrixRecoveryError),

    #[error("Failed to recover with an unknown error: {0}")]
    Other(String),
}

/// Initialize a new Matrix Link (wrapping a Matrix Client) either from an existing (persisted) session/data or by logging in anew.
pub async fn init(init_config: &InitConfig) -> Result<MatrixLink, InitError> {
    struct ClientState {
        client: Client,
        sync_token: Option<String>,
    }

    let mut client_state: Option<ClientState> = None;

    let persistence_manager = PersistenceManager::new(init_config.persistence.clone());

    if persistence_manager.has_existing_session() {
        tracing::info!(
            "Attempting to re-use previous session found in `{}`",
            init_config.persistence.session_file_path.to_string_lossy()
        );

        let (client, sync_token) =
            restore_session(&persistence_manager, &init_config.login.homeserver_url)
                .await
                .map_err(InitError::RestoreSession)?;

        client_state = Some(ClientState {
            client: client.clone(),
            sync_token,
        });

        perform_whoami_sanity_check(&client).await?;
    } else {
        // No session file. Let's make sure the database directory is empty too, so we can start a new session cleanly.

        if persistence_manager.has_existing_db_state_file() {
            tracing::warn!(
                "Found an existing database state file ({}), but no session file ({}). This may happen when a previous initialization attempt failed mid-way or if the session file was deleted subsequently. The only way to recover is to start fresh. Doing that now..",
                persistence_manager.db_state_file_path().to_string_lossy(),
                persistence_manager.session_file_path().to_string_lossy(),
            );

            persistence_manager
                .purge_database()
                .map_err(InitError::PurgeDatabase)?;

            tracing::info!("The old database has been purged successfully");
        }
    }

    let client_state = if let Some(client_state) = client_state {
        client_state
    } else {
        tracing::info!("Creating a brand new client");

        let client = login_and_recover(
            &init_config.login,
            &init_config.persistence.db_dir_path,
            &persistence_manager,
        )
        .await
        .map_err(InitError::Login)?;

        ClientState {
            client,
            sync_token: None,
        }
    };

    let Some(session_meta) = client_state.client.session_meta() else {
        return Err(InitError::SessionMetaMissing);
    };

    let own_user_id = session_meta.user_id.clone();

    Ok(MatrixLink::new(
        own_user_id,
        client_state.client,
        client_state.sync_token,
        persistence_manager,
    ))
}

/// Login with a new device and potentially recovers the encryption keys.
async fn login_and_recover(
    login_config: &LoginConfig,
    db_dir_path: &Path,
    persistence_manager: &PersistenceManager,
) -> Result<Client, LoginError> {
    let mut rng = rand::thread_rng();

    let passphrase: String = (&mut rng)
        .sample_iter(rand::distributions::Alphanumeric)
        .take(32)
        .map(char::from)
        .collect();

    let (client, client_session) =
        create_client_and_session(&login_config.homeserver_url, db_dir_path, passphrase)
            .await
            .map_err(LoginError::ClientBuild)?;

    let matrix_auth = client.matrix_auth();

    match &login_config.credentials {
        LoginCredentials::UserPassword(username, password) => {
            match matrix_auth
                .login_username(username, password)
                .initial_device_display_name(&login_config.device_display_name)
                .await
            {
                Ok(_) => {
                    tracing::info!("Logged in as {username}");
                }
                Err(err) => {
                    tracing::error!(?username, ?err, "Error logging in");
                    return Err(LoginError::Auth(err));
                }
            }
        }
    }

    if let Some(encryption_config) = &login_config.encryption {
        if let Some(recovery_passphrase) = &encryption_config.recovery_passphrase {
            recover(
                &client,
                recovery_passphrase,
                encryption_config.recovery_reset_allowed,
            )
            .await
            .map_err(LoginError::Recovery)?;
        }
    }

    let user_session = matrix_auth
        .session()
        .expect("A logged-in client should have a session");

    let full_session = FullSession {
        client_session,
        user_session,
        sync_token: None,
    };

    persistence_manager
        .persist_full_session(&full_session)
        .await
        .map_err(LoginError::SessionPersistence)?;

    Ok(client)
}

async fn perform_whoami_sanity_check(client: &Client) -> Result<(), InitError> {
    use std::time::Duration;
    use tokio::time::sleep;

    const INITIAL_DELAY: Duration = Duration::from_secs(2);
    const MAX_DELAY: Duration = Duration::from_secs(30);

    let mut delay = INITIAL_DELAY;

    loop {
        tracing::trace!("Performing whoami sanity check..");

        match client.whoami().await {
            Ok(_) => {
                tracing::info!("Whoami sanity check passed");
                return Ok(());
            }
            Err(err) => {
                if !is_potentially_transient_http_error(&err) {
                    tracing::error!(?err, "Whoami sanity check failed with a permanent error");

                    // This appears to be a permanent error, such as an invalid access token.
                    //
                    // Deleting the session file is not enough to restore us back to working order.
                    // We need to delete the database too, etc.
                    // Let's avoid doing that automatically.

                    return Err(InitError::WhoAmISanityCheckFailed);
                }

                tracing::warn!(?delay, "Whoami sanity check with a potentially-transient error.. Retrying after a delay..");

                sleep(delay).await;

                delay = std::cmp::min(delay * 2, MAX_DELAY);
            }
        }
    }
}

/// Recover the encryption keys for newly created client sessions.
/// See: https://matrix-org.github.io/matrix-rust-sdk/matrix_sdk/encryption/recovery/index.html
async fn recover(
    client: &Client,
    recovery_passphrase: &str,
    recovery_reset_allowed: bool,
) -> Result<(), RecoveryError> {
    tracing::info!("Running recovery...");

    let recovery = client.encryption().recovery();

    let Err(err) = recovery.recover(recovery_passphrase).await else {
        tracing::info!("Recovery completed successfully");
        return Ok(());
    };

    let err_result = Err(RecoveryError::Other(format!(
        "Failed to recover with an unknown error: {:?}",
        err
    )));

    if let MatrixRecoveryError::SecretStorage(secret_storage_err) = err {
        match secret_storage_err {
            SecretStorageError::MissingKeyInfo { key_id: _ } => {
                tracing::warn!("Missing recovery information (this may be a first login with recovery enabled). Creating a new recovery key");

                // We don't need this recovery key. We're using the passphrase to recover.
                let _recovery_key = recovery
                    .enable()
                    .wait_for_backups_to_upload()
                    .with_passphrase(recovery_passphrase)
                    .await
                    .map_err(RecoveryError::InitialSetup)?;

                tracing::info!("Recovery created");

                return Ok(());
            }
            // This happens when the `recovery_passphrase` is wrong.
            SecretStorageError::SecretStorageKey(
                matrix_sdk::crypto::secret_storage::DecodeError::Mac(err),
            ) => {
                tracing::error!(
                    "Failed to validate secret storage key (perhaps the key changed): {:?}",
                    err
                );

                if !recovery_reset_allowed {
                    return Err(RecoveryError::SecretMismatchWhileResetDisallowed);
                }

                tracing::info!("Resetting recovery key...");

                let reset_result = recovery
                    .reset_key()
                    .with_passphrase(recovery_passphrase)
                    .await;

                if let Err(err) = reset_result {
                    return Err(RecoveryError::Reset(err));
                }

                return Ok(());
            }
            _ => {}
        };
    }

    err_result
}

/// Build a new client.
async fn create_client_and_session(
    homeserver_url: &str,
    db_dir_path: &Path,
    passphrase: String,
) -> Result<(Client, ClientSession), ClientBuildError> {
    let client = build_client(homeserver_url, db_dir_path, passphrase.clone()).await?;

    Ok((
        client,
        ClientSession {
            homeserver: homeserver_url.to_owned(),
            db_path: db_dir_path.to_path_buf(),
            passphrase,
        },
    ))
}
/// Create a new client instance
async fn build_client(
    homeserver_url: &str,
    db_dir_path: &Path,
    passphrase: String,
) -> Result<Client, ClientBuildError> {
    Client::builder()
        .homeserver_url(homeserver_url)
        // We use the SQLite store, which is enabled by default. This is the crucial part to
        // persist the encryption setup.
        // Note that other store backends are available and you can even implement your own.
        .sqlite_store(db_dir_path, Some(&passphrase))
        .with_encryption_settings(EncryptionSettings {
            auto_enable_cross_signing: true,
            auto_enable_backups: true,
            backup_download_strategy: matrix_sdk::encryption::BackupDownloadStrategy::OneShot,
        })
        .build()
        .await
}

/// Restore a previous session and returns a client its last sync token
async fn restore_session(
    persistence_manager: &PersistenceManager,
    homeserver_url: &str,
) -> Result<(Client, Option<String>), RestoreSessionError> {
    let full_session = persistence_manager
        .read_full_session()
        .await
        .map_err(RestoreSessionError::SessionPersistence)?;

    // Build the client with the previous settings from the session.
    //
    // The only setting we ignore is the homeserver URL - we override this to allow people changing
    // the homeserver URL subsequently while continuing with their existing session.
    let client = build_client(
        homeserver_url,
        &full_session.client_session.db_path,
        full_session.client_session.passphrase.clone(),
    )
    .await
    .map_err(RestoreSessionError::ClientBuild)?;

    tracing::debug!(
        "Restoring session for {}…",
        full_session.user_session.meta.user_id
    );

    // Restore the Matrix user session.
    client
        .restore_session(full_session.user_session)
        .await
        .map_err(RestoreSessionError::Sdk)?;

    Ok((client, full_session.sync_token))
}
