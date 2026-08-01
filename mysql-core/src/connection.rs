// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 接続を管理するモジュール。

pub mod auth;
pub mod packet;
pub mod result;

use crate::auth::AuthPlugin;
use crate::charset::charset_by_name;
use crate::connection::packet::PacketStream;
pub use crate::connection::result::{FeedResult, MySQLResult};
use crate::constants::client;
use crate::constants::client_error;
use crate::constants::command;
use crate::constants::server_status;
use crate::error::{Error, Result};
use crate::protocol::{MysqlPacket, OkPacketWrapper};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// デフォルト文字セット。
pub const DEFAULT_CHARSET: &str = "utf8mb4";

/// 接続オプション。
#[derive(Debug, Clone)]
pub struct ConnectOptions {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: Vec<u8>,
    pub database: Option<String>,
    pub charset: String,
    pub collation: Option<String>,
    pub connect_timeout: Duration,
    pub ssl_mode: SslMode,
    pub ssl_ca: Option<String>,
    pub ssl_cert: Option<String>,
    pub ssl_key: Option<String>,
    pub ssl_verify_identity: bool,
    pub autocommit: Option<bool>,
    pub sql_mode: Option<String>,
    pub init_command: Option<String>,
    pub local_infile: bool,
    pub compress: bool,
    pub max_allowed_packet: usize,
    pub program_name: Option<String>,
    pub server_public_key: Option<Vec<u8>>,
    pub use_unicode: bool,
    /// 接続時に読むオプションファイル (my.cnf)。
    ///
    /// デフォルト値のままのフィールドだけがファイルの値で補完される。
    pub read_default_file: Option<PathBuf>,
    /// オプションファイルから読むグループ。`None` の場合は `client`。
    pub read_default_group: Option<String>,
}

impl Default for ConnectOptions {
    fn default() -> Self {
        Self {
            host: "localhost".to_string(),
            port: 3306,
            user: String::new(),
            password: Vec::new(),
            database: None,
            charset: DEFAULT_CHARSET.to_string(),
            collation: None,
            connect_timeout: Duration::from_secs(10),
            ssl_mode: SslMode::Preferred,
            ssl_ca: None,
            ssl_cert: None,
            ssl_key: None,
            ssl_verify_identity: false,
            autocommit: Some(false),
            sql_mode: None,
            init_command: None,
            local_infile: false,
            compress: false,
            max_allowed_packet: 16 * 1024 * 1024,
            program_name: None,
            server_public_key: None,
            use_unicode: true,
            read_default_file: None,
            read_default_group: None,
        }
    }
}

/// SSL モード。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SslMode {
    /// SSL を使用しない。
    Disabled,
    /// サーバーが対応していれば SSL、しなければ平文。
    Preferred,
    /// SSL が必須。
    Required,
}

/// 認証状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthState {
    /// サーバーからの応答を待つ。
    NeedRead,
    /// 送信すべきデータが send_queue に追加された。
    Send,
    /// 認証成功。
    Success,
}

/// MySQL 接続（sans I/O）。
///
/// 実際の TCP/TLS 入出力は呼び出し側が担当し、
/// 本構造体はプロトコル状態と送受信キューの管理のみを行う。
pub struct Connection {
    options: ConnectOptions,
    packet_stream: PacketStream,
    protocol_version: u8,
    server_version: String,
    server_thread_id: u32,
    salt: Vec<u8>,
    server_capabilities: u32,
    server_status: u16,
    server_language: u8,
    server_charset: Option<String>,
    auth_plugin_name: AuthPlugin,
    client_flag: u32,
    secure: bool,
    encoding: String,
    use_unicode: bool,
    result: Option<MySQLResult>,
    affected_rows: i64,
    pub(crate) server_public_key: Option<Vec<u8>>,
    closed: bool,
    pub(crate) auth_phase: auth::AuthPhase,
    pub(crate) needs_tls_upgrade: bool,
    /// フィールド型ごとに登録されたデコーダー。組み込みデコーダーより優先される。
    field_converters: HashMap<u8, crate::converters::Converter>,
}

impl Connection {
    /// 新規接続のための内部状態を構築する。
    ///
    /// 実際の TCP/TLS 接続およびハンドシェイクは呼び出し側が行う。
    /// `host` が `/` で始まる場合は Unix ドメインソケットのパスとして扱う。
    pub fn connect(options: ConnectOptions) -> Result<Self> {
        // Unix ドメインソケット接続ではポート番号は不要。
        if !options.host.starts_with('/') && options.port == 0 {
            return Err(Error::ProgrammingError {
                code: client_error::CR_UNKNOWN_ERROR,
                message: "port must be greater than 0".to_string(),
            });
        }
        if options.max_allowed_packet == 0 {
            return Err(Error::ProgrammingError {
                code: client_error::CR_UNKNOWN_ERROR,
                message: "max_allowed_packet must be greater than 0".to_string(),
            });
        }
        let one_year = Duration::from_secs(365 * 24 * 60 * 60);
        if options.connect_timeout.is_zero() || options.connect_timeout >= one_year {
            return Err(Error::ProgrammingError {
                code: client_error::CR_UNKNOWN_ERROR,
                message: "connect_timeout must be greater than 0 and less than one year"
                    .to_string(),
            });
        }

        let charset_info =
            charset_by_name(&options.charset).ok_or_else(|| Error::OperationalError {
                code: client_error::CR_CANT_READ_CHARSET,
                message: format!("Unknown charset: {}", options.charset),
            })?;
        let encoding = charset_info.encoding().to_string();
        let mut conn = Self {
            packet_stream: PacketStream::new(options.max_allowed_packet),
            options: options.clone(),
            protocol_version: 0,
            server_version: String::new(),
            server_thread_id: 0,
            salt: Vec::new(),
            server_capabilities: 0,
            server_status: 0,
            server_language: 0,
            server_charset: None,
            auth_plugin_name: AuthPlugin::MysqlNativePassword,
            client_flag: client::CAPABILITIES,
            secure: false,
            encoding,
            use_unicode: options.use_unicode,
            result: None,
            affected_rows: 0,
            server_public_key: options.server_public_key.clone(),
            closed: false,
            auth_phase: auth::AuthPhase::Initial,
            needs_tls_upgrade: false,
            field_converters: HashMap::new(),
        };

        if options.database.is_some() {
            conn.client_flag |= client::CONNECT_WITH_DB;
        }
        if options.local_infile {
            conn.client_flag |= client::LOCAL_FILES;
        }
        if options.compress {
            conn.client_flag |= client::COMPRESS;
        }

        Ok(conn)
    }

    /// TLS アップグレードが必要かどうかを返す。
    pub fn needs_tls_upgrade(&self) -> bool {
        self.needs_tls_upgrade
    }

    /// 接続を閉じる。
    pub fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.closed = true;
        // COM_QUIT を新しいコマンドとして送信する。
        self.packet_stream.reset_seq_id();
        self.write_packet(&[command::COM_QUIT])?;
        Ok(())
    }

    /// 強制的に接続を閉じる。
    pub fn force_close(&mut self) {
        self.packet_stream.force_close();
        self.closed = true;
    }

    /// TLS 状態を設定する。
    pub fn set_secure(&mut self, secure: bool) {
        self.secure = secure;
    }

    /// 接続オプションを取得する。
    pub fn options(&self) -> &ConnectOptions {
        &self.options
    }

    /// 接続が開いているかどうか。
    pub fn is_open(&self) -> bool {
        !self.closed
    }

    /// サーバーのケイパビリティフラグを取得する。
    pub fn server_capabilities(&self) -> u32 {
        self.server_capabilities
    }

    /// 圧縮が有効になっているかどうか。
    pub fn is_compressed(&self) -> bool {
        self.packet_stream.is_compressed()
    }

    /// 圧縮を有効にする。
    ///
    /// 認証完了後、かつサーバーが圧縮に対応している場合に呼び出す。
    pub fn enable_compression(&mut self) {
        self.packet_stream.enable_compression();
    }

    /// 文字セットを設定する。
    pub fn set_character_set(&mut self, charset: &str, collation: Option<String>) -> Result<()> {
        let charset_info = charset_by_name(charset).ok_or_else(|| Error::OperationalError {
            code: client_error::CR_CANT_READ_CHARSET,
            message: format!("Unknown charset: {}", charset),
        })?;
        self.encoding = charset_info.encoding().to_string();

        if let Some(c) = &collation
            && !c.chars().all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        {
            return Err(Error::ProgrammingError {
                code: client_error::CR_CANT_READ_CHARSET,
                message: format!("Invalid collation: {}", c),
            });
        }

        let query = match &collation {
            Some(c) => format!("SET NAMES {} COLLATE {}", charset, c),
            None => format!("SET NAMES {}", charset),
        };
        self.execute_command(command::COM_QUERY, query.as_bytes())?;
        self.options.charset = charset.to_string();
        self.options.collation = collation;
        Ok(())
    }

    /// オートコミットモードを設定する。
    pub fn set_autocommit(&mut self, value: bool) -> Result<()> {
        self.options.autocommit = Some(value);
        let current = self.get_autocommit();
        if value != current {
            let query = format!("SET AUTOCOMMIT = {}", if value { 1 } else { 0 });
            self.execute_command(command::COM_QUERY, query.as_bytes())?;
        }
        Ok(())
    }

    /// サーバーのオートコミット状態を取得する。
    pub fn get_autocommit(&self) -> bool {
        (self.server_status & server_status::SERVER_STATUS_AUTOCOMMIT) != 0
    }

    /// トランザクション内かどうかを返す。
    pub fn in_transaction(&self) -> bool {
        (self.server_status & server_status::SERVER_STATUS_IN_TRANS) != 0
    }

    /// データベースを切り替える。
    ///
    /// PyMySQL の `Connection.select_db` に相当し、COM_INIT_DB コマンドを送信する。
    /// 応答の OK パケット読み込みは呼び出し側が行う。
    pub fn select_db(&mut self, db: &str) -> Result<()> {
        self.execute_command(command::COM_INIT_DB, db.as_bytes())
    }

    /// サーバーへの疎通を確認する。
    ///
    /// PyMySQL の `Connection.ping` に相当し、COM_PING コマンドを送信する。
    /// 応答の OK パケット読み込みは呼び出し側が行う。
    pub fn ping(&mut self) -> Result<()> {
        self.execute_command(command::COM_PING, &[])
    }

    /// 指定したスレッド ID の接続を終了させる。
    ///
    /// PyMySQL の `Connection.kill` と同じく KILL クエリを実行する。
    pub fn kill(&mut self, thread_id: u32) -> Result<i64> {
        self.query(&format!("KILL {}", thread_id), false)
    }

    /// トランザクションを開始する。
    ///
    /// PyMySQL の `Connection.begin` と同じく BEGIN クエリを実行する。
    /// 既にトランザクション内の場合はサーバー側で何も行われない。
    /// 応答の OK パケット読み込みは呼び出し側が行う。
    pub fn begin(&mut self) -> Result<()> {
        self.execute_command(command::COM_QUERY, b"BEGIN")
    }

    /// トランザクションをコミットする。
    ///
    /// PyMySQL の `Connection.commit` と同じく COMMIT クエリを実行する。
    /// 応答の OK パケット読み込みは呼び出し側が行う。
    pub fn commit(&mut self) -> Result<()> {
        self.execute_command(command::COM_QUERY, b"COMMIT")
    }

    /// トランザクションをロールバックする。
    ///
    /// PyMySQL の `Connection.rollback` と同じく ROLLBACK クエリを実行する。
    /// 応答の OK パケット読み込みは呼び出し側が行う。
    pub fn rollback(&mut self) -> Result<()> {
        self.execute_command(command::COM_QUERY, b"ROLLBACK")
    }

    /// クエリを実行する。
    pub fn query(&mut self, sql: &str, unbuffered: bool) -> Result<i64> {
        tracing::debug!(sql = %sql, unbuffered, "Executing query");
        let sql_bytes = sql.as_bytes().to_vec();
        self.execute_command(command::COM_QUERY, &sql_bytes)?;
        self.affected_rows = self.read_query_result(unbuffered)?;
        tracing::debug!(affected_rows = self.affected_rows, "Query executed");
        Ok(self.affected_rows)
    }

    /// 次の結果セットを読み込む。
    pub fn next_result(&mut self, unbuffered: bool) -> Result<i64> {
        self.affected_rows = self.read_query_result(unbuffered)?;
        Ok(self.affected_rows)
    }

    /// 結果セットを読み込む。
    ///
    /// 受信キューに十分なパケットがない場合は `Error::NeedMoreData` を返す。
    /// 呼び出し側はさらにデータを供給してから再度呼び出すことで読み込みを再開できる。
    pub fn read_query_result(&mut self, unbuffered: bool) -> Result<i64> {
        let mut result = self
            .result
            .take()
            .filter(|r| !r.is_done())
            .unwrap_or_default();
        if result.is_initial() {
            result.unbuffered_active = unbuffered;
        }
        loop {
            let packet = self.read_packet()?;
            match result.feed_packet(packet, self)? {
                crate::connection::result::FeedResult::NeedMore => continue,
                crate::connection::result::FeedResult::Done
                | crate::connection::result::FeedResult::UnbufferedReady => {
                    let affected_rows = result.affected_rows;
                    self.server_status = result.server_status.unwrap_or(self.server_status);
                    self.result = Some(result);
                    return Ok(affected_rows);
                }
            }
        }
    }

    /// 結果セットを設定する。
    pub fn set_result(&mut self, result: MySQLResult) {
        self.server_status = result.server_status.unwrap_or(self.server_status);
        self.affected_rows = result.affected_rows;
        self.result = Some(result);
    }

    /// 影響を受けた行数を取得する。
    pub fn affected_rows(&self) -> i64 {
        self.affected_rows
    }

    /// 最後に挿入された ID を取得する。
    pub fn insert_id(&self) -> u64 {
        self.result
            .as_ref()
            .map(|r| r.insert_id.unwrap_or(0))
            .unwrap_or(0)
    }

    /// サーバー情報を取得する。
    pub fn get_server_information(&mut self) -> Result<()> {
        let packet = self.read_packet()?;
        let data = packet.get_all_data().to_vec();
        let mut i = 0_usize;

        if i >= data.len() {
            return Err(Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: empty".to_string(),
            });
        }
        self.protocol_version = data[i];
        i += 1;

        if i >= data.len() {
            return Err(Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: missing server version".to_string(),
            });
        }
        let server_end = data[i..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| i + p)
            .ok_or_else(|| Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: server version is not null-terminated"
                    .to_string(),
            })?;
        self.server_version = String::from_utf8_lossy(&data[i..server_end]).to_string();
        i = server_end + 1;

        if i + 4 > data.len() {
            return Err(Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: missing thread id".to_string(),
            });
        }
        self.server_thread_id =
            u32::from_le_bytes([data[i], data[i + 1], data[i + 2], data[i + 3]]);
        i += 4;

        if i + 9 > data.len() {
            return Err(Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: missing salt part 1".to_string(),
            });
        }
        self.salt = data[i..i + 8].to_vec();
        i += 9; // 8 + 1 filler

        if i + 2 > data.len() {
            return Err(Error::OperationalError {
                code: client_error::CR_SERVER_HANDSHAKE_ERR,
                message: "Malformed handshake packet: missing capability flags".to_string(),
            });
        }
        self.server_capabilities = u16::from_le_bytes([data[i], data[i + 1]]) as u32;
        i += 2;

        if i + 6 <= data.len() {
            let lang = data[i];
            let stat = u16::from_le_bytes([data[i + 1], data[i + 2]]);
            let cap_h = u16::from_le_bytes([data[i + 3], data[i + 4]]);
            let salt_len = data[i + 5] as usize;
            i += 6;

            self.server_language = lang;
            self.server_charset =
                crate::charset::charset_by_id(lang.into()).map(|c| c.name.clone());
            self.server_status = stat;
            self.server_capabilities |= (cap_h as u32) << 16;
            let salt_len = salt_len.saturating_sub(9).max(12);

            if i + 10 > data.len() {
                return Err(Error::OperationalError {
                    code: client_error::CR_SERVER_HANDSHAKE_ERR,
                    message: "Malformed handshake packet: missing reserved bytes".to_string(),
                });
            }
            i += 10; // reserved

            if i + salt_len <= data.len() {
                self.salt.extend_from_slice(&data[i..i + salt_len]);
                i += salt_len;
            }
        } else {
            i += 10; // reserved
        }

        if i >= data.len() {
            return Ok(());
        }
        i += 1;

        if (self.server_capabilities & client::PLUGIN_AUTH) != 0 && i < data.len() {
            let end = data[i..].iter().position(|&b| b == 0).map(|p| i + p);
            let name = match end {
                Some(end) => &data[i..end],
                None => &data[i..],
            };
            self.auth_plugin_name = AuthPlugin::from_bytes(name);
        }

        Ok(())
    }

    /// パケットを送信キューに追加する。
    pub fn write_packet(&mut self, payload: &[u8]) -> Result<()> {
        self.packet_stream.write_packet(payload)
    }

    /// 受信した生バイト列を消費してパケットを組み立て、recv_queue に追加する。
    pub fn feed_bytes(&mut self, data: &[u8]) -> Result<usize> {
        self.packet_stream.feed_bytes(data)
    }

    /// 受信済みパケットキューから一つ取り出す。
    pub fn read_packet(&mut self) -> Result<MysqlPacket> {
        self.packet_stream.read_packet()
    }

    /// コマンドを実行する。
    pub fn execute_command(&mut self, command: u8, sql: &[u8]) -> Result<()> {
        if self.closed {
            return Err(Error::InterfaceError {
                code: 0,
                message: "Connection is closed".to_string(),
            });
        }

        if let Some(mut result) = self.result.take() {
            if result.unbuffered_active {
                result.finish_unbuffered_query(self)?;
            }
            while result.has_next {
                self.next_result(false)?;
                if let Some(r) = self.result.take() {
                    result = r;
                } else {
                    break;
                }
            }
        }

        // コマンドパケットは新しいシーケンスとして seq=0 から始める。
        // MySQL 8.x では seq=1 から始めるとサーバーが接続を切断する場合がある。
        self.packet_stream.reset_seq_id();
        let mut payload = Vec::with_capacity(sql.len().saturating_add(1));
        payload.push(command);
        payload.extend_from_slice(sql);
        tracing::debug!(
            command,
            payload_size = payload.len(),
            "Sending command packet"
        );
        self.write_packet(&payload)?;
        Ok(())
    }

    /// OK パケットを読み込む。
    pub fn read_ok_packet(&mut self) -> Result<OkPacketWrapper> {
        let mut pkt = self.read_packet()?;
        if !pkt.is_ok_packet() {
            return Err(Error::OperationalError {
                code: client_error::CR_COMMANDS_OUT_OF_SYNC,
                message: "Command Out of Sync".to_string(),
            });
        }
        let ok = OkPacketWrapper::from_packet(&mut pkt)?;
        self.server_status = ok.server_status;
        Ok(ok)
    }

    /// 文字列をエスケープする。
    pub fn escape_string(&self, s: &str) -> String {
        if (self.server_status & server_status::SERVER_STATUS_NO_BACKSLASH_ESCAPES) != 0 {
            s.replace('\'', "''")
        } else {
            crate::converters::escape_string(s)
        }
    }

    /// 値を SQL リテラルに変換する。
    pub fn literal(&self, obj: &crate::converters::Value) -> Result<String> {
        match obj {
            crate::converters::Value::String(s) => Ok(format!("'{}'", self.escape_string(s))),
            crate::converters::Value::Bytes(b) => {
                Ok(crate::converters::escape_bytes(b, &self.encoding))
            }
            _ => crate::converters::escape_item(obj, &self.encoding),
        }
    }

    /// スレッド ID を取得する。
    pub fn thread_id(&self) -> u32 {
        self.server_thread_id
    }

    /// 文字セット名を取得する。
    pub fn character_set_name(&self) -> &str {
        &self.options.charset
    }

    /// サーバーバージョンを取得する。
    pub fn server_version(&self) -> &str {
        &self.server_version
    }

    /// プロトコルバージョンを取得する。
    ///
    /// PyMySQL の `Connection.get_proto_info` に相当する。
    pub fn get_proto_info(&self) -> u8 {
        self.protocol_version
    }

    /// 接続先情報を取得する。
    ///
    /// PyMySQL の `Connection.get_host_info` に相当する。
    /// Unix ドメインソケット接続の場合はソケットのパスを返す。
    pub fn get_host_info(&self) -> String {
        if self.options.host.starts_with('/') {
            self.options.host.clone()
        } else {
            format!("{}:{}", self.options.host, self.options.port)
        }
    }

    /// フィールド型ごとのデコーダーを登録する。
    ///
    /// 登録したデコーダーは組み込みのデコーダーより優先される。
    /// PyMySQL の `conv` によるデコーダー差し替えに相当する。
    pub fn register_converter(&mut self, type_code: u8, converter: crate::converters::Converter) {
        self.field_converters.insert(type_code, converter);
    }

    pub(crate) fn field_converter(&self, type_code: u8) -> Option<crate::converters::Converter> {
        self.field_converters.get(&type_code).copied()
    }

    /// 現在の結果セットを取得する。
    pub fn result(&self) -> Option<&MySQLResult> {
        self.result.as_ref()
    }

    /// 現在の結果セットを可変参照で取得する。
    pub fn result_mut(&mut self) -> Option<&mut MySQLResult> {
        self.result.as_mut()
    }

    pub(crate) fn auth_plugin_name(&self) -> &AuthPlugin {
        &self.auth_plugin_name
    }

    pub(crate) fn auth_plugin_name_mut(&mut self) -> &mut AuthPlugin {
        &mut self.auth_plugin_name
    }

    pub(crate) fn salt(&self) -> &Vec<u8> {
        &self.salt
    }

    pub(crate) fn salt_mut(&mut self) -> &mut Vec<u8> {
        &mut self.salt
    }

    pub(crate) fn is_secure(&self) -> bool {
        self.secure
    }

    pub(crate) fn encoding(&self) -> &str {
        &self.encoding
    }

    pub(crate) fn use_unicode(&self) -> bool {
        self.use_unicode
    }

    /// 送信キューから先頭のパケットを取り出す。
    pub fn pop_send_queue(&mut self) -> Option<Vec<u8>> {
        self.packet_stream.send_queue.pop_front()
    }

    /// 受信キューが空かどうかを返す。
    pub fn is_recv_queue_empty(&self) -> bool {
        self.packet_stream.recv_queue.is_empty()
    }
}

/// 長さ符号付き整数をエンコードする。
pub fn lenenc_int(i: usize) -> Vec<u8> {
    if i < 0xFB {
        vec![i as u8]
    } else if i < (1 << 16) {
        let mut v = vec![0xFC];
        v.extend_from_slice(&(i as u16).to_le_bytes());
        v
    } else if i < (1 << 24) {
        let mut v = vec![0xFD];
        v.extend_from_slice(&(i as u32).to_le_bytes()[..3]);
        v
    } else {
        let mut v = vec![0xFE];
        v.extend_from_slice(&(i as u64).to_le_bytes());
        v
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::connection::packet::tests::build_packet;

    #[test]
    fn test_get_server_information_missing_null_terminator() {
        let mut conn = Connection::connect(ConnectOptions::default()).unwrap();
        let payload = vec![10, b'5', b'.', b'7', b'.', b'0'];
        let packet = build_packet(0, &payload);
        conn.feed_bytes(&packet).unwrap();
        let result = conn.get_server_information();
        assert!(result.is_err());
    }

    #[test]
    fn test_get_server_information_empty_packet() {
        let mut conn = Connection::connect(ConnectOptions::default()).unwrap();
        let packet = build_packet(0, &[]);
        conn.feed_bytes(&packet).unwrap();
        let result = conn.get_server_information();
        assert!(result.is_err());
    }

    #[test]
    fn test_feed_bytes_max_allowed_packet() {
        let options = ConnectOptions {
            max_allowed_packet: 1024,
            ..Default::default()
        };
        let mut conn = Connection::connect(options).unwrap();
        let payload = vec![0u8; 1025];
        let packet = build_packet(0, &payload);
        let result = conn.feed_bytes(&packet);
        assert!(result.is_err());
    }

    #[test]
    fn test_connect_with_unix_socket() {
        // host が / で始まる場合は Unix ドメインソケットのパスとして扱い、
        // ポート 0 でもエラーにならない。
        let options = ConnectOptions {
            host: "/var/run/mysqld/mysqld.sock".to_string(),
            port: 0,
            ..Default::default()
        };
        assert!(Connection::connect(options).is_ok());
    }

    #[test]
    fn test_connect_rejects_zero_port_without_unix_socket() {
        let options = ConnectOptions {
            port: 0,
            ..Default::default()
        };
        let result = Connection::connect(options);
        assert!(result.is_err(), "TCP 接続でポート 0 はエラーにするべき");
    }

    #[test]
    fn test_in_transaction() {
        // 初期状態ではトランザクション内ではない。
        let conn = Connection::connect(ConnectOptions::default()).unwrap();
        assert!(!conn.in_transaction());
    }
}
