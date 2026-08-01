// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! optionfile モジュールの Property-Based Testing。
//!
//! 任意のセクション・キー・値から組み立てたテキストをパースしたとき、
//! `get` で元の値 (クォート・空白除去済み) を取り出せることを検証する。

use proptest::prelude::*;
use shiguredo_mysql::optionfile::OptionFile;

/// キー名と値の戦略。
fn key_value_strategy() -> impl Strategy<Value = (String, String)> {
    let key = "[a-zA-Z0-9_]+";
    let value = proptest::string::string_regex(".{0,32}").unwrap();
    (key, value)
}

/// 両端のクォートを除去する (OptionFile の正規化と同一の規則)。
fn strip_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        if (first == b'\'' || first == b'"') && bytes[bytes.len() - 1] == first {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

/// パース時の値の正規化 (trim + クォート除去) を再現する。
fn normalize_value(value: &str) -> String {
    strip_quotes(value.trim())
}

proptest! {
    /// パースしたオプションファイルから、書いた値が正規化済みで取り出せる。
    #[test]
    fn prop_option_file_roundtrip(
        entries in proptest::collection::vec(key_value_strategy(), 0..=8),
        section in "[a-zA-Z0-9_]+",
    ) {
        // キーは小文字化と _ → - の正規化が入るため、正規化後のキーを期待値として使う。
        // 正規化後に衝突しないよう、インデックスを付けて一意にする。
        let normalized: Vec<(String, String)> = entries
            .iter()
            .enumerate()
            .map(|(i, (key, value))| {
                let key = format!("{}_{}", i, key).to_lowercase().replace('_', "-");
                (key, normalize_value(value))
            })
            .collect();

        let mut text = format!("[{}]\n", section);
        for (i, (key, value)) in entries.iter().enumerate() {
            text.push_str(&format!("{}_{}={}\n", i, key, value));
        }

        let file = OptionFile::parse(&text).unwrap();
        for (key, expected) in &normalized {
            prop_assert_eq!(file.get(&section, key), Some(expected.as_str()));
        }
    }

    /// パース結果は決定的である (同じ入力から同じ出力)。
    #[test]
    fn prop_option_file_deterministic(
        entries in proptest::collection::vec(key_value_strategy(), 0..=8),
    ) {
        let mut text = String::from("[client]\n");
        for (i, (key, value)) in entries.iter().enumerate() {
            text.push_str(&format!("{}_{}={}\n", i, key, value));
        }
        let file1 = OptionFile::parse(&text).unwrap();
        let file2 = OptionFile::parse(&text).unwrap();
        for (i, (key, value)) in entries.iter().enumerate() {
            let key = format!("{}_{}", i, key).to_lowercase().replace('_', "-");
            let expected = normalize_value(value);
            prop_assert_eq!(file1.get("client", &key), file2.get("client", &key));
            prop_assert_eq!(file1.get("client", &key), Some(expected.as_str()));
        }
    }
}
