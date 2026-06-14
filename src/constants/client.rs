// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL クライアント・ケイパビリティフラグ。
//! https://dev.mysql.com/doc/internals/en/capability-flags.html

#![allow(dead_code)]

pub const LONG_PASSWORD: u32 = 1;
pub const FOUND_ROWS: u32 = 1 << 1;
pub const LONG_FLAG: u32 = 1 << 2;
pub const CONNECT_WITH_DB: u32 = 1 << 3;
pub const NO_SCHEMA: u32 = 1 << 4;
pub const COMPRESS: u32 = 1 << 5;
pub const ODBC: u32 = 1 << 6;
pub const LOCAL_FILES: u32 = 1 << 7;
pub const IGNORE_SPACE: u32 = 1 << 8;
pub const PROTOCOL_41: u32 = 1 << 9;
pub const INTERACTIVE: u32 = 1 << 10;
pub const SSL: u32 = 1 << 11;
pub const IGNORE_SIGPIPE: u32 = 1 << 12;
pub const TRANSACTIONS: u32 = 1 << 13;
pub const SECURE_CONNECTION: u32 = 1 << 15;
pub const MULTI_STATEMENTS: u32 = 1 << 16;
pub const MULTI_RESULTS: u32 = 1 << 17;
pub const PS_MULTI_RESULTS: u32 = 1 << 18;
pub const PLUGIN_AUTH: u32 = 1 << 19;
pub const CONNECT_ATTRS: u32 = 1 << 20;
pub const PLUGIN_AUTH_LENENC_CLIENT_DATA: u32 = 1 << 21;
pub const HANDLE_EXPIRED_PASSWORDS: u32 = 1 << 22;
pub const SESSION_TRACK: u32 = 1 << 23;
pub const DEPRECATE_EOF: u32 = 1 << 24;

/// デフォルトで送信するケイパビリティ。
pub const CAPABILITIES: u32 = LONG_PASSWORD
    | LONG_FLAG
    | PROTOCOL_41
    | TRANSACTIONS
    | SECURE_CONNECTION
    | MULTI_RESULTS
    | PLUGIN_AUTH
    | PLUGIN_AUTH_LENENC_CLIENT_DATA
    | CONNECT_ATTRS;
