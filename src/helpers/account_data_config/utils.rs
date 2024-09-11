use crate::helpers::encryption::Manager as EncryptionManager;

pub(super) fn parse_encrypted_config<RawConfigType>(
    encryption_manager: &EncryptionManager,
    payload_json_encrypted: &str,
) -> Option<RawConfigType>
where
    RawConfigType: serde::de::DeserializeOwned,
{
    let payload_json = encryption_manager.decrypt_string(payload_json_encrypted);

    match payload_json {
        Err(err) => {
            tracing::error!("Failed decrypting config: {:?}", err);
            None
        }
        Ok(payload_json) => {
            let config = serde_json::from_str(&payload_json);

            match config {
                Err(err) => {
                    tracing::error!("Failed parsing config from JSON: {:?}", err);
                    None
                }
                Ok(config) => Some(config),
            }
        }
    }
}
