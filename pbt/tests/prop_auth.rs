// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! auth モジュールの Property-Based Testing。
//!
//! 各スクランブル関数が決定的に動作し、出力が期待する長さ・形式を満たすことを検証する。

use std::cell::Cell;

use aws_lc_rs::digest::{self, Context};
use shiguredo_mysql_core::auth::{
    ed25519_password, scramble_caching_sha2, scramble_native_password, sha2_rsa_encrypt,
};

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

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

/// 指定長のバイト列をサンプリングする。
///
/// 空・最小・最大の境界に意味のある確率を与えて、
/// 一様分布だけでは到達しにくい空入力を取りこぼさないようにする。
fn sample_bytes_bounded(
    ctx: &mut noprop::TestCaseContext,
    min_len: usize,
    max_len: usize,
) -> Vec<u8> {
    let len = if min_len < max_len {
        noprop::sample_with_boundaries(
            ctx,
            &[min_len, min_len + 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, min_len..=max_len),
        )
    } else {
        noprop::sample_with_boundaries(ctx, &[min_len, max_len], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, min_len..=max_len)
        })
    };
    noprop::sample_bytes_vec(ctx, len)
}

/// mysql_native_password のスクランブル結果は、パスワードが空であれば空、
/// そうでなければ SCRAMBLE_LENGTH（20）バイト。
#[test]
fn prop_scramble_native_password_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let non_empty = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 0, 256);
        let message = sample_bytes_bounded(ctx, 0, 256);
        let scrambled = scramble_native_password(&password, &message);
        let expected_len = if password.is_empty() { 0 } else { 20 };
        assert_eq!(
            scrambled.len(),
            expected_len,
            "スクランブル結果の長さが一致しない"
        );
        if password.is_empty() {
            empty.set(empty.get() + 1);
        } else {
            non_empty.set(non_empty.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空パスワードが 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty.get() > 0,
        "非空パスワードが 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// mysql_native_password は同じ入力に対して決定的に同じ出力を返す。
#[test]
fn prop_scramble_native_password_deterministic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 0, 256);
        let message = sample_bytes_bounded(ctx, 0, 256);
        let a = scramble_native_password(&password, &message);
        let b = scramble_native_password(&password, &message);
        assert_eq!(a, b, "同じ入力に対する出力が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// caching_sha2_password のスクランブル結果は、パスワードが空であれば空、
/// そうでなければ SHA-256 出力の 32 バイト。
#[test]
fn prop_scramble_caching_sha2_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let non_empty = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 0, 256);
        let message = sample_bytes_bounded(ctx, 0, 256);
        let scrambled = scramble_caching_sha2(&password, &message);
        let expected_len = if password.is_empty() { 0 } else { 32 };
        assert_eq!(
            scrambled.len(),
            expected_len,
            "スクランブル結果の長さが一致しない"
        );
        if password.is_empty() {
            empty.set(empty.get() + 1);
        } else {
            non_empty.set(non_empty.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空パスワードが 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty.get() > 0,
        "非空パスワードが 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// caching_sha2_password は同じ入力に対して決定的に同じ出力を返す。
#[test]
fn prop_scramble_caching_sha2_deterministic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 0, 256);
        let message = sample_bytes_bounded(ctx, 0, 256);
        let a = scramble_caching_sha2(&password, &message);
        let b = scramble_caching_sha2(&password, &message);
        assert_eq!(a, b, "同じ入力に対する出力が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// mysql_native_password の結果は手動で計算した SHA1 ベースのスクランブルと一致する。
#[test]
fn prop_scramble_native_password_matches_manual() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 1, 256);
        let message = sample_bytes_bounded(ctx, 0, 256);
        let actual = scramble_native_password(&password, &message);
        let expected = manual_scramble_native_password(&password, &message);
        assert_eq!(actual, expected, "手動計算の結果と一致しない");
        Ok(())
    })?;
    Ok(())
}

/// caching_sha2_password の結果は手動で計算した SHA-256 ベースのスクランブルと一致する。
#[test]
fn prop_scramble_caching_sha2_matches_manual() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 1, 256);
        let nonce = sample_bytes_bounded(ctx, 0, 256);
        let actual = scramble_caching_sha2(&password, &nonce);
        let expected = manual_scramble_caching_sha2(&password, &nonce);
        assert_eq!(actual, expected, "手動計算の結果と一致しない");
        Ok(())
    })?;
    Ok(())
}

/// ed25519_password の署名結果は常に 64 バイト。
#[test]
fn prop_ed25519_password_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 最小長（32 バイト）が実行されたことを記録する。
    let min_len = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 32, 256);
        let scramble = sample_bytes_bounded(ctx, 0, 256);
        let signature = ed25519_password(&password, &scramble).expect("ed25519 署名の生成に失敗");
        assert_eq!(signature.len(), 64, "署名長が 64 バイトでない");
        if password.len() == 32 {
            min_len.set(min_len.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        min_len.get() > 0,
        "最小長 32 バイトのパスワードが 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// ed25519_password は同じ入力に対して決定的に同じ署名を返す。
#[test]
fn prop_ed25519_password_deterministic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 32, 256);
        let scramble = sample_bytes_bounded(ctx, 0, 256);
        let a = ed25519_password(&password, &scramble).expect("ed25519 署名の生成に失敗");
        let b = ed25519_password(&password, &scramble).expect("ed25519 署名の生成に失敗");
        assert_eq!(a, b, "同じ入力に対する署名が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// sha2_rsa_encrypt は有効な公開鍵であれば 2048 ビット鍵に対する 256 バイトの暗号文を返す。
/// RSA-OAEP（SHA-1）の最大メッセージ長は 214 バイトなので、パスワードは 213 バイト以下に抑える。
#[test]
fn prop_sha2_rsa_encrypt_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・最大長の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let max_len = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let password = sample_bytes_bounded(ctx, 0, 213);
        let salt = sample_bytes_bounded(ctx, 0, 256);
        let encrypted = sha2_rsa_encrypt(&password, &salt, TEST_RSA_PUBLIC_KEY_PEM.as_bytes());
        assert!(encrypted.is_ok(), "暗号化に失敗: {encrypted:?}");
        assert_eq!(
            encrypted.expect("暗号化結果の取得に失敗").len(),
            256,
            "暗号文長が 256 バイトでない"
        );
        if password.is_empty() {
            empty.set(empty.get() + 1);
        }
        if password.len() == 213 {
            max_len.set(max_len.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空パスワードが 1 件も実行されなかった\n{runner}"
    );
    assert!(
        max_len.get() > 0,
        "最大長 213 バイトのパスワードが 1 件も実行されなかった\n{runner}"
    );
    Ok(())
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
