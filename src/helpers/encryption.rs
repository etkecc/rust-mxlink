use base64::{engine::general_purpose::STANDARD, Engine as _};

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit, OsRng},
    ChaCha20Poly1305, Key, Nonce,
};

#[derive(Debug, Clone)]
pub struct EncryptionKey([u8; 32]);

impl EncryptionKey {
    pub fn new(bytes: [u8; 32]) -> Self {
        EncryptionKey(bytes)
    }

    pub fn from_hex_str(s: &str) -> Result<Self, &'static str> {
        let bytes = hex::decode(s).map_err(|_| "Invalid hex")?;
        Self::from_vec(bytes)
    }

    pub fn from_vec(vec: Vec<u8>) -> Result<Self, &'static str> {
        if vec.len() == 32 {
            let mut array = [0u8; 32];
            array.copy_from_slice(&vec);
            Ok(EncryptionKey(array))
        } else {
            Err("The provided encryption key is not 32 bytes long")
        }
    }
}

#[derive(Debug, Clone)]
pub struct Manager {
    key: Option<EncryptionKey>,
}

impl Manager {
    pub fn new(key: Option<EncryptionKey>) -> Self {
        Self { key }
    }

    pub fn encrypt_string(&self, plaintext: &str) -> Result<String, String> {
        let Some(key) = &self.key else {
            return Ok(plaintext.to_owned());
        };

        self.do_encrypt_string(plaintext, key)
    }

    fn do_encrypt_string(&self, plaintext: &str, key: &EncryptionKey) -> Result<String, String> {
        let key = Key::from_slice(&key.0);
        let cipher = ChaCha20Poly1305::new(key);

        let nonce = ChaCha20Poly1305::generate_nonce(&mut OsRng); // 12-bytes

        let ciphertext = cipher
            .encrypt(&nonce, plaintext.as_bytes())
            .map_err(|err| format!("Encryption failed: {:?}", err))?;

        let mut combined = Vec::new();
        combined.extend_from_slice(&nonce);
        combined.extend_from_slice(&ciphertext);

        let encoded = STANDARD.encode(&combined);

        Ok(encoded)
    }

    pub fn decrypt_string(&self, ciphertext: &str) -> Result<String, String> {
        let Some(key) = &self.key else {
            return Ok(ciphertext.to_owned());
        };

        self.do_decrypt_string(ciphertext, key)
    }

    fn do_decrypt_string(&self, ciphertext: &str, key: &EncryptionKey) -> Result<String, String> {
        let decoded = STANDARD.decode(ciphertext);
        let Ok(decoded) = decoded else {
            return Err("Invalid base64".into());
        };

        if decoded.len() < 12 {
            return Err("Decoded data too short".into());
        }

        let (nonce, ciphertext) = decoded.split_at(12);

        let key = Key::from_slice(&key.0);
        let cipher = ChaCha20Poly1305::new(key);

        let plaintext = cipher.decrypt(Nonce::from_slice(nonce), ciphertext);

        match plaintext {
            Ok(plaintext) => Ok(String::from_utf8(plaintext)
                .map_err(|e| format!("Failed turning to utf8 string: {:?}", e))?),
            Err(err) => Err(format!("Decryption failed: {:?}", err)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encryption_with_passphrase() {
        let manager = Manager::new(Some(
            EncryptionKey::from_hex_str(
                "45e576aee2b639e73bd1a856f1a134cbb5810babed37e72143f7e7cec744cd5c",
            )
            .unwrap(),
        ));

        let manager_another = Manager::new(Some(
            EncryptionKey::from_hex_str(
                "55e576aee2b639e73bd1a856f1a134cbb5810babed37e72143f7e7cec744cd5c",
            )
            .unwrap(),
        ));

        let plaintext = "Hello, world!";

        let encrypted = manager.encrypt_string(plaintext).unwrap();
        assert_ne!(plaintext, encrypted);

        let decrypted = manager.decrypt_string(&encrypted).unwrap();
        assert_eq!(plaintext, decrypted);

        let decryption_result_from_another = manager_another.decrypt_string(&encrypted);
        assert!(decryption_result_from_another.is_err());
    }

    #[test]
    fn test_encryption_skipped_when_no_passphrase() {
        let manager = Manager::new(None);
        let plaintext = "Hello, world!";

        let encrypted = manager.encrypt_string(plaintext).unwrap();
        assert_eq!(plaintext, encrypted);

        let decrypted = manager.decrypt_string(&encrypted).unwrap();
        assert_eq!(plaintext, decrypted);
    }
}
