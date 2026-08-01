// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

use shiguredo_mysql::constants::field_type;
use shiguredo_mysql::converters::{Value, decoder_for, escape_bytes, escape_string};
use std::collections::HashSet;

#[test]
fn test_escape_string() {
    assert_eq!(
        escape_string("hello"),
        "hello",
        "通常文字列はエスケープしないべき"
    );
    assert_eq!(
        escape_string("it's"),
        "it\\'s",
        "シングルクォートはエスケープするべき"
    );
    assert_eq!(
        escape_string("a\nb"),
        "a\\nb",
        "改行文字はエスケープするべき"
    );
}

#[test]
fn test_escape_bytes() {
    assert_eq!(
        escape_bytes(b"hello", "utf8"),
        "X'68656C6C6F'",
        "バイト列は 16 進リテラルに変換するべき"
    );
    assert_eq!(
        escape_bytes(b"it's", "utf8"),
        "X'69742773'",
        "クォートを含むバイト列も 16 進リテラルに変換するべき"
    );
    assert_eq!(
        escape_bytes(&[], "utf8"),
        "X''",
        "空バイト列も正しく変換するべき"
    );
}

#[test]
fn test_value_to_sql() {
    assert_eq!(
        Value::Null.to_sql("utf8").unwrap(),
        "NULL",
        "NULL は NULL リテラルに変換するべき"
    );
    assert_eq!(
        Value::Int(42).to_sql("utf8").unwrap(),
        "42",
        "整数はそのまま文字列化するべき"
    );
    assert_eq!(
        Value::Bool(true).to_sql("utf8").unwrap(),
        "1",
        "真偽値は 1/0 に変換するべき"
    );
    assert_eq!(
        Value::String("hello".to_string()).to_sql("utf8").unwrap(),
        "'hello'",
        "文字列はクォートで囲むべき"
    );
    assert_eq!(
        Value::String("it's".to_string()).to_sql("utf8").unwrap(),
        "'it\\'s'",
        "文字列内の特殊文字はエスケープするべき"
    );
}

#[test]
fn test_escape_float_non_finite() {
    assert!(
        Value::Float(f64::NAN).to_sql("utf8").is_err(),
        "NaN は SQL リテラルにできないべき"
    );
    assert!(
        Value::Float(f64::INFINITY).to_sql("utf8").is_err(),
        "正の無限大は SQL リテラルにできないべき"
    );
    assert!(
        Value::Float(f64::NEG_INFINITY).to_sql("utf8").is_err(),
        "負の無限大は SQL リテラルにできないべき"
    );
}

#[test]
fn test_value_list_to_sql() {
    let list = Value::List(vec![Value::Int(1), Value::String("a".to_string())]);
    assert_eq!(
        list.to_sql("utf8").unwrap(),
        "(1,'a')",
        "リストはカンマ区切りで括弧で囲むべき"
    );
    assert_eq!(
        Value::List(vec![]).to_sql("utf8").unwrap(),
        "()",
        "空リストは空の括弧にするべき"
    );
    assert_eq!(
        Value::List(vec![Value::Int(1)]).to_sql("utf8").unwrap(),
        "(1)",
        "要素が 1 つのリストも括弧で囲むべき"
    );
}

#[test]
fn test_value_set_to_sql() {
    let mut set = HashSet::new();
    set.insert(Value::Int(1));
    set.insert(Value::Int(2));
    let sql = Value::Set(set).to_sql("utf8").unwrap();
    // 集合は順序を持たないため、要素ごとに検証する。
    let mut values: Vec<&str> = sql.split(',').collect();
    values.sort_unstable();
    assert_eq!(values, vec!["1", "2"]);
    assert_eq!(
        Value::Set(HashSet::new()).to_sql("utf8").unwrap(),
        "",
        "空集合は空文字列にするべき"
    );
}

#[test]
fn test_value_set_hash() {
    use std::collections::HashSet as Set;
    use std::hash::BuildHasher;

    // 要素の挿入順が異なる同一の集合は、同じハッシュになるべき。
    let a = Value::Set(Set::from([Value::Int(1), Value::String("x".to_string())]));
    let b = Value::Set(Set::from([Value::String("x".to_string()), Value::Int(1)]));

    let state = std::collections::hash_map::RandomState::new();
    assert_eq!(state.hash_one(&a), state.hash_one(&b));
}

#[test]
fn test_negative_timedelta_to_sql() {
    use chrono::TimeDelta;

    let td = TimeDelta::seconds(-1);
    assert_eq!(
        Value::TimeSpan(td).to_sql("utf8").unwrap(),
        "'-00:00:01'",
        "負の TimeDelta は符号付きで出力するべき"
    );

    let td = TimeDelta::microseconds(-500000);
    assert_eq!(
        Value::TimeSpan(td).to_sql("utf8").unwrap(),
        "'-00:00:00.500000'",
        "負のマイクロ秒も符号付きで出力するべき"
    );
}

#[test]
fn test_convert_int_fallback() {
    let convert = decoder_for(field_type::LONGLONG).unwrap();
    assert_eq!(
        convert("18446744073709551615"),
        Value::String("18446744073709551615".to_string()),
        "i64 に収まらない整数は String にフォールバックするべき"
    );
}

#[test]
fn test_convert_bit() {
    // PyMySQL と同じく BIT 型は加工せず文字列として返す。
    let convert = decoder_for(field_type::BIT).unwrap();
    assert_eq!(
        convert("\x01"),
        Value::String("\x01".to_string()),
        "BIT 型は文字列として返すべき"
    );
    assert_eq!(
        convert("\x00\x0a"),
        Value::String("\x00\x0a".to_string()),
        "複数バイトの BIT も文字列として返すべき"
    );
}

#[test]
fn test_convert_decimal_fallback() {
    let convert = decoder_for(field_type::NEWDECIMAL).unwrap();
    assert_eq!(
        convert("9999999999999999999999999999.99"),
        Value::String("9999999999999999999999999999.99".to_string()),
        "精度が大きすぎる Decimal は String にフォールバックするべき"
    );
}
