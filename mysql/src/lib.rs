// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! tokio 上で動作する MySQL クライアント。
//!
//! `mysql` の sans I/O なプロトコル実装に対し、
//! TCP/TLS 接続および入出力を担当する。

pub mod connection;
pub mod cursor;
pub mod pool;
pub mod transaction;

pub use connection::Connection;
pub use cursor::{Cursor, DictCursor, UnbufferedCursor, UnbufferedDictCursor};
pub use pool::{Pool, PoolConfig, PooledConnection};
pub use shiguredo_mysql_core::connection::{ConnectOptions, SslMode};
pub use shiguredo_mysql_core::{
    auth, charset, constants, converters, error, optionfile, protocol, time,
};
