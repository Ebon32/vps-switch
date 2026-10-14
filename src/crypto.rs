use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use anyhow::{anyhow, Context, Result};
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use rand::RngCore;
use scrypt::{scrypt, Params as ScryptParams};
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};

const ONESHELL_FALLBACK_SECRET: &str = "change-me-before-production";
const ONESHELL_LEGACY_SALT: &[u8] = b"ai-web-terminal";

#[derive(Clone)]
pub struct MasterKey {
    key: [u8; 32],
}

#[derive(Debug, Serialize, Deserialize)]
struct EncryptedBlob {
    iv: String,
    value: String,
}

#[derive(Debug, Deserialize)]
pub struct OneShellCipherPayload {
    pub salt: Option<String>,
    pub iv: Option<String>,
    pub tag: Option<String>,
    pub value: Option<String>,
}

impl MasterKey {
    pub fn load_or_create(data_dir: &Path) -> Result<Self> {
        fs::create_dir_all(data_dir)?;
        let key_path = data_dir.join("secret.key");
        if key_path.exists() {
            let raw = fs::read_to_string(&key_path).context("读取 secret.key 失败")?;
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                let decoded = B64
                    .decode(trimmed)
                    .context("解码 secret.key base64 失败")?;
                if decoded.len() != 32 {
                    // 关键：绝不覆盖。旧实现遇到非法内容会直接生成新密钥盖掉原文件，
                    // 那会让库里所有已加密的密码/私钥**永久**解不开，且毫无提示。
                    // 宁可拒绝启动，让人工处理（真要从头开始就手动删掉这个文件）。
                    return Err(anyhow!(
                        "secret.key 存在但不是合法的 32 字节密钥（解出 {} 字节），已拒绝覆盖，\
                         以免破坏库中已加密的凭据。请人工恢复该文件；确需重新开始请手动删除它。",
                        decoded.len()
                    ));
                }
                let mut key = [0u8; 32];
                key.copy_from_slice(&decoded);
                return Ok(Self { key });
            }
            // 空文件（例如上一次写入被中途打断）：当作尚未创建，走下面重新生成
        }

        let mut key = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut key);
        fs::write(&key_path, B64.encode(key)).context("写入 secret.key 失败")?;
        Ok(Self { key })
    }

    pub fn encrypt(&self, plaintext: &str) -> Result<Option<String>> {
        if plaintext.is_empty() {
            return Ok(None);
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let mut iv = [0u8; 12];
        rand::thread_rng().fill_bytes(&mut iv);
        let nonce = Nonce::from_slice(&iv);
        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_bytes())
            .map_err(|_| anyhow!("AES-GCM 加密失败"))?;
        let blob = EncryptedBlob {
            iv: B64.encode(iv),
            value: B64.encode(ciphertext),
        };
        Ok(Some(serde_json::to_string(&blob)?))
    }

    pub fn decrypt(&self, blob_json: Option<&str>) -> Result<Option<String>> {
        let Some(raw) = blob_json.filter(|s| !s.trim().is_empty()) else {
            return Ok(None);
        };
        let blob: EncryptedBlob = serde_json::from_str(raw).context("解析加密凭据 JSON 失败")?;
        let iv = B64.decode(blob.iv).context("解码 IV 失败")?;
        let ciphertext = B64.decode(blob.value).context("解码密文失败")?;
        if iv.len() != 12 {
            return Err(anyhow!("无效的 IV 长度"));
        }
        let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&self.key));
        let plaintext = cipher
            .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
            .map_err(|_| anyhow!("凭据解密失败"))?;
        Ok(Some(String::from_utf8(plaintext)?))
    }
}

/// 解密旧版 1Shell 的 AES-256-GCM 凭据 ({ salt, iv, tag, value })
pub fn decrypt_1shell_payload(
    payload: &OneShellCipherPayload,
    custom_secret: Option<&str>,
) -> Result<Option<String>> {
    let Some(value_b64) = payload.value.as_deref().filter(|s| !s.is_empty()) else {
        return Ok(None);
    };
    let iv_b64 = payload
        .iv
        .as_deref()
        .ok_or_else(|| anyhow!("1Shell 凭据缺少 iv"))?;
    let tag_b64 = payload
        .tag
        .as_deref()
        .ok_or_else(|| anyhow!("1Shell 凭据缺少 tag"))?;

    let salt_bytes = match payload.salt.as_deref().filter(|s| !s.is_empty()) {
        Some(s) => B64.decode(s).context("解码 1Shell salt 失败")?,
        None => ONESHELL_LEGACY_SALT.to_vec(),
    };
    let iv = B64.decode(iv_b64).context("解码 1Shell iv 失败")?;
    let tag = B64.decode(tag_b64).context("解码 1Shell tag 失败")?;
    let mut ciphertext = B64.decode(value_b64).context("解码 1Shell value 失败")?;

    if iv.len() != 12 || tag.len() != 16 {
        return Err(anyhow!("1Shell 凭据 iv/tag 长度异常"));
    }

    let secret = custom_secret
        .filter(|s| !s.is_empty())
        .unwrap_or(ONESHELL_FALLBACK_SECRET);

    // Node crypto.scryptSync 默认参数: N=16384 (log_n=14), r=8, p=1, keylen=32
    let params = ScryptParams::new(14, 8, 1, 32).map_err(|_| anyhow!("无效的 scrypt 参数"))?;
    let mut derived_key = [0u8; 32];
    scrypt(secret.as_bytes(), &salt_bytes, &params, &mut derived_key)
        .map_err(|_| anyhow!("scrypt 派生密钥失败"))?;

    // aes-gcm crate 的密文末尾直接拼接 16 字节 auth tag
    ciphertext.extend_from_slice(&tag);

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&derived_key));
    let plaintext = cipher
        .decrypt(Nonce::from_slice(&iv), ciphertext.as_ref())
        .map_err(|_| anyhow!("1Shell 凭据解密校验失败"))?;

    Ok(Some(String::from_utf8(plaintext)?))
}
