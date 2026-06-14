// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! converters モジュールの Property-Based Testing。
//!
//! エスケープ関数の出力が安全な形式を満たすこと、および値の変換が
//! 決定的に動作することを検証する。

use chrono::{NaiveDate, NaiveTime, TimeDelta};
use proptest::prelude::*;
use rust_decimal::Decimal;
use shiguredo_mysql::constants::field_type;
use shiguredo_mysql::converters::{Value, decoder_for, escape_bytes, escape_string};

/// テスト用の Value 戦略。
fn value_strategy() -> impl Strategy<Value = Value> {
    prop_oneof![
        Just(Value::Null),
        any::<bool>().prop_map(Value::Bool),
        any::<i64>().prop_map(Value::Int),
        any::<f64>()
            .prop_filter("finite float", |f| f.is_finite())
            .prop_map(Value::Float),
        any::<String>().prop_map(Value::String),
        any::<Vec<u8>>().prop_map(Value::Bytes),
        (1970i32..=9999, 1u32..=12, 1u32..=31).prop_filter_map("valid date", |(y, m, d)| {
            NaiveDate::from_ymd_opt(y, m, d).map(Value::Date)
        }),
        (0u32..=23, 0u32..=59, 0u32..=59)
            .prop_map(|(h, m, s)| Value::Time(NaiveTime::from_hms_opt(h, m, s).unwrap())),
        (
            1970i32..=9999,
            1u32..=12,
            1u32..=31,
            0u32..=23,
            0u32..=59,
            0u32..=59
        )
            .prop_filter_map("valid datetime", |(y, mo, d, h, mi, s)| {
                NaiveDate::from_ymd_opt(y, mo, d)
                    .and_then(|date| date.and_hms_opt(h, mi, s))
                    .map(Value::DateTime)
            }),
        (-1_000_000_000_000i64..=1_000_000_000_000i64)
            .prop_map(|s| Value::TimeSpan(TimeDelta::seconds(s))),
        any::<i64>().prop_map(|i| Value::Decimal(Decimal::from(i))),
    ]
}

proptest! {
    /// escape_string の結果に、エスケープされていないシングルクォートは含まれない。
    #[test]
    fn prop_escape_string_no_unescaped_quote(s in "\\PC*") {
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\'' {
                prop_assert!(i > 0 && bytes[i - 1] == b'\\');
            }
        }
    }

    /// escape_bytes の結果は X'...' 形式の 16 進リテラルである。
    #[test]
    fn prop_escape_bytes_hex_literal(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let escaped = escape_bytes(&data, "utf8");
        prop_assert!(escaped.starts_with("X'") && escaped.ends_with('\''));
        let inner = &escaped[2..escaped.len() - 1];
        prop_assert!(inner.chars().all(|c| c.is_ascii_hexdigit()));
    }

    /// escape_string の結果に、孤立したバックスラッシュは含まれない。
    #[test]
    fn prop_escape_string_no_lone_backslash(s in "\\PC*") {
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                // バックスラッシュは必ず次の 1 文字とペアになっている。
                prop_assert!(i + 1 < bytes.len());
                i += 2;
            } else {
                i += 1;
            }
        }
    }

    /// escape_string の結果に、エスケープ対象文字（バックスラッシュを除く）が
    /// 単体で出現しない。バックスラッシュはエスケープ文字そのものなので先頭に来てもよい。
    #[test]
    fn prop_escape_string_no_unescaped_special_chars(s in "\\PC*") {
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        for i in 0..bytes.len() {
            match bytes[i] {
                b'\'' | b'"' | b'\n' | b'\r' | b'\0' | 0x1a => {
                    prop_assert!(i > 0 && bytes[i - 1] == b'\\');
                }
                _ => {}
            }
        }
    }

    /// escape_string をアンエスケープすると元の文字列に戻る。
    #[test]
    fn prop_escape_string_roundtrip(s in "\\PC*") {
        let escaped = escape_string(&s);
        let unescaped = unescape_string(&escaped);
        prop_assert_eq!(unescaped, s);
    }

    /// Value::to_sql は決定的に動作し、各型に応じた MySQL リテラル形式を満たす。
    #[test]
    fn prop_value_to_sql_format(value in value_strategy()) {
        let sql = value.to_sql("utf8").unwrap();
        let sql2 = value.to_sql("utf8").unwrap();
        prop_assert_eq!(sql.clone(), sql2);

        match &value {
            Value::Null => prop_assert_eq!(sql, "NULL"),
            Value::Bool(b) => prop_assert_eq!(sql, if *b { "1" } else { "0" }),
            Value::Int(i) => {
                prop_assert_eq!(sql, i.to_string());
            }
            Value::Decimal(d) => {
                prop_assert_eq!(sql, d.to_string());
            }
            Value::Float(_f) => {
                prop_assert!(!sql.is_empty());
                // 有限値の Float は必ず数字または 'e' を含む。
                prop_assert!(sql.chars().any(|c| c.is_ascii_digit() || c == 'e'));
            }
            Value::String(s) => {
                prop_assert!(sql.starts_with('\'') && sql.ends_with('\''));
                let inner = &sql[1..sql.len() - 1];
                prop_assert_eq!(unescape_string(inner), s.as_str());
            }
            Value::Bytes(_) => {
                prop_assert!(sql.starts_with("X'") && sql.ends_with('\''));
            }
            Value::Date(d) => {
                prop_assert!(sql.starts_with('\'') && sql.ends_with('\''));
                prop_assert_eq!(&sql[1..sql.len() - 1], d.format("%Y-%m-%d").to_string());
            }
            Value::Time(t) => {
                prop_assert!(sql.starts_with('\'') && sql.ends_with('\''));
                let expected = t
                    .format("%H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                prop_assert_eq!(&sql[1..sql.len() - 1], expected);
            }
            Value::DateTime(dt) => {
                prop_assert!(sql.starts_with('\'') && sql.ends_with('\''));
                let expected = dt
                    .format("%Y-%m-%d %H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                prop_assert_eq!(&sql[1..sql.len() - 1], expected);
            }
            Value::TimeSpan(_td) => {
                prop_assert!(sql.starts_with('\'') && sql.ends_with('\''));
                let inner = &sql[1..sql.len() - 1];
                // TimeSpan は '[-]HH:MM:SS[.ffffff]' 形式。
                prop_assert!(inner.split(':').count() == 3);
            }
        }
    }

    /// decoder_for はフィールド型に応じた converter を返す。
    #[test]
    fn prop_decoder_for_consistency(input in "\\PC*") {
        let int_types = [
            field_type::BIT,
            field_type::TINY,
            field_type::SHORT,
            field_type::LONG,
            field_type::LONGLONG,
            field_type::INT24,
            field_type::YEAR,
        ];
        for ft in int_types {
            let converter = decoder_for(ft).expect("int converter");
            let value = converter(&input);
            prop_assert!(
                matches!(value, Value::Int(_) | Value::String(_)),
                "expected Int or String fallback for field type {}, got {:?}",
                ft,
                value
            );
        }

        let float_types = [field_type::FLOAT, field_type::DOUBLE];
        for ft in float_types {
            let converter = decoder_for(ft).expect("float converter");
            let value = converter(&input);
            prop_assert!(
                matches!(value, Value::Float(_) | Value::String(_)),
                "expected Float or String fallback for field type {}, got {:?}",
                ft,
                value
            );
        }

        let decimal_types = [field_type::DECIMAL, field_type::NEWDECIMAL];
        for ft in decimal_types {
            let converter = decoder_for(ft).expect("decimal converter");
            let value = converter(&input);
            prop_assert!(
                matches!(value, Value::Decimal(_) | Value::String(_)),
                "expected Decimal or String fallback for field type {}, got {:?}",
                ft,
                value
            );
        }

        let string_types = [
            field_type::BLOB,
            field_type::TINY_BLOB,
            field_type::MEDIUM_BLOB,
            field_type::LONG_BLOB,
            field_type::STRING,
            field_type::VAR_STRING,
            field_type::VARCHAR,
        ];
        for ft in string_types {
            let converter = decoder_for(ft).expect("string converter");
            let value = converter(&input);
            prop_assert!(matches!(value, Value::String(_)), "expected String for field type {}, got {:?}", ft, value);
        }

        let date_converter = decoder_for(field_type::DATE).expect("date converter");
        prop_assert!(matches!(date_converter(&input), Value::Date(_) | Value::String(_)));

        let datetime_converter = decoder_for(field_type::DATETIME).expect("datetime converter");
        prop_assert!(matches!(datetime_converter(&input), Value::DateTime(_) | Value::String(_)));

        let time_converter = decoder_for(field_type::TIME).expect("time converter");
        prop_assert!(matches!(time_converter(&input), Value::TimeSpan(_) | Value::String(_)));
    }

    /// convert_date は有効な 'YYYY-MM-DD' 形式を Date に、無効な形式を String に変換する。
    #[test]
    fn prop_convert_date_validity(
        (y, m, d) in (1970i32..=9999, 1u32..=12, 1u32..=31),
    ) {
        let s = format!("{:04}-{:02}-{:02}", y, m, d);
        let converter = decoder_for(field_type::DATE).expect("date converter");
        let result = converter(&s);
        if NaiveDate::from_ymd_opt(y, m, d).is_some() {
            prop_assert!(matches!(result, Value::Date(_)), "expected Date for {}, got {:?}", s, result);
        } else {
            prop_assert_eq!(result, Value::String(s));
        }
    }

    /// convert_datetime は有効な 'YYYY-MM-DD HH:MM:SS' 形式を DateTime に変換する。
    #[test]
    fn prop_convert_datetime_validity(
        (y, mo, d, h, mi, s) in
        (1970i32..=9999, 1u32..=12, 1u32..=31, 0u32..=23, 0u32..=59, 0u32..=59)
    ) {
        let input = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s);
        let converter = decoder_for(field_type::DATETIME).expect("datetime converter");
        let result = converter(&input);
        let expected = NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|date| date.and_hms_opt(h, mi, s));
        if expected.is_some() {
            prop_assert!(matches!(result, Value::DateTime(_)), "expected DateTime for {}, got {:?}", input, result);
        } else {
            prop_assert_eq!(result, Value::String(input));
        }
    }

    /// convert_timedelta は有効な '[-]HH:MM:SS[.ffffff]' 形式を TimeSpan に変換する。
    #[test]
    fn prop_convert_timedelta_validity(
        neg in any::<bool>(),
        h in 0u32..=999,
        m in 0u32..=59,
        s in 0u32..=59,
    ) {
        let prefix = if neg { "-" } else { "" };
        let input = format!("{}{:02}:{:02}:{:02}", prefix, h, m, s);
        let converter = decoder_for(field_type::TIME).expect("time converter");
        let result = converter(&input);
        prop_assert!(matches!(result, Value::TimeSpan(_)), "expected TimeSpan for {}, got {:?}", input, result);
    }

    /// decoder_for は任意のフィールド型に対して converter を返し、
    /// 任意の文字列入力でパニックしない。
    #[test]
    fn prop_decoder_for_no_panic(field_type in any::<u8>(), s in "\\PC*") {
        if let Some(converter) = decoder_for(field_type) {
            let _ = converter(&s);
        }
    }
}

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
                    // 未知のエスケープはそのまま保持。
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
