// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! converters モジュールの Property-Based Testing。
//!
//! エスケープ関数の出力が安全な形式を満たすこと、および値の変換が
//! 決定的に動作することを検証する。

use std::cell::Cell;
use std::collections::HashSet;

use chrono::{NaiveDate, NaiveTime, TimeDelta};
use rust_decimal::Decimal;
use shiguredo_mysql_core::constants::field_type;
use shiguredo_mysql_core::converters::{Value, decoder_for, escape_bytes, escape_string};

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

/// エスケープ検証用の文字プール。
///
/// 素の ASCII に加えて、エスケープ対象の全クラス（`'`・`"`・`\`・改行・
/// 復帰・NUL・0x1A）と非 ASCII 文字を含む。完全な Unicode 範囲から引くと
/// これらの特定文字にほぼ到達できないため、小さなプールで重点的に生成する。
const ESCAPE_POOL: [char; 12] = [
    'a', '\'', '"', '\\', '\n', '\r', '\0', '\x1a', 'あ', 'Z', '0', 'e',
];

/// 数値系デコーダー検証用の文字プール（数字・符号・区切り文字）。
const NUMERIC_POOL: &[u8] = b"0123456789-+:. eE";

/// エスケープ検証用の文字列をサンプリングする。
fn sample_escape_input(ctx: &mut noprop::TestCaseContext) -> String {
    // 空・最大長にも確率を与えて境界を重点的に生成する。
    let len =
        noprop::sample_with_boundaries(ctx, &[0usize, 1, 32], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 0..=32)
        });
    (0..len)
        .map(|_| noprop::sample_choice(ctx, &ESCAPE_POOL))
        .collect()
}

/// 有効な日付をサンプリングする。
fn sample_valid_date(ctx: &mut noprop::TestCaseContext) -> NaiveDate {
    // 無効な組み合わせ（2 月 30 日等）は棄却して引き直す。
    // 受け入れ率が高く、16 回の試行で枯渇する確率は無視できる。
    noprop::sample_with_rejection(ctx, 16, |ctx| {
        let y = noprop::sample_usize_in(ctx, 1970..=9999) as i32;
        let m = noprop::sample_usize_in(ctx, 1..=12) as u32;
        let d = noprop::sample_usize_in(ctx, 1..=31) as u32;
        NaiveDate::from_ymd_opt(y, m, d)
    })
}

/// 有効な日時をサンプリングする。
fn sample_valid_datetime(ctx: &mut noprop::TestCaseContext) -> chrono::NaiveDateTime {
    noprop::sample_with_rejection(ctx, 16, |ctx| {
        let y = noprop::sample_usize_in(ctx, 1970..=9999) as i32;
        let mo = noprop::sample_usize_in(ctx, 1..=12) as u32;
        let d = noprop::sample_usize_in(ctx, 1..=31) as u32;
        let h = noprop::sample_usize_in(ctx, 0..=23) as u32;
        let mi = noprop::sample_usize_in(ctx, 0..=59) as u32;
        let s = noprop::sample_usize_in(ctx, 0..=59) as u32;
        NaiveDate::from_ymd_opt(y, mo, d).and_then(|date| date.and_hms_opt(h, mi, s))
    })
}

/// リーフの値をサンプリングする（11 バリアントから一様に選択する）。
fn sample_leaf_value(ctx: &mut noprop::TestCaseContext) -> Value {
    match noprop::sample_usize_in(ctx, 0..11) {
        0 => Value::Null,
        1 => Value::Bool(noprop::sample_bool(ctx)),
        2 => Value::Int(noprop::sample_i64(ctx)),
        // 有限値のみを生成する（無限・NaN は MySQL で扱えない）。
        3 => Value::Float(noprop::sample_f64(ctx)),
        4 => {
            let len = noprop::sample_usize_in(ctx, 0..=32);
            Value::String(noprop::sample_string(ctx, len))
        }
        5 => {
            let len = noprop::sample_usize_in(ctx, 0..=32);
            Value::Bytes(noprop::sample_bytes_vec(ctx, len))
        }
        6 => Value::Date(sample_valid_date(ctx)),
        7 => {
            let h = noprop::sample_usize_in(ctx, 0..=23) as u32;
            let m = noprop::sample_usize_in(ctx, 0..=59) as u32;
            let s = noprop::sample_usize_in(ctx, 0..=59) as u32;
            Value::Time(NaiveTime::from_hms_opt(h, m, s).expect("有効な時刻の生成に失敗"))
        }
        8 => Value::DateTime(sample_valid_datetime(ctx)),
        9 => {
            // 符号付き範囲はオフセット付きで有効値のみを生成する。
            let v = noprop::sample_u64_in(ctx, 0..=2_000_000_000_000u64);
            Value::TimeSpan(TimeDelta::seconds(v as i64 - 1_000_000_000_000i64))
        }
        _ => Value::Decimal(Decimal::from(noprop::sample_i64(ctx))),
    }
}

/// コレクションの要素数をサンプリングする（空・最大に確率を与える）。
fn sample_collection_len(ctx: &mut noprop::TestCaseContext) -> usize {
    noprop::sample_with_boundaries(ctx, &[0usize, 1, 4], noprop::Ratio::one_nth(2), |ctx| {
        noprop::sample_usize_in(ctx, 0..=4)
    })
}

/// テスト用の Value をサンプリングする（深さ制限付きの再帰）。
fn sample_value(ctx: &mut noprop::TestCaseContext, depth: usize) -> Value {
    if depth == 0 {
        return sample_leaf_value(ctx);
    }
    // リーフを 6 割、List・Set を各 2 割で生成する。
    match noprop::sample_weighted_index(ctx, &[6, 2, 2]) {
        0 => sample_leaf_value(ctx),
        1 => {
            let len = sample_collection_len(ctx);
            Value::List((0..len).map(|_| sample_value(ctx, depth - 1)).collect())
        }
        _ => {
            let len = sample_collection_len(ctx);
            Value::Set((0..len).map(|_| sample_value(ctx, depth - 1)).collect())
        }
    }
}

/// Value のバリアント名を返す（カバレッジ記録用）。
fn value_kind(value: &Value) -> &'static str {
    match value {
        Value::Null => "Null",
        Value::Bool(_) => "Bool",
        Value::Int(_) => "Int",
        Value::Float(_) => "Float",
        Value::String(_) => "String",
        Value::Bytes(_) => "Bytes",
        Value::Date(_) => "Date",
        Value::Time(_) => "Time",
        Value::DateTime(_) => "DateTime",
        Value::TimeSpan(_) => "TimeSpan",
        Value::Decimal(_) => "Decimal",
        Value::List(_) => "List",
        Value::Set(_) => "Set",
    }
}

/// デコーダー検証用の入力文字列をサンプリングする。
///
/// 数値らしい文字列と任意文字列を半々で生成して、
/// パース成功・失敗の両方に到達可能にする。
/// 任意文字列だけでは数値パースがほぼ成功せず、検証が空振りになるため。
fn sample_decoder_input(ctx: &mut noprop::TestCaseContext) -> (String, bool) {
    match noprop::sample_weighted_index(ctx, &[1, 1]) {
        0 => {
            let len = noprop::sample_with_boundaries(
                ctx,
                &[0usize, 1, 16],
                noprop::Ratio::one_nth(5),
                |ctx| noprop::sample_usize_in(ctx, 0..=16),
            );
            let s: String = (0..len)
                .map(|_| noprop::sample_choice(ctx, NUMERIC_POOL) as char)
                .collect();
            (s, true)
        }
        _ => {
            let len = noprop::sample_usize_in(ctx, 0..=32);
            (noprop::sample_string(ctx, len), false)
        }
    }
}

/// escape_string の結果に、エスケープされていないシングルクォートは含まれない。
#[test]
fn prop_escape_string_no_unescaped_quote() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // クォートを含む入力が実行されたことを記録する。
    let with_quote = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let s = sample_escape_input(ctx);
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        for i in 0..bytes.len() {
            if bytes[i] == b'\'' {
                assert!(
                    i > 0 && bytes[i - 1] == b'\\',
                    "エスケープされていないクォートが含まれる"
                );
            }
        }
        if s.contains('\'') {
            with_quote.set(with_quote.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        with_quote.get() > 0,
        "クォートを含む入力が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// escape_bytes の結果は X'...' 形式の 16 進リテラルである。
#[test]
fn prop_escape_bytes_hex_literal() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let non_empty = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let len = noprop::sample_with_boundaries(
            ctx,
            &[0usize, 1, 256],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 0..=256),
        );
        let data = noprop::sample_bytes_vec(ctx, len);
        let escaped = escape_bytes(&data, "utf8");
        assert!(
            escaped.starts_with("X'") && escaped.ends_with('\''),
            "16 進リテラル形式でない"
        );
        let inner = &escaped[2..escaped.len() - 1];
        assert!(
            inner.chars().all(|c| c.is_ascii_hexdigit()),
            "16 進数以外の文字が含まれる"
        );
        if data.is_empty() {
            empty.set(empty.get() + 1);
        } else {
            non_empty.set(non_empty.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空バイト列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty.get() > 0,
        "非空バイト列が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// escape_string の結果に、孤立したバックスラッシュは含まれない。
#[test]
fn prop_escape_string_no_lone_backslash() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // バックスラッシュを含む入力が実行されたことを記録する。
    let with_backslash = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let s = sample_escape_input(ctx);
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'\\' {
                // バックスラッシュは必ず次の 1 文字とペアになっている。
                assert!(i + 1 < bytes.len(), "末尾に孤立したバックスラッシュがある");
                i += 2;
            } else {
                i += 1;
            }
        }
        if s.contains('\\') {
            with_backslash.set(with_backslash.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        with_backslash.get() > 0,
        "バックスラッシュを含む入力が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// escape_string の結果に、エスケープ対象文字（バックスラッシュを除く）が
/// 単体で出現しない。バックスラッシュはエスケープ文字そのものなので先頭に来てもよい。
#[test]
fn prop_escape_string_no_unescaped_special_chars() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 特殊文字を含む入力が実行されたことを記録する。
    let with_special = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let s = sample_escape_input(ctx);
        let escaped = escape_string(&s);
        let bytes = escaped.as_bytes();
        for i in 0..bytes.len() {
            match bytes[i] {
                b'\'' | b'"' | b'\n' | b'\r' | b'\0' | 0x1a => {
                    assert!(
                        i > 0 && bytes[i - 1] == b'\\',
                        "エスケープされていない特殊文字が含まれる"
                    );
                }
                _ => {}
            }
        }
        if s.chars()
            .any(|c| matches!(c, '\'' | '"' | '\n' | '\r' | '\0' | '\x1a'))
        {
            with_special.set(with_special.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        with_special.get() > 0,
        "特殊文字を含む入力が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// escape_string をアンエスケープすると元の文字列に戻る。
#[test]
fn prop_escape_string_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 特殊文字を含む入力が実行されたことを記録する。
    let with_special = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let s = sample_escape_input(ctx);
        let escaped = escape_string(&s);
        let unescaped = unescape_string(&escaped);
        assert_eq!(unescaped, s, "アンエスケープ結果が元に戻らない");
        if s.chars()
            .any(|c| matches!(c, '\'' | '"' | '\\' | '\n' | '\r' | '\0' | '\x1a'))
        {
            with_special.set(with_special.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        with_special.get() > 0,
        "特殊文字を含む入力が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// Value::to_sql は決定的に動作し、各型に応じた MySQL リテラル形式を満たす。
#[test]
fn prop_value_to_sql_format() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全バリアントが実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    // 空の List・Set が実行されたことを記録する。
    let empty_list = Cell::new(0usize);
    let empty_set = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = sample_value(ctx, 2);
        let sql = value.to_sql("utf8").expect("SQL リテラルへの変換に失敗");
        let sql2 = value.to_sql("utf8").expect("SQL リテラルへの変換に失敗");
        assert_eq!(sql.clone(), sql2, "同じ値に対する変換結果が一致しない");

        match &value {
            Value::Null => assert_eq!(sql, "NULL", "NULL リテラルでない"),
            Value::Bool(b) => assert_eq!(sql, if *b { "1" } else { "0" }, "真偽リテラルでない"),
            Value::Int(i) => {
                assert_eq!(sql, i.to_string(), "整数リテラルでない");
            }
            Value::Decimal(d) => {
                assert_eq!(sql, d.to_string(), "DECIMAL リテラルでない");
            }
            Value::Float(_f) => {
                assert!(!sql.is_empty(), "Float リテラルが空");
                // 有限値の Float は必ず数字または 'e' を含む。
                assert!(
                    sql.chars().any(|c| c.is_ascii_digit() || c == 'e'),
                    "Float リテラルに数字も指数もない"
                );
            }
            Value::String(s) => {
                assert!(
                    sql.starts_with('\'') && sql.ends_with('\''),
                    "文字列リテラルがクォートで囲まれていない"
                );
                let inner = &sql[1..sql.len() - 1];
                assert_eq!(
                    unescape_string(inner),
                    s.as_str(),
                    "文字列の中身が一致しない"
                );
            }
            Value::Bytes(_) => {
                assert!(
                    sql.starts_with("X'") && sql.ends_with('\''),
                    "バイト列が 16 進リテラルでない"
                );
            }
            Value::Date(d) => {
                assert!(
                    sql.starts_with('\'') && sql.ends_with('\''),
                    "日付リテラルがクォートで囲まれていない"
                );
                assert_eq!(
                    &sql[1..sql.len() - 1],
                    d.format("%Y-%m-%d").to_string(),
                    "日付形式でない"
                );
            }
            Value::Time(t) => {
                assert!(
                    sql.starts_with('\'') && sql.ends_with('\''),
                    "時刻リテラルがクォートで囲まれていない"
                );
                let expected = t
                    .format("%H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                assert_eq!(&sql[1..sql.len() - 1], expected, "時刻形式でない");
            }
            Value::DateTime(dt) => {
                assert!(
                    sql.starts_with('\'') && sql.ends_with('\''),
                    "日時リテラルがクォートで囲まれていない"
                );
                let expected = dt
                    .format("%Y-%m-%d %H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .to_string();
                assert_eq!(&sql[1..sql.len() - 1], expected, "日時形式でない");
            }
            Value::TimeSpan(_td) => {
                assert!(
                    sql.starts_with('\'') && sql.ends_with('\''),
                    "TimeSpan リテラルがクォートで囲まれていない"
                );
                let inner = &sql[1..sql.len() - 1];
                // TimeSpan は '[-]HH:MM:SS[.ffffff]' 形式。
                assert!(inner.split(':').count() == 3, "TimeSpan 形式でない");
            }
            Value::List(items) => {
                if items.is_empty() {
                    assert_eq!(sql, "()", "空リストが () でない");
                    empty_list.set(empty_list.get() + 1);
                } else {
                    assert!(
                        sql.starts_with('(') && sql.ends_with(')'),
                        "リストが括弧で囲まれていない"
                    );
                }
            }
            Value::Set(items) => {
                if items.is_empty() {
                    assert_eq!(sql, "", "空集合が空文字列でない");
                    empty_set.set(empty_set.get() + 1);
                }
            }
        }
        seen.borrow_mut().insert(value_kind(&value));
        Ok(())
    })?;
    let seen = seen.borrow();
    for kind in [
        "Null", "Bool", "Int", "Float", "String", "Bytes", "Date", "Time", "DateTime", "TimeSpan",
        "Decimal", "List", "Set",
    ] {
        assert!(
            seen.contains(kind),
            "バリアント {kind} が 1 件も実行されなかった\n{runner}"
        );
    }
    assert!(
        empty_list.get() > 0,
        "空リストが 1 件も実行されなかった\n{runner}"
    );
    assert!(
        empty_set.get() > 0,
        "空集合が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// decoder_for はフィールド型に応じた converter を返す。
#[test]
fn prop_decoder_for_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 数値らしい入力・任意入力の両方が実行されたことを記録する。
    let numeric_input = Cell::new(0usize);
    let arbitrary_input = Cell::new(0usize);
    // パース成功（Int・Float）が観測されたことを記録する。
    let int_success = Cell::new(0usize);
    let float_success = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let (input, is_numeric) = sample_decoder_input(ctx);
        if is_numeric {
            numeric_input.set(numeric_input.get() + 1);
        } else {
            arbitrary_input.set(arbitrary_input.get() + 1);
        }
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
            let converter = decoder_for(ft).expect("整数 converter の取得に失敗");
            let value = converter(&input);
            assert!(
                matches!(value, Value::Int(_) | Value::String(_)),
                "フィールド型 {ft} は Int か String を返すべき: {value:?}"
            );
            if matches!(value, Value::Int(_)) {
                int_success.set(int_success.get() + 1);
            }
        }

        let float_types = [field_type::FLOAT, field_type::DOUBLE];
        for ft in float_types {
            let converter = decoder_for(ft).expect("浮動小数 converter の取得に失敗");
            let value = converter(&input);
            assert!(
                matches!(value, Value::Float(_) | Value::String(_)),
                "フィールド型 {ft} は Float か String を返すべき: {value:?}"
            );
            if matches!(value, Value::Float(_)) {
                float_success.set(float_success.get() + 1);
            }
        }

        let decimal_types = [field_type::DECIMAL, field_type::NEWDECIMAL];
        for ft in decimal_types {
            let converter = decoder_for(ft).expect("DECIMAL converter の取得に失敗");
            let value = converter(&input);
            assert!(
                matches!(value, Value::Decimal(_) | Value::String(_)),
                "フィールド型 {ft} は Decimal か String を返すべき: {value:?}"
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
            let converter = decoder_for(ft).expect("文字列 converter の取得に失敗");
            let value = converter(&input);
            assert!(
                matches!(value, Value::String(_)),
                "フィールド型 {ft} は String を返すべき: {value:?}"
            );
        }

        let date_converter = decoder_for(field_type::DATE).expect("日付 converter の取得に失敗");
        assert!(matches!(
            date_converter(&input),
            Value::Date(_) | Value::String(_)
        ));

        let datetime_converter =
            decoder_for(field_type::DATETIME).expect("日時 converter の取得に失敗");
        assert!(matches!(
            datetime_converter(&input),
            Value::DateTime(_) | Value::String(_)
        ));

        let time_converter = decoder_for(field_type::TIME).expect("時刻 converter の取得に失敗");
        assert!(matches!(
            time_converter(&input),
            Value::TimeSpan(_) | Value::String(_)
        ));
        Ok(())
    })?;
    assert!(
        numeric_input.get() > 0,
        "数値らしい入力が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        arbitrary_input.get() > 0,
        "任意入力が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        int_success.get() > 0,
        "整数パースの成功が 1 件も観測されなかった\n{runner}"
    );
    assert!(
        float_success.get() > 0,
        "浮動小数パースの成功が 1 件も観測されなかった\n{runner}"
    );
    Ok(())
}

/// convert_date は有効な 'YYYY-MM-DD' 形式を Date に、無効な形式を String に変換する。
#[test]
fn prop_convert_date_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let y = noprop::sample_usize_in(ctx, 1970..=9999) as i32;
        let m = noprop::sample_usize_in(ctx, 1..=12) as u32;
        // 月末境界（29〜31 日）に確率を与えて無効な日付にも到達可能にする。
        let d = noprop::sample_with_boundaries(
            ctx,
            &[29u32, 30, 31],
            noprop::Ratio::one_nth(3),
            |ctx| noprop::sample_usize_in(ctx, 1..=31) as u32,
        );
        let s = format!("{:04}-{:02}-{:02}", y, m, d);
        let converter = decoder_for(field_type::DATE).expect("日付 converter の取得に失敗");
        let result = converter(&s);
        if NaiveDate::from_ymd_opt(y, m, d).is_some() {
            assert!(
                matches!(result, Value::Date(_)),
                "{s} は Date に変換されるべき: {result:?}"
            );
            valid.set(valid.get() + 1);
        } else {
            assert_eq!(result, Value::String(s.clone()), "{s} は String に残るべき");
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な日付が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な日付が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// convert_datetime は有効な 'YYYY-MM-DD HH:MM:SS' 形式を DateTime に変換する。
#[test]
fn prop_convert_datetime_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let y = noprop::sample_usize_in(ctx, 1970..=9999) as i32;
        let mo = noprop::sample_usize_in(ctx, 1..=12) as u32;
        // 月末境界に確率を与えて無効な日付にも到達可能にする。
        let d = noprop::sample_with_boundaries(
            ctx,
            &[29u32, 30, 31],
            noprop::Ratio::one_nth(3),
            |ctx| noprop::sample_usize_in(ctx, 1..=31) as u32,
        );
        let h = noprop::sample_usize_in(ctx, 0..=23) as u32;
        let mi = noprop::sample_usize_in(ctx, 0..=59) as u32;
        let s = noprop::sample_usize_in(ctx, 0..=59) as u32;
        let input = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, mo, d, h, mi, s);
        let converter = decoder_for(field_type::DATETIME).expect("日時 converter の取得に失敗");
        let result = converter(&input);
        let expected =
            NaiveDate::from_ymd_opt(y, mo, d).and_then(|date| date.and_hms_opt(h, mi, s));
        if expected.is_some() {
            assert!(
                matches!(result, Value::DateTime(_)),
                "{input} は DateTime に変換されるべき: {result:?}"
            );
            valid.set(valid.get() + 1);
        } else {
            assert_eq!(
                result,
                Value::String(input.clone()),
                "{input} は String に残るべき"
            );
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な日時が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な日時が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// convert_timedelta は有効な '[-]HH:MM:SS[.ffffff]' 形式を TimeSpan に変換する。
#[test]
fn prop_convert_timedelta_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 正・負の両方が実行されたことを記録する。
    let positive = Cell::new(0usize);
    let negative = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let neg = noprop::sample_bool(ctx);
        let h = noprop::sample_usize_in(ctx, 0..=999) as u32;
        let m = noprop::sample_usize_in(ctx, 0..=59) as u32;
        let s = noprop::sample_usize_in(ctx, 0..=59) as u32;
        let prefix = if neg { "-" } else { "" };
        let input = format!("{}{:02}:{:02}:{:02}", prefix, h, m, s);
        let converter = decoder_for(field_type::TIME).expect("時刻 converter の取得に失敗");
        let result = converter(&input);
        assert!(
            matches!(result, Value::TimeSpan(_)),
            "{input} は TimeSpan に変換されるべき: {result:?}"
        );
        if neg {
            negative.set(negative.get() + 1);
        } else {
            positive.set(positive.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        positive.get() > 0,
        "正の TimeSpan が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        negative.get() > 0,
        "負の TimeSpan が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// decoder_for は任意のフィールド型に対して converter を返し、
/// 任意の文字列入力でパニックしない。
#[test]
fn prop_decoder_for_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let field_type = noprop::sample_u8(ctx);
        let s = sample_escape_input(ctx);
        if let Some(converter) = decoder_for(field_type) {
            let _ = converter(&s);
        }
        Ok(())
    })?;
    Ok(())
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
