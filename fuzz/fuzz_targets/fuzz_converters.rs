// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql::converters::{decoder_for, escape_bytes, escape_string, Value};

fuzz_target!(|data: &[u8]| {
    // 任意の入力に対してエスケープ関数がパニックしないことを検証する。
    if let Ok(s) = std::str::from_utf8(data) {
        let escaped = escape_string(s);
        // escape_string の結果はアンエスケープすると元に戻る。
        assert_eq!(unescape_string(&escaped), s);
    }

    let escaped_bytes = escape_bytes(data, "utf8");
    // escape_bytes の結果は X'...' 形式の 16 進リテラルである。
    assert!(escaped_bytes.starts_with("X'") && escaped_bytes.ends_with('\''));
    let inner = &escaped_bytes[2..escaped_bytes.len() - 1];
    assert!(inner.chars().all(|c| c.is_ascii_hexdigit()));

    // 単一のランダムなフィールド型に対して decoder_for が返す converter がパニックしない。
    let field_type = if data.is_empty() { 0 } else { data[0] };
    if let Some(converter) = decoder_for(field_type) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = converter(s);
        }
    }

    // Value::to_sql が各バリアントでパニックしないことを検証する。
    // Float は有限値のみを使用する。
    let values = [
        Value::Null,
        Value::Bool(true),
        Value::Int(0),
        Value::Float(1.5),
        Value::String(String::new()),
        Value::Bytes(data.to_vec()),
        Value::Date(chrono::NaiveDate::from_ymd_opt(2024, 1, 1).unwrap()),
        Value::Time(chrono::NaiveTime::from_hms_opt(12, 0, 0).unwrap()),
        Value::DateTime(
            chrono::NaiveDate::from_ymd_opt(2024, 1, 1)
                .unwrap()
                .and_hms_opt(12, 0, 0)
                .unwrap(),
        ),
        Value::TimeSpan(chrono::TimeDelta::seconds(3600)),
        Value::Decimal(rust_decimal::Decimal::from(42)),
    ];
    for value in &values {
        let sql = value.to_sql("utf8").unwrap();
        // 文字列・日時型はシングルクォートで囲まれる。バイト列は X'...' 形式。
        match value {
            Value::String(_) | Value::Date(_) | Value::Time(_) | Value::DateTime(_) | Value::TimeSpan(_) => {
                assert!(sql.starts_with('\'') && sql.ends_with('\''));
            }
            Value::Bytes(_) => {
                assert!(sql.starts_with("X'") && sql.ends_with('\''));
            }
            _ => {}
        }
    }
});

/// escape_string の結果をアンエスケープする。
fn unescape_string(s: &str) -> String {
    let mut result = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => result.push('\\'),
                Some('\'') => result.push('\''),
                Some('"') => result.push('"'),
                Some('n') => result.push('\n'),
                Some('r') => result.push('\r'),
                Some('0') => result.push('\0'),
                Some('Z') => result.push('\x1a'),
                Some(other) => {
                    result.push('\\');
                    result.push(other);
                }
                None => result.push('\\'),
            }
        } else {
            result.push(c);
        }
    }
    result
}
