// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! time モジュールの Property-Based Testing。
//!
//! 日時生成関数の一貫性と有効範囲を検証する。

use std::cell::Cell;

use chrono::{DateTime, NaiveDate, NaiveTime};
use shiguredo_mysql_core::time::{
    date, date_from_ticks, time, time_from_ticks, timestamp, timestamp_from_ticks,
};

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

/// Unix タイムスタンプのサンプリング範囲の絶対値。
const TICKS_ABS_MAX: u64 = 1_000_000_000_000;

/// テスト対象の ticks 値をサンプリングする。
///
/// 有効範囲内（`-TICKS_ABS_MAX..=TICKS_ABS_MAX`）を重点的に生成しつつ、
/// 全範囲のランダム値や境界値（エポック・最小・最大）も混ぜて
/// `DateTime::from_timestamp` が `None` を返す無効値も到達可能にする。
/// 従来の `-1e12..=1e12` だけでは無効値に到達できず、
/// 無効系の分岐が空振りになるため。
fn sample_ticks(ctx: &mut noprop::TestCaseContext) -> i64 {
    match noprop::sample_weighted_index(ctx, &[6, 2, 1]) {
        // 有効範囲内を重点的に生成する。
        0 => {
            let v = noprop::sample_u64_in(ctx, 0..=TICKS_ABS_MAX * 2);
            v as i64 - TICKS_ABS_MAX as i64
        }
        // 全範囲から生成する（範囲外の無効値を主に狙う）。
        1 => noprop::sample_i64(ctx),
        // 境界値（エポック・最小・最大）。
        _ => noprop::sample_choice(ctx, &[0, i64::MIN, i64::MAX]),
    }
}

/// date_from_ticks は chrono::DateTime::from_timestamp と一致する日付を返す。
#[test]
fn prop_date_from_ticks_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ticks = sample_ticks(ctx);
        let result = date_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.date_naive();
            assert_eq!(result, Some(expected), "ticks {ticks} の日付が一致しない");
            valid.set(valid.get() + 1);
        } else {
            assert!(
                result.is_none(),
                "範囲外の ticks {ticks} は None を返すべき"
            );
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な ticks が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な ticks が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// time_from_ticks は chrono::DateTime::from_timestamp と一致する時刻を返す。
#[test]
fn prop_time_from_ticks_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ticks = sample_ticks(ctx);
        let result = time_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.time();
            assert_eq!(result, Some(expected), "ticks {ticks} の時刻が一致しない");
            valid.set(valid.get() + 1);
        } else {
            assert!(
                result.is_none(),
                "範囲外の ticks {ticks} は None を返すべき"
            );
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な ticks が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な ticks が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// timestamp_from_ticks は与えられた Unix タイムスタンプと一致する日時を返す。
#[test]
fn prop_timestamp_from_ticks_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ticks = sample_ticks(ctx);
        let result = timestamp_from_ticks(ticks);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            let expected = dt.naive_utc();
            assert_eq!(result, Some(expected), "ticks {ticks} の日時が一致しない");
            valid.set(valid.get() + 1);
        } else {
            assert!(
                result.is_none(),
                "範囲外の ticks {ticks} は None を返すべき"
            );
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な ticks が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な ticks が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// date は有効な日付に対して Some を返し、無効な日付に対して None を返す。
#[test]
fn prop_date_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 月に 13、日��に 32 を含めて無効な入力も生成する。
        let year = noprop::sample_usize_in(ctx, 1..=9999) as i32;
        let month = noprop::sample_usize_in(ctx, 1..=13) as u32;
        let day = noprop::sample_usize_in(ctx, 1..=32) as u32;
        let result = date(year, month, day);
        assert_eq!(
            result.is_some(),
            NaiveDate::from_ymd_opt(year, month, day).is_some(),
            "日付の有効性が一致しない: {year}-{month:02}-{day:02}"
        );
        if result.is_some() {
            valid.set(valid.get() + 1);
        } else {
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

/// time は有効な時刻に対して Some を返し、無効な時刻に対して None を返す。
#[test]
fn prop_time_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 境界外の 24 時・60 分・60 秒を含めて無効な入力も生成する。
        let hour = noprop::sample_usize_in(ctx, 0..=25) as u32;
        let minute = noprop::sample_usize_in(ctx, 0..=61) as u32;
        let second = noprop::sample_usize_in(ctx, 0..=61) as u32;
        let result = time(hour, minute, second);
        assert_eq!(
            result.is_some(),
            NaiveTime::from_hms_opt(hour, minute, second).is_some(),
            "時刻の有効性が一致しない: {hour:02}:{minute:02}:{second:02}"
        );
        if result.is_some() {
            valid.set(valid.get() + 1);
        } else {
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な時刻が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な時刻が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// timestamp は有効な日時に対して Some を返し、無効な日時に対して None を返す。
#[test]
fn prop_timestamp_validity() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let year = noprop::sample_usize_in(ctx, 1..=9999) as i32;
        let month = noprop::sample_usize_in(ctx, 1..=13) as u32;
        let day = noprop::sample_usize_in(ctx, 1..=32) as u32;
        let hour = noprop::sample_usize_in(ctx, 0..=25) as u32;
        let minute = noprop::sample_usize_in(ctx, 0..=61) as u32;
        let second = noprop::sample_usize_in(ctx, 0..=61) as u32;
        let result = timestamp(year, month, day, hour, minute, second);
        let expected = NaiveDate::from_ymd_opt(year, month, day)
            .and_then(|d| d.and_hms_opt(hour, minute, second));
        assert_eq!(
            result, expected,
            "日時の有効性が一致しない: {year}-{month:02}-{day:02} {hour:02}:{minute:02}:{second:02}"
        );
        if result.is_some() {
            valid.set(valid.get() + 1);
        } else {
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

/// 閏年と月末の境界を検証する。
#[test]
fn prop_date_edge_cases() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let year = noprop::sample_usize_in(ctx, 1990..=2100) as i32;
        let month = noprop::sample_usize_in(ctx, 1..=12) as u32;
        // 28〜31 日に絞って月末境界を重点的に生成する。
        let day = noprop::sample_usize_in(ctx, 28..=31) as u32;
        let result = date(year, month, day);
        let expected = NaiveDate::from_ymd_opt(year, month, day);
        assert_eq!(
            result, expected,
            "月末境界の日付が一致しない: {year}-{month:02}-{day:02}"
        );
        if result.is_some() {
            valid.set(valid.get() + 1);
        } else {
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な月末日付が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な月末日付が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// 時刻の境界（24 時、60 秒、60 分）を検証する。
#[test]
fn prop_time_edge_cases() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let hour = noprop::sample_usize_in(ctx, 0..=24) as u32;
        let minute = noprop::sample_usize_in(ctx, 0..=60) as u32;
        let second = noprop::sample_usize_in(ctx, 0..=60) as u32;
        let result = time(hour, minute, second);
        let expected = NaiveTime::from_hms_opt(hour, minute, second);
        assert_eq!(
            result, expected,
            "境界の時刻が一致しない: {hour:02}:{minute:02}:{second:02}"
        );
        if result.is_some() {
            valid.set(valid.get() + 1);
        } else {
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な境界時刻が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な境界時刻が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// ticks 系関数は互いに整合する日付・時刻・日時を返す。
#[test]
fn prop_ticks_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 有効・無効の両方が実行されたことを記録する。
    let valid = Cell::new(0usize);
    let invalid = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let ticks = sample_ticks(ctx);
        if let Some(dt) = DateTime::from_timestamp(ticks, 0) {
            assert_eq!(
                date_from_ticks(ticks),
                Some(dt.date_naive()),
                "ticks {ticks} の日付が一致しない"
            );
            assert_eq!(
                time_from_ticks(ticks),
                Some(dt.time()),
                "ticks {ticks} の時刻が一致しない"
            );
            assert_eq!(
                timestamp_from_ticks(ticks),
                Some(dt.naive_utc()),
                "ticks {ticks} の日時が一致しない"
            );
            // timestamp_from_ticks の日付部分は date_from_ticks と一致する。
            if let (Some(ts), Some(d)) = (timestamp_from_ticks(ticks), date_from_ticks(ticks)) {
                assert_eq!(ts.date(), d, "ticks {ticks} の日付部分が一致しない");
            }
            valid.set(valid.get() + 1);
        } else {
            assert!(
                date_from_ticks(ticks).is_none(),
                "範囲外の ticks {ticks} の日付は None を返すべき"
            );
            assert!(
                time_from_ticks(ticks).is_none(),
                "範囲外の ticks {ticks} の時刻は None を返すべき"
            );
            assert!(
                timestamp_from_ticks(ticks).is_none(),
                "範囲外の ticks {ticks} の日時は None を返すべき"
            );
            invalid.set(invalid.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        valid.get() > 0,
        "有効な ticks が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        invalid.get() > 0,
        "無効な ticks が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}
