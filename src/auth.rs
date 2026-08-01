// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 認証プラグインの実装。

use crate::error::{Error, Result};
use aws_lc_rs::digest::{self, Context};
use aws_lc_rs::rsa::{OAEP_SHA1_MGF1SHA1, OaepPublicEncryptingKey, PublicEncryptingKey};
use rustls_pki_types::{SubjectPublicKeyInfoDer, pem::PemObject};
use x509_cert::Certificate;
use x509_cert::der::{Decode, DecodePem, Encode};

/// スクランブル長。
pub const SCRAMBLE_LENGTH: usize = 20;

/// mysql_native_password のスクランブルを生成する。
pub fn scramble_native_password(password: &[u8], message: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let stage1 = sha1_hash(password);
    let stage2 = sha1_hash(&stage1);
    let mut hasher = Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    hasher.update(&message[..SCRAMBLE_LENGTH.min(message.len())]);
    hasher.update(&stage2);
    let result = hasher.finish();
    my_crypt(result.as_ref(), &stage1)
}

fn sha1_hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, data)
        .as_ref()
        .to_vec()
}

fn my_crypt(message1: &[u8], message2: &[u8]) -> Vec<u8> {
    message1
        .iter()
        .zip(message2.iter())
        .map(|(a, b)| a ^ b)
        .collect()
}

/// caching_sha2_password のスクランブルを生成する。
pub fn scramble_caching_sha2(password: &[u8], nonce: &[u8]) -> Vec<u8> {
    if password.is_empty() {
        return Vec::new();
    }
    let p1 = sha256_hash(password);
    let p2 = sha256_hash(&p1);
    let mut hasher = Context::new(&digest::SHA256);
    hasher.update(&p2);
    hasher.update(nonce);
    let p3 = hasher.finish();

    p1.iter()
        .zip(p3.as_ref().iter())
        .map(|(a, b)| a ^ b)
        .collect()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}

/// パスワードとソルトを XOR する。
fn xor_password(password: &[u8], salt: &[u8]) -> Vec<u8> {
    let salt = &salt[..SCRAMBLE_LENGTH.min(salt.len())];
    if salt.is_empty() {
        return password.to_vec();
    }
    password
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ salt[i % salt.len()])
        .collect()
}

/// RSA-OAEP でパスワードを暗号化する。
pub fn sha2_rsa_encrypt(password: &[u8], salt: &[u8], public_key: &[u8]) -> Result<Vec<u8>> {
    let mut message = xor_password(password, salt);
    message.push(0);

    // PEM 形式の公開鍵から SPKI (SubjectPublicKeyInfo) DER を抽出する。
    let spki = parse_public_key_pem(public_key)?;

    let public_key =
        PublicEncryptingKey::from_der(spki.as_ref()).map_err(|_| Error::OperationalError {
            code: crate::constants::client_error::CR_AUTH_PLUGIN_ERR,
            message: "Failed to parse public key".to_string(),
        })?;

    let oaep_key =
        OaepPublicEncryptingKey::new(public_key).map_err(|_| Error::OperationalError {
            code: crate::constants::client_error::CR_AUTH_PLUGIN_ERR,
            message: "Failed to construct RSA-OAEP key".to_string(),
        })?;

    let mut ciphertext = vec![0u8; oaep_key.ciphertext_size()];
    oaep_key
        .encrypt(&OAEP_SHA1_MGF1SHA1, &message, &mut ciphertext, None)
        .map_err(|_| Error::OperationalError {
            code: crate::constants::client_error::CR_AUTH_PLUGIN_ERR,
            message: "RSA encryption failed".to_string(),
        })?;

    Ok(ciphertext)
}

/// PEM 形式の公開鍵を SPKI DER バイト列に変換する。
fn parse_public_key_pem(public_key: &[u8]) -> Result<Vec<u8>> {
    // まず SubjectPublicKeyInfo として解析を試みる。
    if let Ok(spki) = SubjectPublicKeyInfoDer::from_pem_slice(public_key) {
        return Ok(spki.as_ref().to_vec());
    }

    // 次に X.509 証明書として PEM 形式を解析を試みる。
    if let Ok(pem_str) = std::str::from_utf8(public_key)
        && let Ok(cert) = Certificate::from_pem(pem_str)
    {
        return extract_spki(&cert);
    }

    // 最後に X.509 証明書として DER 形式を解析を試みる。
    let cert = Certificate::from_der(public_key).map_err(|e| Error::OperationalError {
        code: crate::constants::client_error::CR_AUTH_PLUGIN_ERR,
        message: format!("Failed to parse public key PEM/DER: {}", e),
    })?;
    extract_spki(&cert)
}

fn extract_spki(cert: &Certificate) -> Result<Vec<u8>> {
    cert.tbs_certificate()
        .subject_public_key_info()
        .to_der()
        .map_err(|e| Error::OperationalError {
            code: crate::constants::client_error::CR_AUTH_PLUGIN_ERR,
            message: format!("Failed to extract SubjectPublicKeyInfo: {}", e),
        })
}

/// 認証プラグイン名。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AuthPlugin {
    MysqlNativePassword,
    CachingSha2Password,
    Sha256Password,
    MysqlClearPassword,
    MysqlOldPassword,
    ClientEd25519,
    Dialog,
    Other(String),
}

impl AuthPlugin {
    /// バイト列から認証プラグイン名を解析する。
    pub fn from_bytes(name: &[u8]) -> Self {
        match name {
            b"mysql_native_password" => AuthPlugin::MysqlNativePassword,
            b"caching_sha2_password" => AuthPlugin::CachingSha2Password,
            b"sha256_password" => AuthPlugin::Sha256Password,
            b"mysql_clear_password" => AuthPlugin::MysqlClearPassword,
            b"mysql_old_password" => AuthPlugin::MysqlOldPassword,
            b"client_ed25519" => AuthPlugin::ClientEd25519,
            b"dialog" => AuthPlugin::Dialog,
            _ => AuthPlugin::Other(String::from_utf8_lossy(name).to_string()),
        }
    }

    /// 認証プラグイン名をバイト列に変換する。
    pub fn as_bytes(&self) -> &[u8] {
        match self {
            AuthPlugin::MysqlNativePassword => b"mysql_native_password",
            AuthPlugin::CachingSha2Password => b"caching_sha2_password",
            AuthPlugin::Sha256Password => b"sha256_password",
            AuthPlugin::MysqlClearPassword => b"mysql_clear_password",
            AuthPlugin::MysqlOldPassword => b"mysql_old_password",
            AuthPlugin::ClientEd25519 => b"client_ed25519",
            AuthPlugin::Dialog => b"dialog",
            AuthPlugin::Other(s) => s.as_bytes(),
        }
    }
}

/// 初期ハンドシェイク用の認証レスポンスを生成する。
pub fn make_auth_response(
    plugin: &AuthPlugin,
    password: &[u8],
    salt: &[u8],
    secure: bool,
) -> Result<Vec<u8>> {
    match plugin {
        AuthPlugin::MysqlNativePassword => Ok(scramble_native_password(password, salt)),
        AuthPlugin::CachingSha2Password => {
            if password.is_empty() {
                Ok(Vec::new())
            } else {
                Ok(scramble_caching_sha2(password, salt))
            }
        }
        AuthPlugin::Sha256Password => {
            if secure {
                let mut v = password.to_vec();
                v.push(0);
                Ok(v)
            } else if password.is_empty() {
                Ok(vec![0])
            } else {
                Ok(vec![1])
            }
        }
        AuthPlugin::MysqlClearPassword => {
            let mut v = password.to_vec();
            v.push(0);
            Ok(v)
        }
        AuthPlugin::ClientEd25519 => ed25519_password(password, salt),
        _ => Ok(Vec::new()),
    }
}

/// client_ed25519 用の署名を生成する。
/// パスワードから秘密鍵・公開鍵を導出し、scramble に署名する。
pub fn ed25519_password(password: &[u8], scramble: &[u8]) -> Result<Vec<u8>> {
    // Ed25519 の秘密鍵導出は RFC 8032 section 5.1.6 に基づく。
    // curve25519-dalek の低レベル API を使用して実装する。
    // 簡潔さのため、ed25519-dalek の Keypair::from_bytes 等は使用せず、
    // ハッシュから直接 scalar と nonce を導出して署名を計算する。
    let h = sha512_hash(password);
    let (s_bytes, r_seed) = h.split_at(32);
    let s = clamp_scalar(s_bytes);

    let mut r_hasher = Context::new(&digest::SHA512);
    r_hasher.update(r_seed);
    r_hasher.update(scramble);
    let r = scalar_reduce(r_hasher.finish().as_ref());

    let r_point = curve25519_dalek::constants::ED25519_BASEPOINT_TABLE * &r;
    let r_encoded = r_point.compress();

    let a_point = curve25519_dalek::constants::ED25519_BASEPOINT_TABLE * &s;
    let a_encoded = a_point.compress();

    let mut k_hasher = Context::new(&digest::SHA512);
    k_hasher.update(r_encoded.as_bytes());
    k_hasher.update(a_encoded.as_bytes());
    k_hasher.update(scramble);
    let k = scalar_reduce(k_hasher.finish().as_ref());

    let ks = k * s;
    let s_scalar = ks + r;

    let mut signature = Vec::with_capacity(64);
    signature.extend_from_slice(r_encoded.as_bytes());
    signature.extend_from_slice(s_scalar.as_bytes());
    Ok(signature)
}

fn sha512_hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA512, data).as_ref().to_vec()
}

fn clamp_scalar(s32: &[u8]) -> curve25519_dalek::Scalar {
    let mut h: [u8; 32] = s32[..32]
        .try_into()
        .expect("clamp_scalar input must be at least 32 bytes");
    h[0] &= 248;
    h[31] &= 127;
    h[31] |= 64;
    curve25519_dalek::Scalar::from_bytes_mod_order(h)
}

fn scalar_reduce(hash: &[u8]) -> curve25519_dalek::Scalar {
    let mut expanded = [0u8; 64];
    let len = hash.len().min(64);
    expanded[..len].copy_from_slice(&hash[..len]);
    curve25519_dalek::Scalar::from_bytes_mod_order_wide(&expanded)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rcgen::{CertificateParams, DnType, KeyPair};

    /// テスト用の自己署名 RSA 証明書（PEM）を動的に生成する。
    fn generate_test_cert_pem() -> String {
        let key = KeyPair::generate_for(&rcgen::PKCS_RSA_SHA256).expect("rsa keygen");
        let mut params = CertificateParams::default();
        params
            .distinguished_name
            .push(DnType::CommonName, "mysql-rs-test");
        let cert = params.self_signed(&key).expect("self-sign");
        cert.pem()
    }

    #[test]
    fn test_parse_public_key_pem_from_x509_cert() {
        let cert_pem = generate_test_cert_pem();
        let spki = parse_public_key_pem(cert_pem.as_bytes()).unwrap();
        assert!(!spki.is_empty());
        // SPKI DER としてパース可能であることを確認。
        PublicEncryptingKey::from_der(&spki).unwrap();
    }

    #[test]
    fn test_parse_public_key_pem_invalid_input() {
        let result = parse_public_key_pem(b"not a pem");
        assert!(result.is_err());
    }

    #[test]
    fn test_make_auth_response_client_ed25519() {
        let plugin = AuthPlugin::ClientEd25519;
        // ed25519_password は 32 バイト以上の入力を必要とする。
        let password = b"this password must be at least thirtytwo bytes!!";
        let salt = b"12345678901234567890";
        let response = make_auth_response(&plugin, password, salt, false).unwrap();
        assert_eq!(response.len(), 64);
    }
}
