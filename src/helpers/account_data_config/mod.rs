use thiserror::Error;

mod global;
mod room;
mod utils;

pub use global::{GlobalConfig, GlobalConfigCarrierContent, Manager as GlobalConfigManager};
pub use room::{Manager as RoomConfigManager, RoomConfig, RoomConfigCarrierContent};

#[derive(Error, Debug)]
pub enum ConfigError {
    #[error("Encryption error: {0}")]
    Encryption(String),

    #[error("Serialization/deserialization error: {0}")]
    SerializeDeserialize(serde_json::Error),

    #[error("Error from the matrix SDK: {0}")]
    Sdk(#[from] matrix_sdk::Error),

    #[error("HTTP Error from the matrix SDK: {0}")]
    SdkHttp(#[from] matrix_sdk::HttpError),
}
