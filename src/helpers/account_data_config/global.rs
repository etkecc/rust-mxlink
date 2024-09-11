use std::future::Future;
use std::pin::Pin;

use matrix_sdk::ruma::api::client::config::set_global_account_data;
use matrix_sdk::ruma::events::{GlobalAccountDataEventContent, StaticEventContent};

use super::ConfigError;
use crate::helpers::encryption::Manager as EncryptionManager;
use crate::MatrixLink;

/// A trait that your global configuration should implement.
pub trait GlobalConfig: Clone + serde::Serialize + serde::de::DeserializeOwned {}

/// A trait that your room configuration "carrier content" struct should implement.
pub trait GlobalConfigCarrierContent:
    StaticEventContent + GlobalAccountDataEventContent + serde::de::DeserializeOwned
{
    fn new(payload: String) -> Self;

    fn payload(&self) -> &str;
}

/// Manages global configuration stored in account data in an encrypted manner.
///
/// Your configuration gets:
/// - serialized as a string
/// - potentially encrypted (via EncryptionManager), although the encryption key could be None to disable encryption
/// - wrapped into the "carrier content" struct
/// - stored in global account data with a key as specified on your "carrier content" struct
///
/// # Examples
///
/// ```no_run
/// use std::future::Future;
/// use std::pin::Pin;
///
/// use mxlink::matrix_sdk::ruma::events::macros::EventContent;
///
/// use mxlink::helpers::encryption::Manager as EncryptionManager;
/// use mxlink::helpers::{GlobalConfig, GlobalConfigCarrierContent, GlobalConfigManager};
///
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
/// #[ruma_event(type = "com.example.mybot.my_global_config", kind = GlobalAccountData)]
/// pub struct MyGlobalConfigCarrierContent {
///     pub payload: String,
/// }
///
/// impl GlobalConfigCarrierContent for MyGlobalConfigCarrierContent {
///     fn payload(&self) -> &str {
///         &self.payload
///     }
///
///     fn new(payload: String) -> Self {
///         Self { payload }
///     }
/// }
///
/// #[derive(Clone, Debug, Deserialize, Serialize)]
/// pub struct MyGlobalConfig {
///     pub some_setting: String,
///     pub another_setting: bool,
/// }
///
/// impl GlobalConfig for MyGlobalConfig {}
///
/// // Your own type alias
/// pub type MyGlobalConfigManager =
///     mxlink::helpers::account_data_config::GlobalConfigManager<MyGlobalConfig, MyGlobalConfigCarrierContent>;
///
/// async fn create_initial_global_config() -> MyGlobalConfig {
///     MyGlobalConfig {some_setting: "default".to_owned(), another_setting: false}
/// }
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let matrix_link:: mxlink:MatrixLink = todo!() // mxlink::init(..)
///     let encryption_manager: EncryptionManager = todo!; // EncryptionManager::new(..)
///
///     let initial_global_config_callback = || {
///         let future = create_initial_global_config();
///
///         // Explicitly box the future to match the expected type
///         Box::pin(future) as Pin<Box<dyn Future<Output = MyGlobalConfig> + Send>>
///     };
///
///     let global_config_manager: MyGlobalConfigManager = AccountDataGlobalConfigManager::new(
///         matrix_link.clone(),
///         encryption_manager.clone(),
///         initial_global_config_callback,
///     );
///
///      let global_config: MyGlobalConfig = global_config_manager.get_or_create().await?;
/// }
/// ```
pub struct Manager<ConfigType, ConfigCarrierContentType> {
    matrix_link: MatrixLink,
    encryption_manager: EncryptionManager,
    initial_global_config_callback:
        Box<dyn Fn() -> Pin<Box<dyn Future<Output = ConfigType> + Send>> + Send + Sync>,

    lock: tokio::sync::Mutex<()>,

    last_cached_config: Option<ConfigType>,

    // Markers to hold the generic types
    _marker_config: std::marker::PhantomData<ConfigType>,
    _marker_carrier: std::marker::PhantomData<ConfigCarrierContentType>,
}

impl<ConfigType, ConfigCarrierContentType> Manager<ConfigType, ConfigCarrierContentType>
where
    ConfigType: GlobalConfig,
    ConfigCarrierContentType: GlobalConfigCarrierContent,
{
    pub fn new<InitialGlobalConfigCallback>(
        matrix_link: MatrixLink,
        encryption_manager: EncryptionManager,
        initial_global_config_callback: InitialGlobalConfigCallback,
    ) -> Self
    where
        InitialGlobalConfigCallback:
            Fn() -> Pin<Box<dyn Future<Output = ConfigType> + Send>> + Send + Sync + 'static,
    {
        Self {
            matrix_link,
            encryption_manager,
            initial_global_config_callback: Box::new(initial_global_config_callback),

            lock: tokio::sync::Mutex::new(()),

            last_cached_config: None,

            _marker_config: std::marker::PhantomData,
            _marker_carrier: std::marker::PhantomData,
        }
    }

    #[tracing::instrument(skip_all, name = "global_config_get_or_create")]
    pub async fn get_or_create(&mut self) -> Result<ConfigType, ConfigError> {
        let start = std::time::Instant::now();

        tracing::debug!("Request for global config");

        let _lock = self.lock.lock().await;

        if let Some(config) = &self.last_cached_config {
            tracing::trace!("Returning existing cached global config..");
            return Ok(config.clone());
        }

        tracing::trace!("Fetching global config..");

        let account = self.matrix_link.client().account();
        let maybe_content = account.account_data::<ConfigCarrierContentType>().await?;

        let config = if let Some(raw_content) = maybe_content {
            tracing::trace!("Found existing global config: {:?}", raw_content);

            match raw_content.deserialize() {
                Ok(content) => {
                    let global_config = super::utils::parse_encrypted_config(
                        &self.encryption_manager,
                        content.payload(),
                    );

                    if let Some(global_config) = global_config {
                        tracing::trace!("Reusing existing global config");
                        global_config
                    } else {
                        tracing::warn!("Found existing global config, but failed decrypting/parsing it.. Making new..");
                        self.do_create_new_without_locking().await?
                    }
                }
                Err(err) => {
                    tracing::warn!(
                        "Failed parsing existing global config: {:?}. Creating new one",
                        err
                    );
                    self.do_create_new_without_locking().await?
                }
            }
        } else {
            self.do_create_new_without_locking().await?
        };

        self.last_cached_config = Some(config.clone());

        tracing::trace!("Returning global config after {:?}", start.elapsed());

        Ok(config)
    }

    async fn do_create_new_without_locking(&self) -> Result<ConfigType, ConfigError> {
        tracing::info!("Creating new global config");

        let config = (self.initial_global_config_callback)().await;

        self.persist_without_locking(&config).await?;

        Ok(config)
    }

    #[tracing::instrument(skip_all, name = "global_config_persist")]
    pub async fn persist(&mut self, config: &ConfigType) -> Result<(), ConfigError> {
        let _lock = self.lock.lock().await;

        self.persist_without_locking(config).await?;

        self.last_cached_config = Some(config.clone());

        Ok(())
    }

    async fn persist_without_locking(&self, config: &ConfigType) -> Result<(), ConfigError> {
        let config_json =
            serde_json::to_string(config).map_err(ConfigError::SerializeDeserialize)?;

        let config_json_encrypted = self
            .encryption_manager
            .encrypt_string(&config_json)
            .map_err(ConfigError::Encryption)?;

        let encrypted_config = ConfigCarrierContentType::new(config_json_encrypted);

        let user_id = self.matrix_link.user_id().clone();
        let client = self.matrix_link.client();

        let request = set_global_account_data::v3::Request::new(user_id, &encrypted_config)
            .map_err(ConfigError::SerializeDeserialize)?;

        client
            .send(request, None)
            .await
            .map_err(ConfigError::SdkHttp)?;

        Ok(())
    }
}
