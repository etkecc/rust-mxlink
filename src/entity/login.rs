pub enum Credentials {
    UserPassword(String, String),
}

pub struct Encryption {
    /// The recovery passphrase to use for the recovery module (https://matrix-org.github.io/matrix-rust-sdk/matrix_sdk/encryption/recovery/index.html).
    /// If this is `None`, the recovery module will not be used.
    pub(crate) recovery_passphrase: Option<String>,

    pub(crate) recovery_reset_allowed: bool,
}

impl Encryption {
    pub fn new(recovery_passphrase: Option<String>, recovery_reset_allowed: bool) -> Self {
        Self {
            recovery_passphrase,
            recovery_reset_allowed,
        }
    }
}

pub struct Config {
    pub(crate) homeserver_url: String,

    pub(crate) credentials: Credentials,

    pub(crate) encryption: Option<Encryption>,

    pub(crate) device_display_name: String,
}

impl Config {
    pub fn new(
        homeserver_url: String,
        credentials: Credentials,
        encryption: Option<Encryption>,
        device_display_name: String,
    ) -> Self {
        Self {
            homeserver_url,
            credentials,
            encryption,
            device_display_name,
        }
    }
}
