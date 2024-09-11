use crate::helpers::encryption::EncryptionKey;

#[derive(Debug, Clone)]
pub struct Config {
    pub(crate) session_file_path: std::path::PathBuf,
    pub(crate) session_encryption_key: Option<EncryptionKey>,
    pub(crate) db_dir_path: std::path::PathBuf,
}

impl Config {
    pub fn new(
        session_file_path: std::path::PathBuf,
        session_encryption_key: Option<EncryptionKey>,
        db_dir_path: std::path::PathBuf,
    ) -> Self {
        Self {
            session_file_path,
            session_encryption_key,
            db_dir_path,
        }
    }
}
