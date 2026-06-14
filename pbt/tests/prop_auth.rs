// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! auth モジュールの Property-Based Testing。
//!
//! 各スクランブル関数が決定的に動作し、出力が期待する長さ・形式を満たすことを検証する。

use aws_lc_rs::digest::{self, Context};
use proptest::prelude::*;
use shiguredo_mysql::auth::{
    ed25519_password, scramble_caching_sha2, scramble_native_password, sha2_rsa_encrypt,
};

/// テスト用の 2048 ビット RSA 公開鍵（PEM 形式）。
/// これは公開鍵であり機密情報ではないため、ソースコードに埋め込む。
const TEST_RSA_PUBLIC_KEY_PEM: &str = "-----BEGIN PUBLIC KEY-----\n\
MIIBIjANBgkqhkiG9w0BAQEFAAOCAQ8AMIIBCgKCAQEA1av+Xh2TwBCT5sGRLsLK\n\
5td0IJ93SQoL3KZhazisqYLB41OmosXJgKX7DsWfUEWpa2uY2+JKsSY4Rah9ElRm\n\
b/M0c69qHni1OZYR182ovN2Ju7g1yGONJuMXkK5K6DmxS4+uvYrHbjlYYA2MAlUjc\n\
u8duSnesN0Z+j2hzRhu+qWGSagtTSdFvk/2xo+TlheEEUIG0bswR8XDxnujbTkzj7\n\
DvTm/2PA0XaYpmx5r615cUWMsUBelMbFMYNWFKmdIUJLzSSIT6zftgMr8Cd+YPY3i\n\
9QxtIWXno2BYeU6VpbV0sbW0fSP/XAxArw8QOL07ffOxpRNlHSHzUDFx9r4wyIQID\n\
AQAB\n\
-----END PUBLIC KEY-----";

proptest! {
    /// mysql_native_password のスクランブル結果は、パスワードが空であれば空、
    /// そうでなければ SCRAMBLE_LENGTH（20）バイト。
    #[test]
    fn prop_scramble_native_password_length(
        password in proptest::collection::vec(any::<u8>(), 0..=256),
        message in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let scrambled = scramble_native_password(&password, &message);
        let expected_len = if password.is_empty() { 0 } else { 20 };
        prop_assert_eq!(scrambled.len(), expected_len);
    }

    /// mysql_native_password は同じ入力に対して決定的に同じ出力を返す。
    #[test]
    fn prop_scramble_native_password_deterministic(
        password in proptest::collection::vec(any::<u8>(), 0..=256),
        message in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let a = scramble_native_password(&password, &message);
        let b = scramble_native_password(&password, &message);
        prop_assert_eq!(a, b);
    }

    /// caching_sha2_password のスクランブル結果は、パスワードが空であれば空、
    /// そうでなければ SHA-256 出力の 32 バイト。
    #[test]
    fn prop_scramble_caching_sha2_length(
        password in proptest::collection::vec(any::<u8>(), 0..=256),
        message in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let scrambled = scramble_caching_sha2(&password, &message);
        let expected_len = if password.is_empty() { 0 } else { 32 };
        prop_assert_eq!(scrambled.len(), expected_len);
    }

    /// caching_sha2_password は同じ入力に対して決定的に同じ出力を返す。
    #[test]
    fn prop_scramble_caching_sha2_deterministic(
        password in proptest::collection::vec(any::<u8>(), 0..=256),
        message in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let a = scramble_caching_sha2(&password, &message);
        let b = scramble_caching_sha2(&password, &message);
        prop_assert_eq!(a, b);
    }

    /// mysql_native_password の結果は手動で計算した SHA1 ベースのスクランブルと一致する。
    #[test]
    fn prop_scramble_native_password_matches_manual(
        password in proptest::collection::vec(any::<u8>(), 1..=256),
        message in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let actual = scramble_native_password(&password, &message);
        let expected = manual_scramble_native_password(&password, &message);
        prop_assert_eq!(actual, expected);
    }

    /// caching_sha2_password の結果は手動で計算した SHA-256 ベースのスクランブルと一致する。
    #[test]
    fn prop_scramble_caching_sha2_matches_manual(
        password in proptest::collection::vec(any::<u8>(), 1..=256),
        nonce in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let actual = scramble_caching_sha2(&password, &nonce);
        let expected = manual_scramble_caching_sha2(&password, &nonce);
        prop_assert_eq!(actual, expected);
    }

    /// ed25519_password の署名結果は常に 64 バイト。
    #[test]
    fn prop_ed25519_password_length(
        password in proptest::collection::vec(any::<u8>(), 32..=256),
        scramble in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let signature = ed25519_password(&password, &scramble).expect("ed25519 signature");
        prop_assert_eq!(signature.len(), 64);
    }

    /// ed25519_password は同じ入力に対して決定的に同じ署名を返す。
    #[test]
    fn prop_ed25519_password_deterministic(
        password in proptest::collection::vec(any::<u8>(), 32..=256),
        scramble in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let a = ed25519_password(&password, &scramble).expect("ed25519 signature");
        let b = ed25519_password(&password, &scramble).expect("ed25519 signature");
        prop_assert_eq!(a, b);
    }

    /// sha2_rsa_encrypt は有効な公開鍵であれば 2048 ビット鍵に対する 256 バイトの暗号文を返す。
    /// RSA-OAEP（SHA-1）の最大メッセージ長は 214 バイトなので、パスワードは 213 バイト以下に抑える。
    #[test]
    fn prop_sha2_rsa_encrypt_length(
        password in proptest::collection::vec(any::<u8>(), 0..=213),
        salt in proptest::collection::vec(any::<u8>(), 0..=256),
    ) {
        let encrypted = sha2_rsa_encrypt(&password, &salt, TEST_RSA_PUBLIC_KEY_PEM.as_bytes());
        prop_assert!(encrypted.is_ok(), "encryption failed: {:?}", encrypted);
        prop_assert_eq!(encrypted.unwrap().len(), 256);
    }
}

/// mysql_native_password のアルゴリズムを手動で再実装して期待値を求める。
fn manual_scramble_native_password(password: &[u8], message: &[u8]) -> Vec<u8> {
    const SCRAMBLE_LENGTH: usize = 20;
    let stage1 = sha1_hash(password);
    let stage2 = sha1_hash(&stage1);
    let mut hasher = Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    hasher.update(&message[..SCRAMBLE_LENGTH.min(message.len())]);
    hasher.update(&stage2);
    let result = hasher.finish();
    result
        .as_ref()
        .iter()
        .zip(stage1.iter())
        .map(|(a, b)| a ^ b)
        .collect()
}

/// caching_sha2_password のアルゴリズムを手動で再実装して期待値を求める。
fn manual_scramble_caching_sha2(password: &[u8], nonce: &[u8]) -> Vec<u8> {
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

fn sha1_hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, data)
        .as_ref()
        .to_vec()
}

fn sha256_hash(data: &[u8]) -> Vec<u8> {
    digest::digest(&digest::SHA256, data).as_ref().to_vec()
}
