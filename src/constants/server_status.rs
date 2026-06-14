// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL サーバーステータスフラグ。

#![allow(dead_code)]

pub const SERVER_STATUS_IN_TRANS: u16 = 1;
pub const SERVER_STATUS_AUTOCOMMIT: u16 = 2;
pub const SERVER_MORE_RESULTS_EXISTS: u16 = 8;
pub const SERVER_QUERY_NO_GOOD_INDEX_USED: u16 = 16;
pub const SERVER_QUERY_NO_INDEX_USED: u16 = 32;
pub const SERVER_STATUS_CURSOR_EXISTS: u16 = 64;
pub const SERVER_STATUS_LAST_ROW_SENT: u16 = 128;
pub const SERVER_STATUS_DB_DROPPED: u16 = 256;
pub const SERVER_STATUS_NO_BACKSLASH_ESCAPES: u16 = 512;
pub const SERVER_STATUS_METADATA_CHANGED: u16 = 1024;
