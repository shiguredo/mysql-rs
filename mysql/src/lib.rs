// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! tokio 上で動作する MySQL クライアント。
//!
//! sans I/O なプロトコル実装 (`shiguredo_mysql_core`) は内部実装であり、
//! 利用者は本クレートだけに依存すればよい。プロトコル実装の公開 API は
//! 以下のモジュールとして再エクスポートされる。
//!
//! - [`auth`] - 認証処理 (スクランブル計算等)
//! - [`charset`] - 文字セットと変換
//! - [`constants`] - プロトコル定数 (コマンド・エラーコード等)
//! - [`converters`] - 値の変換 (`Value` 等)
//! - [`error`] - エラー型
//! - [`optionfile`] - オプションファイル (my.cnf) の解析
//! - [`protocol`] - ワイヤプロトコルのメッセージとレスポンス型
//! - [`time`] - 日付・時刻型の変換
//!
//! TCP/TLS 接続および入出力は本クレートが担当する。

pub use shiguredo_mysql_core::auth;
pub use shiguredo_mysql_core::charset;
pub use shiguredo_mysql_core::constants;
pub use shiguredo_mysql_core::converters;
pub use shiguredo_mysql_core::error;
pub use shiguredo_mysql_core::optionfile;
pub use shiguredo_mysql_core::protocol;
pub use shiguredo_mysql_core::time;

pub mod connection;
pub mod cursor;
pub mod pool;
pub mod transaction;
