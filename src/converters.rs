// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! 値の変換処理。

use crate::constants::field_type;
use chrono::{NaiveDate, NaiveDateTime, NaiveTime};
use rust_decimal::Decimal;
use std::collections::HashSet;
use std::hash::Hasher;

/// データベース上で扱う値の型。
#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(String),
    Bytes(Vec<u8>),
    Date(NaiveDate),
    Time(NaiveTime),
    DateTime(NaiveDateTime),
    TimeSpan(chrono::TimeDelta),
    Decimal(Decimal),
    /// 要素のリスト。`(v1,v2,...)` の形式でエスケープされる。
    List(Vec<Value>),
    /// 要素の集合。`v1,v2,...` の形式でエスケープされる。
    ///
    /// 集合の要素は順序を持たないため、出力される順序は不定である。
    Set(HashSet<Value>),
}

impl std::hash::Hash for Value {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Value::Null => {}
            Value::Bool(b) => b.hash(state),
            Value::Int(i) => i.hash(state),
            // f64 は std の Hash を実装していないためビット表現でハッシュする。
            Value::Float(f) => f.to_bits().hash(state),
            Value::String(s) => s.hash(state),
            Value::Bytes(b) => b.hash(state),
            Value::Date(d) => d.hash(state),
            Value::Time(t) => t.hash(state),
            Value::DateTime(dt) => dt.hash(state),
            Value::TimeSpan(td) => td.hash(state),
            Value::Decimal(d) => d.hash(state),
            Value::List(items) => items.hash(state),
            Value::Set(items) => {
                // HashSet は順序を持たないため、要素のハッシュを XOR で合成する。
                // 等しい集合は同じ要素から構成されるため、常に同じハッシュになる。
                let mut combined = 0_u64;
                for item in items {
                    let mut hasher = std::collections::hash_map::DefaultHasher::new();
                    item.hash(&mut hasher);
                    combined ^= hasher.finish();
                }
                combined.hash(state);
            }
        }
    }
}

// HashSet<Value> の PartialEq には Value: Eq が必要なため、
// PartialEq と Hash を実装したうえで Eq をマーカーとして実装する。
// NaN を含む Float の等価性は PartialEq の定義に従う。
impl Eq for Value {}

impl Value {
    /// 値を SQL リテラルとしてエスケープする。
    pub fn to_sql(&self, encoding: &str) -> crate::error::Result<String> {
        match self {
            Value::Null => Ok("NULL".to_string()),
            Value::Bool(b) => Ok((if *b { 1 } else { 0 }).to_string()),
            Value::Int(i) => Ok(i.to_string()),
            Value::Float(f) => escape_float(*f),
            Value::String(s) => Ok(format!("'{}'", escape_string(s))),
            Value::Bytes(b) => Ok(escape_bytes(b, encoding)),
            Value::Date(d) => Ok(format!("'{}'", d.format("%Y-%m-%d"))),
            Value::Time(t) => Ok(format!(
                "'{}'",
                t.format("%H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
            )),
            Value::DateTime(dt) => Ok(format!(
                "'{}'",
                dt.format("%Y-%m-%d %H:%M:%S%.6f")
                    .to_string()
                    .trim_end_matches('0')
                    .trim_end_matches('.')
            )),
            Value::TimeSpan(td) => Ok(format!("'{}'", format_timedelta(td))),
            Value::Decimal(d) => Ok(d.to_string()),
            Value::List(items) => escape_sequence(items, encoding),
            Value::Set(items) => escape_set(items, encoding),
        }
    }
}

/// 型変換関数の型エイリアス。
pub type Converter = fn(&str) -> Value;

/// 値を SQL リテラルに変換する。
pub fn escape_item(value: &Value, encoding: &str) -> crate::error::Result<String> {
    value.to_sql(encoding)
}

/// 値のリストを `(v1,v2,...)` の形式に変換する。
///
/// PyMySQL の `escape_sequence` に相当する。
pub fn escape_sequence(values: &[Value], encoding: &str) -> crate::error::Result<String> {
    let items = values
        .iter()
        .map(|value| value.to_sql(encoding))
        .collect::<crate::error::Result<Vec<_>>>()?;
    Ok(format!("({})", items.join(",")))
}

/// 値の集合を `v1,v2,...` の形式に変換する。
///
/// PyMySQL の `escape_set` に相当する。集合は順序を持たないため、
/// 出力される順序は不定である。
pub fn escape_set(values: &HashSet<Value>, encoding: &str) -> crate::error::Result<String> {
    let items = values
        .iter()
        .map(|value| value.to_sql(encoding))
        .collect::<crate::error::Result<Vec<_>>>()?;
    Ok(items.join(","))
}

/// 浮動小数点数を MySQL 用に文字列化する。
fn escape_float(value: f64) -> crate::error::Result<String> {
    if !value.is_finite() {
        return Err(crate::error::Error::DataError {
            code: 0,
            message: format!("{} can not be used with MySQL", value),
        });
    }
    // PyMySQL と同じく repr() 相当の表現を使用し、指数部が無い場合は e0 を付与する。
    let s = format!("{:?}", value);
    if s.contains('e') || s.contains('E') {
        Ok(s)
    } else {
        Ok(format!("{}e0", s))
    }
}

/// 文字列をエスケープする。
pub fn escape_string(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\'', "\\'")
        .replace('\n', "\\n")
        .replace('\r', "\\r")
        .replace('\0', "\\0")
        .replace('\x1a', "\\Z")
        .replace('"', "\\\"")
}

/// バイト列を X'...' 16 進リテラルに変換する。
pub fn escape_bytes(value: &[u8], _encoding: &str) -> String {
    let hex = value
        .iter()
        .map(|b| format!("{:02X}", b))
        .collect::<String>();
    format!("X'{}'", hex)
}

fn format_timedelta(td: &chrono::TimeDelta) -> String {
    let neg = td.num_seconds() < 0 || (td.num_seconds() == 0 && td.subsec_micros() < 0);
    let total_seconds = td.num_seconds().abs();
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    let microseconds = td.subsec_micros().abs() as i64;
    let body = if microseconds > 0 {
        format!(
            "{:02}:{:02}:{:02}.{:06}",
            hours, minutes, seconds, microseconds
        )
    } else {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    };
    if neg { format!("-{}", body) } else { body }
}

/// フィールド型に対応するデコーダーを取得する。
pub fn decoder_for(field_type: u8) -> Option<Converter> {
    match field_type {
        field_type::TINY
        | field_type::SHORT
        | field_type::LONG
        | field_type::LONGLONG
        | field_type::INT24
        | field_type::YEAR => Some(convert_int),
        field_type::FLOAT | field_type::DOUBLE => Some(convert_float),
        field_type::TIMESTAMP | field_type::DATETIME => Some(convert_datetime),
        field_type::TIME => Some(convert_timedelta),
        field_type::DATE => Some(convert_date),
        field_type::DECIMAL | field_type::NEWDECIMAL => Some(convert_decimal),
        field_type::BLOB
        | field_type::TINY_BLOB
        | field_type::MEDIUM_BLOB
        | field_type::LONG_BLOB
        | field_type::STRING
        | field_type::VAR_STRING
        | field_type::VARCHAR => Some(convert_string),
        _ => Some(convert_string),
    }
}

fn convert_int(s: &str) -> Value {
    s.parse()
        .map_or_else(|_| Value::String(s.to_string()), Value::Int)
}

fn convert_float(s: &str) -> Value {
    s.parse()
        .map_or_else(|_| Value::String(s.to_string()), Value::Float)
}

fn convert_string(s: &str) -> Value {
    Value::String(s.to_string())
}

fn convert_decimal(s: &str) -> Value {
    match s.parse::<Decimal>() {
        Ok(d) if d.to_string() == s => Value::Decimal(d),
        _ => Value::String(s.to_string()),
    }
}

fn convert_date(s: &str) -> Value {
    match NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        Ok(d) => Value::Date(d),
        Err(_) => Value::String(s.to_string()),
    }
}

fn convert_datetime(s: &str) -> Value {
    // マイクロ秒部分は可変長なので手動でパースする。
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    let base = parts[0];
    let fmt = "%Y-%m-%d %H:%M:%S";
    match NaiveDateTime::parse_from_str(base, fmt) {
        Ok(dt) => {
            if parts.len() == 2 {
                let frac = parts[1].chars().take(6).collect::<String>();
                match frac.parse::<u32>() {
                    Ok(m) => {
                        let micros = m * 10_u32.pow(6 - frac.len() as u32);
                        Value::DateTime(dt + chrono::TimeDelta::microseconds(micros as i64))
                    }
                    Err(_) => Value::String(s.to_string()),
                }
            } else {
                Value::DateTime(dt)
            }
        }
        Err(_) => Value::String(s.to_string()),
    }
}

fn convert_timedelta(s: &str) -> Value {
    // MySQL の TIME 形式は [-]HH:MM:SS[.ffffff]。
    let neg = s.starts_with('-');
    let s = s.trim_start_matches('-');
    let parts: Vec<&str> = s.splitn(2, '.').collect();
    let time_part = parts[0];
    let mut secs: i64 = 0;
    for (i, p) in time_part.split(':').rev().enumerate() {
        let v: i64 = p.parse().unwrap_or(0);
        let multiplier = 60_i64.checked_pow(i as u32).unwrap_or(i64::MAX);
        secs = secs.saturating_add(v.saturating_mul(multiplier));
    }
    let micros = if parts.len() == 2 {
        let frac = parts[1].chars().take(6).collect::<String>();
        match frac.parse::<u32>() {
            Ok(m) => {
                let frac_len = frac.chars().count() as u32;
                let scale = if frac_len <= 6 {
                    10_i64.checked_pow(6 - frac_len).unwrap_or(1)
                } else {
                    1
                };
                (m as i64).saturating_mul(scale)
            }
            Err(_) => return Value::String(s.to_string()),
        }
    } else {
        0
    };
    let total = secs.saturating_mul(1_000_000).saturating_add(micros);
    // TimeDelta::microseconds は i64::MAX / 1_000 を超えると範囲外となるため制限する。
    let max_micros = i64::MAX / 1_000;
    let total = total.clamp(-max_micros, max_micros);
    let td = chrono::TimeDelta::microseconds(if neg { -total } else { total });
    Value::TimeSpan(td)
}
