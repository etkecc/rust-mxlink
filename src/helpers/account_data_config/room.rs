use std::future::Future;
use std::pin::Pin;

use matrix_sdk::ruma::api::client::config::set_room_account_data;
use matrix_sdk::ruma::events::{
    RoomAccountDataEvent, RoomAccountDataEventContent, StaticEventContent,
};
use matrix_sdk::ruma::OwnedUserId;
use matrix_sdk::Room;

use quick_cache::sync::Cache;

use super::ConfigError;
use crate::helpers::encryption::Manager as EncryptionManager;

/// A trait that your room configuration should implement.
pub trait RoomConfig: Clone + serde::Serialize + serde::de::DeserializeOwned {}

/// A trait that your room configuration "carrier content" struct should implement.
pub trait RoomConfigCarrierContent:
    StaticEventContent + RoomAccountDataEventContent + serde::de::DeserializeOwned
{
    fn new(payload: String) -> Self;

    fn payload(&self) -> &str;
}

/// Manages per-room configuration stored in account data in an encrypted manner.
///
/// Your configuration gets:
/// - serialized as a string
/// - potentially encrypted (via EncryptionManager), although the encryption key could be None to disable encryption
/// - wrapped into the "carrier content" struct
/// - stored in room account data with a key as specified on your "carrier content" struct
///
/// # Examples
///
/// ```no_run
/// use std::future::Future;
/// use std::pin::Pin;
///
/// use mxlink::matrix_sdk::ruma::events::macros::EventContent;
/// use mxlink::matrix_sdk::Room;
///
/// use mxlink::helpers::encryption::Manager as EncryptionManager;
/// use mxlink::helpers::{RoomConfig, RoomConfigCarrierContent, RoomConfigManager};
///
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Clone, Debug, Deserialize, Serialize, EventContent)]
/// #[ruma_event(type = "com.example.mybot.my_room_config", kind = RoomAccountData)]
/// pub struct MyRoomConfigCarrierContent {
///     pub payload: String,
/// }
///
/// impl RoomConfigCarrierContent for MyRoomConfigCarrierContent {
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
/// pub struct MyRoomConfig {
///     pub some_setting: String,
///     pub another_setting: bool,
/// }
///
/// impl RoomConfig for MyRoomConfig {}
///
/// // Your own type alias
/// pub type MyRoomConfigManager =
///     mxlink::helpers::account_data_config::RoomConfigManager<MyRoomConfig, MyRoomConfigCarrierContent>;
///
/// async fn create_initial_room_config(room: Room) -> MyRoomConfig {
///     // Feel free to make this dynamic based on the room
///     MyRoomConfig {some_setting: "default".to_owned(), another_setting: false}
/// }
///
/// #[tokio::main]
/// async fn main() -> anyhow::Result<()> {
///     let matrix_link:: mxlink:MatrixLink = todo!() // mxlink::init(..)
///     let encryption_manager: EncryptionManager = todo!; // EncryptionManager::new(..)
///
///     let initial_room_config_callback = |room: Room| {
///         let future = create_initial_room_config(room);
///
///         // Explicitly box the future to match the expected type
///         Box::pin(future) as Pin<Box<dyn Future<Output = MyGlobalConfig> + Send>>
///     };
///
///     let room_config_manager: MyRoomConfigManager = AccountDataRoomConfigManager::new(
///         matrix_link.user_id().clone(),
///         encryption_manager.clone(),
///         initial_room_config_callback,
///     );
///
///     let room: matrix_sdk::Room = todo!();
///
///     let room_config: MyRoomConfig = room_config_manager.get_or_create_for_room(room).await?;
/// }
/// ```
pub struct Manager<ConfigType, ConfigCarrierContentType> {
    user_id: OwnedUserId,
    encryption_manager: EncryptionManager,

    initial_room_config_callback:
        Box<dyn Fn(Room) -> Pin<Box<dyn Future<Output = ConfigType> + Send>> + Send + Sync>,

    lru_cache: Option<Cache<String, ConfigType>>,

    // Protects all room config operations.
    // Using a per-room lock would be better, but increasing complexity
    // is not necessary at this point.
    lock: tokio::sync::Mutex<()>,

    // Markers to hold the generic types
    _marker_config: std::marker::PhantomData<ConfigType>,
    _marker_carrier: std::marker::PhantomData<ConfigCarrierContentType>,
}

impl<ConfigType, ConfigCarrierContentType> Manager<ConfigType, ConfigCarrierContentType>
where
    ConfigType: RoomConfig,
    ConfigCarrierContentType: RoomConfigCarrierContent,
{
    pub fn new<InitialRoomConfigCallback>(
        user_id: OwnedUserId,
        encryption_manager: EncryptionManager,
        initial_room_config_callback: InitialRoomConfigCallback,
        lru_cache_size: Option<usize>,
    ) -> Self
    where
        InitialRoomConfigCallback:
            Fn(Room) -> Pin<Box<dyn Future<Output = ConfigType> + Send>> + Send + Sync + 'static,
    {
        let lru_cache = lru_cache_size.map(Cache::new);

        Self {
            user_id,
            encryption_manager,
            initial_room_config_callback: Box::new(initial_room_config_callback),

            lru_cache,

            lock: tokio::sync::Mutex::new(()),

            _marker_config: std::marker::PhantomData,
            _marker_carrier: std::marker::PhantomData,
        }
    }

    #[tracing::instrument(skip_all, name="room_config_get_or_create", fields(room_id = room.room_id().as_str()))]
    pub async fn get_or_create_for_room(&self, room: &Room) -> Result<ConfigType, ConfigError> {
        let start = std::time::Instant::now();

        tracing::debug!("Request for room config");

        let _lock = self.lock.lock().await;

        let Some(lru_cache) = &self.lru_cache else {
            let result = self
                .do_get_or_create_for_room_without_locking_and_caching(room)
                .await;

            tracing::trace!(
                "Returning uncached room config (after {:?}) for room {}",
                start.elapsed(),
                room.room_id()
            );

            return result;
        };

        let guard = lru_cache
            .get_value_or_guard_async(room.room_id().as_str())
            .await;

        match guard {
            Ok(config) => {
                tracing::trace!("Returning existing cached room config..");
                return Ok(config);
            }
            Err(guard) => {
                let config = self
                    .do_get_or_create_for_room_without_locking_and_caching(room)
                    .await?;

                let _ = guard.insert(config.clone());

                tracing::trace!(
                    "Returning now-cached room config (after {:?}) for room {}",
                    start.elapsed(),
                    room.room_id()
                );

                return Ok(config);
            }
        }
    }

    async fn do_get_or_create_for_room_without_locking_and_caching(
        &self,
        room: &Room,
    ) -> Result<ConfigType, ConfigError> {
        tracing::trace!("Fetching config for room: {}..", room.room_id());

        let data = room
            .account_data_static::<ConfigCarrierContentType>()
            .await?;

        let room_config: ConfigType = match data {
            Some(raw_event) => {
                tracing::trace!("Found existing room config: {:?}", raw_event);

                let event: serde_json::Result<RoomAccountDataEvent<ConfigCarrierContentType>> =
                    raw_event.deserialize();

                match event {
                    Ok(event) => {
                        let room_config = super::utils::parse_encrypted_config(
                            &self.encryption_manager,
                            event.content.payload(),
                        );

                        if let Some(room_config) = room_config {
                            tracing::trace!("Reusing existing room config");
                            room_config
                        } else {
                            tracing::warn!("Found existing room config, but failed decrypting/parsing it.. Making new..");
                            self.do_create_new_for_room_without_locking(room).await?
                        }
                    }
                    Err(err) => {
                        tracing::warn!("Failed parsing existing room config: {:?}", err);
                        self.do_create_new_for_room_without_locking(room).await?
                    }
                }
            }
            None => self.do_create_new_for_room_without_locking(room).await?,
        };

        tracing::trace!("Returning room config");

        Ok(room_config)
    }

    #[tracing::instrument(skip_all, name="room_config_create_new", fields(room_id = room.room_id().as_str()))]
    pub async fn create_new_for_room(&self, room: &Room) -> Result<ConfigType, ConfigError> {
        let _lock = self.lock.lock().await;

        self.do_create_new_for_room_without_locking(room).await
    }

    async fn do_create_new_for_room_without_locking(
        &self,
        room: &Room,
    ) -> Result<ConfigType, ConfigError> {
        tracing::info!("Creating new room config");

        let config = (self.initial_room_config_callback)(room.clone()).await;

        tracing::trace!("Persisting new room config");

        self.persist_without_locking(room, &config).await?;

        tracing::trace!("Persisted new room config");

        Ok(config)
    }

    #[tracing::instrument(skip_all, name="room_config_persist", fields(room_id = room.room_id().as_str()))]
    pub async fn persist(&self, room: &Room, config: &ConfigType) -> Result<(), ConfigError> {
        let _lock = self.lock.lock().await;

        self.persist_without_locking(room, config).await?;

        if let Some(lru_cache) = &self.lru_cache {
            let _ = lru_cache.replace(room.room_id().as_str().to_owned(), config.clone(), false);
        }

        Ok(())
    }

    async fn persist_without_locking(
        &self,
        room: &Room,
        config: &ConfigType,
    ) -> Result<(), ConfigError> {
        let config_json =
            serde_json::to_string(config).map_err(ConfigError::SerializeDeserialize)?;

        let config_json_encrypted = self
            .encryption_manager
            .encrypt_string(&config_json)
            .map_err(ConfigError::Encryption)?;

        let encrypted_config = ConfigCarrierContentType::new(config_json_encrypted);

        let request = set_room_account_data::v3::Request::new(
            self.user_id.clone(),
            room.room_id().to_owned(),
            &encrypted_config,
        )
        .map_err(ConfigError::SerializeDeserialize)?;

        room.client()
            .send(request, None)
            .await
            .map_err(ConfigError::SdkHttp)?;

        Ok(())
    }
}
