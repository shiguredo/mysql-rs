// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! time モジュールの Property-Based Testing。
//!
//! 日時生成関数の一貫性と有効範囲を検証する。

use chrono::{DateTime, NaiveDate, NaiveTime};
use proptest::prelude::*;
use shiguredo_mysql_core::time::{
    date, date_from_ticks, time, time_from_ticks, timestamp, timestamp_from_ticks,
};

proptest! {
    /// date_from_ticks は chrono::DateTime::from_timestamp と一致する日付を返す。
    #[test]
    fn prop_date_from_ticks_consistency(ticks in -1_000_000_000_000i64..=1_000_000_000_000i64) {
        let result = date_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.date_naive();
            prop_assert_eq!(result, Some(expected));
        } else {
            prop_assert!(result.is_none());
        }
    }

    /// time_from_ticks は chrono::DateTime::from_timestamp と一致する時刻を返す。
    #[test]
    fn prop_time_from_ticks_consistency(ticks in -1_000_000_000_000i64..=1_000_000_000_000i64) {
        let result = time_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.time();
            prop_assert_eq!(result, Some(expected));
        } else {
            prop_assert!(result.is_none());
        }
    }

    /// timestamp_from_ticks は与えられた Unix タイムスタンプと一致する日時を返す。
    #[test]
    fn prop_timestamp_from_ticks_consistency(ticks in -1_000_000_000_000i64..=1_000_000_000_000i64) {
        let result = timestamp_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.naive_utc();
            prop_assert_eq!(result, Some(expected));
        } else {
            prop_assert!(result.is_none());
        }
    }

    /// date は有効な日付に対して Some を返し、無効な日付に対して None を返す。
    #[test]
    fn prop_date_validity((year, month, day) in (1i32..=9999, 1u32..=13, 1u32..=32)) {
        let result = date(year, month, day);
        prop_assert_eq!(result.is_some(), NaiveDate::from_ymd_opt(year, month, day).is_some());
    }

    /// time は有効な時刻に対して Some を返し、無効な時刻に対して None を返す。
    #[test]
    fn prop_time_validity((hour, minute, second) in (0u32..=25, 0u32..=61, 0u32..=61)) {
        let result = time(hour, minute, second);
        prop_assert_eq!(result.is_some(), NaiveTime::from_hms_opt(hour, minute, second).is_some());
    }

    /// timestamp は有効な日時に対して Some を返し、無効な日時に対して None を返す。
    #[test]
    fn prop_timestamp_validity(
        (year, month, day, hour, minute, second) in
        (1i32..=9999, 1u32..=13, 1u32..=32, 0u32..=25, 0u32..=61, 0u32..=61)
    ) {
        let result = timestamp(year, month, day, hour, minute, second);
        let expected = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, minute, second));
        prop_assert_eq!(result, expected);
    }

    /// 閏年と月末の境界を検証する。
    #[test]
    fn prop_date_edge_cases(year in 1990i32..=2100, month in 1u32..=12, day in 28u32..=31) {
        let result = date(year, month, day);
        let expected = NaiveDate::from_ymd_opt(year, month, day);
        prop_assert_eq!(result, expected);
    }

    /// 時刻の境界（24 時、60 秒、60 分）を検証する。
    #[test]
    fn prop_time_edge_cases(
        hour in 0u32..=24,
        minute in 0u32..=60,
        second in 0u32..=60,
    ) {
        let result = time(hour, minute, second);
        let expected = NaiveTime::from_hms_opt(hour, minute, second);
        prop_assert_eq!(result, expected);
    }

    /// ticks 系関数は互いに整合する日付・時刻・日時を返す。
    #[test]
    fn prop_ticks_consistency(ticks in -1_000_000_000_000i64..=1_000_000_000_000i64) {
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            prop_assert_eq!(date_from_ticks(ticks), Some(dt.date_naive()));
            prop_assert_eq!(time_from_ticks(ticks), Some(dt.time()));
            prop_assert_eq!(timestamp_from_ticks(ticks), Some(dt.naive_utc()));
            // timestamp_from_ticks の日付部分は date_from_ticks と一致する。
            if let (Some(ts), Some(d)) = (timestamp_from_ticks(ticks), date_from_ticks(ticks)) {
                prop_assert_eq!(ts.date(), d);
            }
        } else {
            prop_assert!(date_from_ticks(ticks).is_none());
            prop_assert!(time_from_ticks(ticks).is_none());
            prop_assert!(timestamp_from_ticks(ticks).is_none());
        }
    }

}
