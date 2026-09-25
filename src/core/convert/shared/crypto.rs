use super::error::{DecompilerError, Result};
use aes_gcm::aead::array::Array;
use aes_gcm::aead::array::typenum::{U12, U32};
use aes_gcm::{
    Aes256Gcm,
    aead::{Aead, KeyInit},
};
use base64::{Engine as _, engine::general_purpose};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::sync::Arc;

// 加密服务
#[derive(Clone)]
pub(crate) struct CryptoService {
    salt: Arc<[u8]>,
}

const NONCE_SIZE: usize = 12;

impl CryptoService {
    pub(crate) fn new(salt: &[u8]) -> Self {
        Self {
            salt: Arc::from(salt),
        }
    }

    pub(crate) fn sha256(data: &str) -> String {
        use std::fmt::Write as _;
        let mut hasher = Sha256::new();
        hasher.update(data.as_bytes());
        let result = hasher.finalize();
        let mut out = String::with_capacity(result.len() * 2);
        for b in result {
            let _ = write!(out, "{b:02x}");
        }
        out
    }

    pub(crate) fn base64_to_bytes(data: &str) -> Result<Vec<u8>> {
        general_purpose::STANDARD
            .decode(data)
            .map_err(|e| DecompilerError::Crypto(format!("Base64解码失败: {}", e)))
    }

    pub(crate) fn reverse_string(data: &str) -> String {
        data.chars().rev().collect()
    }

    pub(crate) fn generate_aes_key(&self) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(&self.salt);
        let hash = hasher.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&hash);
        key
    }

    pub(crate) fn decrypt_aes_gcm(&self, ciphertext: &[u8], iv: &[u8]) -> Result<Vec<u8>> {
        type AesKey = Array<u8, U32>;
        type Nonce = Array<u8, U12>;

        let key = self.generate_aes_key();
        let key_array = AesKey::try_from(key.as_slice())
            .map_err(|e| DecompilerError::Crypto(format!("Invalid AES key: {}", e)))?;
        let cipher = Aes256Gcm::new(&key_array);
        let nonce = Nonce::try_from(iv)
            .map_err(|e| DecompilerError::Crypto(format!("Invalid nonce: {}", e)))?;

        cipher
            .decrypt(&nonce, ciphertext)
            .map_err(|e| DecompilerError::Crypto(format!("AES解密失败: {}", e)))
    }

    pub(crate) fn decrypt_bcmkn(&self, encrypted_content: &str) -> Result<Vec<u8>> {
        let reversed = Self::reverse_string(encrypted_content);
        let decoded = Self::base64_to_bytes(&reversed)?;
        if decoded.len() <= NONCE_SIZE {
            return Err(DecompilerError::Crypto(format!(
                "数据长度 {} 不足,至少需要 {} 字节",
                decoded.len(),
                NONCE_SIZE + 1
            )));
        }
        let (iv, ciphertext) = decoded
            .split_at_checked(NONCE_SIZE)
            .ok_or_else(|| DecompilerError::Crypto("IV 长度不足".into()))?;
        self.decrypt_aes_gcm(ciphertext, iv)
    }

    /// NEKO 播放器下发的密文 → KN 文档 JSON:
    /// `base64(reverse(content))` → AES-GCM(前 12 字节为 IV)→ UTF-8 → JSON
    pub(crate) fn decrypt_bcmkn_json(&self, encrypted_content: &str) -> Result<Value> {
        let decrypted_bytes = self.decrypt_bcmkn(encrypted_content)?;
        let decrypted_str = String::from_utf8(decrypted_bytes)
            .map_err(|e| DecompilerError::Crypto(format!("UTF-8转换失败: {}", e)))?;
        Ok(serde_json::from_str(&decrypted_str)?)
    }
}
