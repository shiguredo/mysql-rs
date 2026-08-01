// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql_core::charset::{charset_by_name, mblength};

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列を文字セット名として参照してもパニックしない。
    if let Ok(name) = std::str::from_utf8(data) {
        if let Some(charset) = charset_by_name(name) {
            // 名前検索で取得した文字セットの名前は、小文字化した入力名と一致するか、utf8 の場合は utf8mb4。
            let lowered = name.to_lowercase();
            assert!(
                charset.name == lowered || (lowered == "utf8" && charset.name == "utf8mb4"),
                "charset name mismatch: input={}, got={}",
                name,
                charset.name
            );
        }
    }

    // 任意の ID に対して mblength が 1 以上を返す。
    let id = if data.len() >= 2 {
        u16::from_le_bytes([data[0], data[1]])
    } else {
        0
    };
    assert!(mblength(id) >= 1);
});
