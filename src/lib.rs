// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! Sans I/O な MySQL プロトコル実装。
//!
//! 実際の TCP/TLS 入出力は呼び出し側が担当する。

pub mod auth;
pub mod charset;
pub mod connection;
pub mod constants;
pub mod converters;
pub mod error;
pub mod optionfile;
pub mod protocol;
pub mod time;

pub use connection::{ConnectOptions, Connection, SslMode};
pub use error::{Error, Result};
pub use time::{date, date_from_ticks, time, time_from_ticks, timestamp, timestamp_from_ticks};
