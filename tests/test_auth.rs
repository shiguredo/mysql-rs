// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

use aws_lc_rs::digest::{self, Context};
use shiguredo_mysql::auth::{scramble_caching_sha2, scramble_native_password};

#[test]
fn test_scramble_native_password_empty() {
    let result = scramble_native_password(b"", b"salt");
    assert!(result.is_empty(), "空パスワードの結果は空であるべき");
}

#[test]
fn test_scramble_native_password() {
    let password = b"password";
    let message = b"abcdefghijklmnopqrst";
    let result = scramble_native_password(password, message);

    // 結果が 20 バイトの SHA1 ダイジェストであることを確認。
    assert_eq!(
        result.len(),
        20,
        "native_password の結果長は 20 バイトであるべき"
    );

    // 手動で計算して一致することを確認。
    let stage1 = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, password);
    let stage2 = digest::digest(&digest::SHA1_FOR_LEGACY_USE_ONLY, stage1.as_ref());
    let mut h = Context::new(&digest::SHA1_FOR_LEGACY_USE_ONLY);
    h.update(message);
    h.update(stage2.as_ref());
    let expected: Vec<u8> = h
        .finish()
        .as_ref()
        .iter()
        .zip(stage1.as_ref().iter())
        .map(|(a, b)| a ^ b)
        .collect();
    assert_eq!(
        result, expected,
        "native_password の結果が手動計算と一致するべき"
    );
}

#[test]
fn test_scramble_caching_sha2_empty() {
    let result = scramble_caching_sha2(b"", b"nonce");
    assert!(result.is_empty(), "空パスワードの結果は空であるべき");
}

#[test]
fn test_scramble_caching_sha2() {
    let password = b"password";
    let nonce = b"nonce";
    let result = scramble_caching_sha2(password, nonce);
    assert_eq!(
        result.len(),
        32,
        "caching_sha2 の結果長は 32 バイトであるべき"
    );

    let p1 = digest::digest(&digest::SHA256, password);
    let p2 = digest::digest(&digest::SHA256, p1.as_ref());
    let mut h = Context::new(&digest::SHA256);
    h.update(p2.as_ref());
    h.update(nonce);
    let p3 = h.finish();
    let expected: Vec<u8> = p1
        .as_ref()
        .iter()
        .zip(p3.as_ref().iter())
        .map(|(a, b)| a ^ b)
        .collect();
    assert_eq!(
        result, expected,
        "caching_sha2 の結果が手動計算と一致するべき"
    );
}
