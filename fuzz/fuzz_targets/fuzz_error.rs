// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql_core::error::raise_mysql_exception;

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列に対して raise_mysql_exception がパニックしないことを検証する。
    let result = raise_mysql_exception(data);

    if data.len() < 3 {
        // 3 バイト未満は Malformed エラー。
        assert!(
            matches!(result, shiguredo_mysql_core::error::Error::InternalError { .. }),
            "short input must produce InternalError"
        );
    } else {
        // 3 バイト以上は errno に応じた既知のエラー種別のいずれかとなる。
        let errno = u16::from_le_bytes([data[1], data[2]]);
        let code = match result {
            shiguredo_mysql_core::error::Error::ProgrammingError { code, .. } => code,
            shiguredo_mysql_core::error::Error::DataError { code, .. } => code,
            shiguredo_mysql_core::error::Error::IntegrityError { code, .. } => code,
            shiguredo_mysql_core::error::Error::NotSupportedError { code, .. } => code,
            shiguredo_mysql_core::error::Error::OperationalError { code, .. } => code,
            shiguredo_mysql_core::error::Error::InternalError { code, .. } => code,
            other => panic!("unexpected error variant for valid-length input: {other:?}"),
        };
        assert_eq!(code, errno, "error code must match errno parsed from input");
    }
});
