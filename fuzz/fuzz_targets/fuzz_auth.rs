// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql_core::auth::{scramble_caching_sha2, scramble_native_password};

fuzz_target!(|data: &[u8]| {
    // 任意の入力に対してスクランブル関数がパニックしないことを検証する。
    let native = scramble_native_password(data, data);
    let native2 = scramble_native_password(data, data);
    assert_eq!(native, native2, "scramble_native_password must be deterministic");
    assert_eq!(
        native.len(),
        if data.is_empty() { 0 } else { 20 },
        "scramble_native_password output length must be 20 bytes or empty"
    );

    let caching = scramble_caching_sha2(data, data);
    let caching2 = scramble_caching_sha2(data, data);
    assert_eq!(caching, caching2, "scramble_caching_sha2 must be deterministic");
    assert_eq!(
        caching.len(),
        if data.is_empty() { 0 } else { 32 },
        "scramble_caching_sha2 output length must be 32 bytes or empty"
    );
});
