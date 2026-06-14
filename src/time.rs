// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! 日時ユーティリティ。

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};

/// 日付型のエイリアス。
pub type Date = NaiveDate;
/// 時刻型のエイリアス。
pub type Time = NaiveTime;
/// 日時型のエイリアス。
pub type Timestamp = NaiveDateTime;

/// タイムスタンプから日付を生成する。
pub fn date_from_ticks(ticks: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp(ticks, 0).map(|dt| dt.date_naive())
}

/// タイムスタンプから時刻を生成する。
pub fn time_from_ticks(ticks: i64) -> Option<NaiveTime> {
    DateTime::from_timestamp(ticks, 0).map(|dt| dt.time())
}

/// タイムスタンプから日時を生成する。
pub fn timestamp_from_ticks(ticks: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(ticks, 0).map(|dt| dt.naive_utc())
}

/// 日付を生成する。
pub fn date(year: i32, month: u32, day: u32) -> Option<NaiveDate> {
    NaiveDate::from_ymd_opt(year, month, day)
}

/// 時刻を生成する。
pub fn time(hour: u32, minute: u32, second: u32) -> Option<NaiveTime> {
    NaiveTime::from_hms_opt(hour, minute, second)
}

/// 日時を生成する。
pub fn timestamp(
    year: i32,
    month: u32,
    day: u32,
    hour: u32,
    minute: u32,
    second: u32,
) -> Option<NaiveDateTime> {
    NaiveDate::from_ymd_opt(year, month, day).and_then(|d| d.and_hms_opt(hour, minute, second))
}
