// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql_core::time::{
    date, date_from_ticks, time, time_from_ticks, timestamp, timestamp_from_ticks,
};

fuzz_target!(|data: &[u8]| {
    // 任意の 8 バイトを i64 として解釈し、ticks 系関数がパニックしないことを検証する。
    let ticks = if data.len() >= 8 {
        i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ])
    } else {
        0
    };
    let d = date_from_ticks(ticks);
    let t = time_from_ticks(ticks);
    let ts = timestamp_from_ticks(ticks);

    // chrono::DateTime::from_timestamp と一致することを検証。
    if let Some(dt) = chrono::DateTime::from_timestamp(ticks, 0) {
        let local_dt = dt.with_timezone(&chrono::Local);
        assert_eq!(d, Some(local_dt.date_naive()));
        assert_eq!(t, Some(local_dt.time()));
        assert_eq!(ts, Some(local_dt.naive_local()));
    } else {
        assert!(d.is_none());
        assert!(t.is_none());
        assert!(ts.is_none());
    }

    // fuzz 入力から日時成分を生成して、日時生成関数がパニックしないことを検証。
    let year = 1970 + (if data.is_empty() { 0 } else { data[0] } as i32);
    let month = 1 + (if data.len() >= 2 { data[1] } else { 0 } as u32) % 14;
    let day = 1 + (if data.len() >= 3 { data[2] } else { 0 } as u32) % 32;
    let hour = (if data.len() >= 4 { data[3] } else { 0 } as u32) % 26;
    let minute = (if data.len() >= 5 { data[4] } else { 0 } as u32) % 62;
    let second = (if data.len() >= 6 { data[5] } else { 0 } as u32) % 62;
    let _ = date(year, month, day);
    let _ = time(hour, minute, second);
    let _ = timestamp(year, month, day, hour, minute, second);
});
