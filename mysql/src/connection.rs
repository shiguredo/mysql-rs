// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 接続の tokio I/O 実装。
//!
//! sans I/O な状態マシン (内部実装) に対し、
//! TCP/TLS 接続、タイムアウト、読み書きを行う。

use crate::constants::client;
use crate::constants::client_error;
use crate::constants::command;
use crate::converters::Value;
use crate::error::{Error, Result};
use crate::optionfile::OptionFile;
use crate::protocol::{LoadLocalPacketWrapper, MysqlPacket, OkPacketWrapper};
use rustls::client::WebPkiServerVerifier;
use rustls::client::danger::{HandshakeSignatureValid, ServerCertVerified, ServerCertVerifier};
use rustls::pki_types::{CertificateDer, PrivatePkcs8KeyDer, ServerName, UnixTime, pem::PemObject};
use rustls::{DigitallySignedStruct, Error as RustlsError};
use rustls_platform_verifier::{BuilderVerifierExt, Verifier};
use shiguredo_mysql_core::connection::{AuthState, Connection as InnerConnection, FeedResult};
use std::io;
use std::str::FromStr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpStream, UnixStream};
use tokio::time::timeout;
use tokio_rustls::TlsConnector;

// 以下は sans I/O 実装 (shiguredo_mysql_core) から再エクスポートした型。
// 利用者は shiguredo_mysql クレートだけに依存すればよい。
// ドキュメントは sans I/O 実装側のものが引き継がれる。
pub use shiguredo_mysql_core::connection::{ConnectOptions, MySQLResult, SslMode};

/// MySQL 接続。
pub struct Connection {
    inner: InnerConnection,
    stream: Option<ConnectionStream>,
    /// commit / rollback されずに破棄されたトランザクションがあるかどうか。
    transaction_dirty: bool,
}

enum ConnectionStream {
    Plain(TcpStream),
    Tls(Box<tokio_rustls::client::TlsStream<TcpStream>>),
    Unix(UnixStream),
}

impl ConnectionStream {
    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            Self::Plain(s) => s.read(buf).await,
            Self::Tls(s) => s.read(buf).await,
            Self::Unix(s) => s.read(buf).await,
        }
    }

    async fn write_all(&mut self, data: &[u8]) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.write_all(data).await,
            Self::Tls(s) => s.write_all(data).await,
            Self::Unix(s) => s.write_all(data).await,
        }
    }

    async fn flush(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.flush().await,
            Self::Tls(s) => s.flush().await,
            Self::Unix(s) => s.flush().await,
        }
    }

    async fn shutdown(&mut self) -> std::io::Result<()> {
        match self {
            Self::Plain(s) => s.shutdown().await,
            Self::Tls(s) => s.shutdown().await,
            Self::Unix(s) => s.shutdown().await,
        }
    }
}

impl Connection {
    /// 新規接続を確立する。
    pub async fn connect(options: ConnectOptions) -> Result<Self> {
        // オプションファイルを適用する。
        let options = apply_option_file(options)?;
        let inner = InnerConnection::connect(options.clone())?;
        let stream = if options.host.starts_with('/') {
            // Unix ドメインソケット。postgres-rs と同じく
            // ソケットファイルのパスを host に直接指定する。
            let path = options.host.trim_end_matches('/');
            let stream = timeout(options.connect_timeout, UnixStream::connect(path))
                .await
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_CONN_HOST_ERROR,
                    message: format!("Connection timeout: {}", e),
                })?
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_CONN_HOST_ERROR,
                    message: format!("Can't connect to MySQL server on {:?} ({})", path, e),
                })?;
            ConnectionStream::Unix(stream)
        } else {
            let addr = match std::net::IpAddr::from_str(&options.host) {
                Ok(ip) if ip.is_ipv6() => format!("[{}]:{}", options.host, options.port),
                _ => format!("{}:{}", options.host, options.port),
            };
            let stream = timeout(options.connect_timeout, TcpStream::connect(&addr))
                .await
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_CONN_HOST_ERROR,
                    message: format!("Connection timeout: {}", e),
                })?
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_CONN_HOST_ERROR,
                    message: format!("Can't connect to MySQL server on {:?} ({})", addr, e),
                })?;
            stream
                .set_nodelay(true)
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_CONN_HOST_ERROR,
                    message: format!("Failed to set TCP_NODELAY: {}", e),
                })?;
            ConnectionStream::Plain(stream)
        };

        let mut conn = Self {
            inner,
            stream: Some(stream),
            transaction_dirty: false,
        };

        // greeting
        conn.pump_read().await?;
        conn.inner.get_server_information()?;

        // authentication
        conn.authenticate().await?;

        // サーバーが圧縮に対応していれば有効化する。
        if options.compress && conn.inner.server_capabilities() & client::COMPRESS != 0 {
            conn.inner.enable_compression();
        }

        // post-connect setup
        conn.post_connect_setup(&options).await?;

        tracing::info!(
            host = %options.host,
            port = options.port,
            server_version = %conn.inner.server_version(),
            thread_id = conn.inner.thread_id(),
            "Connected to MySQL server"
        );
        Ok(conn)
    }

    async fn post_connect_setup(&mut self, options: &ConnectOptions) -> Result<()> {
        self.inner
            .set_character_set(&options.charset, options.collation.clone())?;
        self.pump_write().await?;
        self.read_ok_packet().await?;

        if let Some(sql_mode) = &options.sql_mode {
            let query = format!("SET sql_mode='{}'", self.inner.escape_string(sql_mode));
            self.query(&query, false).await?;
        }

        if let Some(init_command) = &options.init_command {
            self.query(init_command, false).await?;
        }

        if let Some(autocommit) = options.autocommit {
            let query = format!("SET AUTOCOMMIT = {}", if autocommit { 1 } else { 0 });
            self.query(&query, false).await?;
        }

        Ok(())
    }

    async fn read_ok_packet(&mut self) -> Result<OkPacketWrapper> {
        let mut packet = self.read_packet().await?;
        if !packet.is_ok_packet() {
            return Err(Error::OperationalError {
                code: client_error::CR_COMMANDS_OUT_OF_SYNC,
                message: "Command Out of Sync".to_string(),
            });
        }
        OkPacketWrapper::from_packet(&mut packet)
    }

    async fn authenticate(&mut self) -> Result<()> {
        // connect() で既に initial handshake を受信・解析済みなので、
        // ここでは認証パケットの生成から開始する。
        let mut state = self.inner.request_authentication_start()?;
        if self.inner.needs_tls_upgrade() {
            // SSL リクエストパケットのみ送信してから TLS ハンドシェイクを行う。
            self.pump_write().await?;
            self.upgrade_to_tls().await?;
            state = self.inner.request_authentication_send_credentials()?;
            self.pump_write().await?;
        } else {
            self.pump_write().await?;
        }

        loop {
            match state {
                AuthState::NeedRead => {
                    self.pump_read().await?;
                    state = self.inner.request_authentication_continue()?;
                }
                AuthState::Send => {
                    self.pump_write().await?;
                    state = AuthState::NeedRead;
                }
                AuthState::Success => break,
            }
        }
        Ok(())
    }

    async fn upgrade_to_tls(&mut self) -> Result<()> {
        let config = build_tls_config(self.inner.options()).await?;
        let connector = TlsConnector::from(Arc::new(config));
        let server_name = server_name_from_host(&self.inner.options().host)?;

        let stream = self.stream.take().ok_or_else(|| Error::InterfaceError {
            code: client_error::CR_NULL_POINTER,
            message: "No stream to upgrade".to_string(),
        })?;
        let plain = match stream {
            ConnectionStream::Plain(s) => s,
            ConnectionStream::Tls(_) => {
                return Err(Error::InterfaceError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: "Already TLS".to_string(),
                });
            }
            // Unix ドメインソケットはローカル接続のため TLS を使わない。
            ConnectionStream::Unix(_) => {
                return Err(Error::InterfaceError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: "TLS upgrade is not supported on Unix domain sockets".to_string(),
                });
            }
        };

        let tls_stream =
            connector
                .connect(server_name, plain)
                .await
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: format!("TLS handshake failed: {}", e),
                })?;
        self.stream = Some(ConnectionStream::Tls(Box::new(tls_stream)));
        self.inner.set_secure(true);
        Ok(())
    }

    /// 送信キューの内容をすべて書き込む。
    async fn pump_write(&mut self) -> Result<()> {
        while let Some(data) = self.inner.pop_send_queue() {
            self.write_all(&data).await?;
        }
        self.flush().await?;
        Ok(())
    }

    /// サーバーからデータを読み込み、内部の受信バッファに供給する。
    ///
    /// 受信バッファに完全なパケットが蓄積されるまで繰り返し読み込む。
    async fn pump_read(&mut self) -> Result<()> {
        loop {
            let mut buf = vec![0u8; 4096];
            let n = match self.read(&mut buf).await {
                Ok(0) => {
                    self.inner.force_close();
                    self.stream.take();
                    return Err(Error::OperationalError {
                        code: client_error::CR_SERVER_LOST,
                        message: "Lost connection to MySQL server during query".to_string(),
                    });
                }
                Ok(n) => n,
                Err(e) => {
                    self.inner.force_close();
                    self.stream.take();
                    return Err(Error::OperationalError {
                        code: client_error::CR_SERVER_LOST,
                        message: format!("Lost connection to MySQL server during query ({})", e),
                    });
                }
            };
            let added = self.inner.feed_bytes(&buf[..n])?;
            if added > 0 {
                break;
            }
        }
        Ok(())
    }

    async fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self.stream.as_mut() {
            Some(stream) => stream.read(buf).await,
            None => Err(io::Error::new(
                io::ErrorKind::NotConnected,
                "No stream available",
            )),
        }
    }

    async fn write_all(&mut self, data: &[u8]) -> Result<()> {
        match self.stream.as_mut() {
            Some(stream) => stream.write_all(data).await,
            None => {
                return Err(Error::InterfaceError {
                    code: client_error::CR_NULL_POINTER,
                    message: "No stream available".to_string(),
                });
            }
        }
        .map_err(|e| {
            self.inner.force_close();
            Error::OperationalError {
                code: client_error::CR_SERVER_GONE_ERROR,
                message: format!("MySQL server has gone away ({})", e),
            }
        })
    }

    async fn flush(&mut self) -> Result<()> {
        match self.stream.as_mut() {
            Some(stream) => stream.flush().await,
            None => {
                return Err(Error::InterfaceError {
                    code: client_error::CR_NULL_POINTER,
                    message: "No stream available".to_string(),
                });
            }
        }
        .map_err(|e| {
            self.inner.force_close();
            Error::OperationalError {
                code: client_error::CR_SERVER_GONE_ERROR,
                message: format!("MySQL server has gone away ({})", e),
            }
        })
    }

    /// 受信キューにパケットが来るまで非同期読み込みを続ける。
    async fn read_packet(&mut self) -> Result<MysqlPacket> {
        loop {
            match self.inner.read_packet() {
                Ok(packet) => return Ok(packet),
                Err(Error::NeedMoreData) => self.pump_read().await?,
                Err(e) => return Err(e),
            }
        }
    }

    /// 結果セットを読み込む。
    async fn read_query_result(&mut self, unbuffered: bool) -> Result<i64> {
        let mut result = MySQLResult::new();
        result.unbuffered_active = unbuffered;
        // LOAD LOCAL パケットは結果セットの最初のパケットとしてのみ現れる。
        // 行データの NULL 値 (0xFB) と区別するため、最初のパケットだけ判定する。
        let mut first_packet = true;
        loop {
            let packet = self.read_packet().await?;
            if first_packet && packet.is_load_local_packet() {
                // LOAD LOCAL パケットはファイルの内容を送信してから結果の読み込みを続ける。
                self.send_load_local_file(&packet).await?;
                first_packet = false;
                continue;
            }
            first_packet = false;
            match result.feed_packet(packet, &self.inner)? {
                FeedResult::NeedMore => continue,
                FeedResult::Done | FeedResult::UnbufferedReady => {
                    let affected = result.affected_rows;
                    self.inner.set_result(result);
                    return Ok(affected);
                }
            }
        }
    }

    /// LOAD DATA LOCAL INFILE のファイル内容を送信する。
    ///
    /// PyMySQL の `LoadLocalFile` に相当する。
    /// ファイルはチャンクに分けて送信し、空パケットで終端する。
    async fn send_load_local_file(&mut self, packet: &MysqlPacket) -> Result<()> {
        if !self.inner.options().local_infile {
            return Err(Error::OperationalError {
                code: client_error::CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
                message: "Received LOAD_LOCAL packet but local_infile option is false".to_string(),
            });
        }
        let load_packet = LoadLocalPacketWrapper::from_packet(packet)?;
        let filename = String::from_utf8_lossy(&load_packet.filename).to_string();
        let filename = expand_user(&filename);
        let data = tokio::fs::read(&filename)
            .await
            .map_err(|e| Error::OperationalError {
                code: client_error::CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
                message: format!(
                    "Failed to read LOAD DATA LOCAL INFILE file {}: {}",
                    filename, e
                ),
            })?;
        // チャンクごとにパケットを送信する。パケット分割はコア側が行う。
        const CHUNK_SIZE: usize = 8192;
        for chunk in data.chunks(CHUNK_SIZE) {
            self.inner.write_packet(chunk)?;
        }
        // 空パケットでファイルの終端を示す。
        self.inner.write_packet(&[])?;
        self.pump_write().await?;
        tracing::debug!(filename = %filename, bytes = data.len(), "Sent LOAD DATA LOCAL INFILE file");
        Ok(())
    }

    /// アンバッファードクエリの残りの行をすべて消費する。
    pub(crate) async fn finish_unbuffered_query(&mut self) -> Result<()> {
        loop {
            let active = self
                .inner
                .result()
                .map(|r| r.unbuffered_active)
                .unwrap_or(false);
            if !active {
                return Ok(());
            }
            let packet = self.read_packet().await?;
            let result = self
                .inner
                .result_mut()
                .expect("unbuffered result is set while unbuffered_active is true");
            result.read_rowdata_packet_unbuffered(packet)?;
        }
    }

    /// アンバッファードクエリの次の行を読み込む。
    ///
    /// 結果セットの末尾に達した場合は `None` を返す。
    pub async fn next_unbuffered_row(&mut self) -> Result<Option<Vec<Value>>> {
        loop {
            let active = self
                .inner
                .result()
                .map(|r| r.unbuffered_active)
                .unwrap_or(false);
            if !active {
                return Ok(None);
            }
            let packet = self.read_packet().await?;
            let result = self
                .inner
                .result_mut()
                .expect("unbuffered result is set while unbuffered_active is true");
            let row = result.read_rowdata_packet_unbuffered(packet)?;
            if row.is_some() {
                return Ok(row);
            }
        }
    }

    /// クエリを実行する。
    pub async fn query(&mut self, sql: &str, unbuffered: bool) -> Result<i64> {
        tracing::debug!(sql = %sql, unbuffered, "Executing query");
        // アンバッファードクエリの残りがあれば先に消費する。
        self.finish_unbuffered_query().await?;
        self.inner
            .execute_command(command::COM_QUERY, sql.as_bytes())?;
        self.pump_write().await?;
        let affected = self.read_query_result(unbuffered).await?;
        tracing::debug!(affected_rows = affected, "Query executed");
        Ok(affected)
    }

    /// クエリを実行する（引数付き）。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        let query = self.mogrify(query, args)?;
        self.query(&query, false).await
    }

    /// クエリ文字列に引数を埋め込む。
    ///
    /// `%s` は引数に順番に置換される。`%%s` は `%s` として出力される。
    /// プレースホルダーが引数より少ない場合はエラーとなる。
    pub fn mogrify(&self, query: &str, args: Option<&[Value]>) -> Result<String> {
        mogrify_query(query, args, |v| self.literal(v))
    }

    /// 次の結果セットに移動する。
    pub async fn next_result(&mut self, unbuffered: bool) -> Result<i64> {
        self.finish_unbuffered_query().await?;
        self.read_query_result(unbuffered).await
    }

    /// カーソルを作成する。
    pub fn cursor(&mut self) -> crate::cursor::Cursor<'_> {
        crate::cursor::Cursor::new(self)
    }

    /// アンバッファードカーソルを作成する。
    ///
    /// 行をメモリに蓄えず、fetch のたびにサーバーから読み込む。
    pub fn unbuffered_cursor(&mut self) -> crate::cursor::UnbufferedCursor<'_> {
        crate::cursor::UnbufferedCursor::new(self)
    }

    /// 接続を閉じる。
    pub async fn close(&mut self) -> Result<()> {
        // アンバッファードクエリの残りがあれば先に消費する。
        self.finish_unbuffered_query().await?;
        self.inner.close()?;
        self.pump_write().await?;
        if let Some(mut stream) = self.stream.take() {
            let _ = stream.shutdown().await;
        }
        Ok(())
    }

    /// 強制的に接続を閉じる。
    ///
    /// COM_QUIT の送信やストリームの graceful shutdown を行わず、
    /// 即座に接続を破棄する。
    pub fn force_close(&mut self) {
        self.inner.force_close();
        self.stream.take();
    }

    /// 値を SQL リテラルに変換する。
    pub fn literal(&self, obj: &Value) -> Result<String> {
        self.inner.literal(obj)
    }

    /// 文字列をエスケープする。
    pub fn escape_string(&self, s: &str) -> String {
        self.inner.escape_string(s)
    }

    /// 現在の結果セットを取得する。
    pub fn result(&self) -> Option<&MySQLResult> {
        self.inner.result()
    }

    /// 影響を受けた行数を取得する。
    pub fn affected_rows(&self) -> i64 {
        self.inner.affected_rows()
    }

    /// 最後に挿入された ID を取得する。
    pub fn insert_id(&self) -> u64 {
        self.inner.insert_id()
    }

    /// 接続が開いているかどうか。
    pub fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    /// 文字セット名を取得する。
    pub fn character_set_name(&self) -> &str {
        self.inner.character_set_name()
    }

    /// スレッド ID を取得する。
    pub fn thread_id(&self) -> u32 {
        self.inner.thread_id()
    }

    /// サーバーバージョンを取得する。
    pub fn server_version(&self) -> &str {
        self.inner.server_version()
    }

    /// サーバーへの疎通を確認する。
    ///
    /// PyMySQL の `Connection.ping` に相当する。
    pub async fn ping(&mut self) -> Result<()> {
        self.inner.ping()?;
        self.pump_write().await?;
        self.read_ok_packet().await?;
        Ok(())
    }

    /// 指定したスレッド ID の接続を終了させる。
    ///
    /// PyMySQL の `Connection.kill` に相当する。
    pub async fn kill(&mut self, thread_id: u32) -> Result<i64> {
        self.query(&format!("KILL {}", thread_id), false).await
    }

    /// データベースを切り替える。
    ///
    /// PyMySQL の `Connection.select_db` に相当する。
    pub async fn select_db(&mut self, db: &str) -> Result<()> {
        self.inner.select_db(db)?;
        self.pump_write().await?;
        self.read_ok_packet().await?;
        Ok(())
    }

    /// トランザクションを開始する。
    ///
    /// 既にトランザクション内の場合はエラーを返す。
    /// トランザクションを閉じるときは `Transaction::commit` または
    /// `Transaction::rollback` を呼ぶ。
    /// どちらも呼ばずに破棄した場合は、次の `begin()` 時に
    /// ロールバックされてから開始される。
    pub async fn begin(&mut self) -> Result<crate::transaction::Transaction<'_>> {
        crate::transaction::Transaction::begin(self).await
    }

    /// トランザクションを開始する (オプション指定)。
    pub async fn begin_with(
        &mut self,
        options: crate::transaction::TxOptions,
    ) -> Result<crate::transaction::Transaction<'_>> {
        crate::transaction::Transaction::begin_with(self, options).await
    }

    /// トランザクションをコミットする。
    ///
    /// PyMySQL の `Connection.commit` に相当する。
    /// 明示的なトランザクション管理には `begin()` が返す
    /// `Transaction` ガード型の使用を推奨する。
    pub async fn commit(&mut self) -> Result<()> {
        self.inner.commit()?;
        self.pump_write().await?;
        self.read_ok_packet().await?;
        Ok(())
    }

    /// トランザクションをロールバックする。
    ///
    /// PyMySQL の `Connection.rollback` に相当する。
    /// 明示的なトランザクション管理には `begin()` が返す
    /// `Transaction` ガード型の使用を推奨する。
    pub async fn rollback(&mut self) -> Result<()> {
        self.inner.rollback()?;
        self.pump_write().await?;
        self.read_ok_packet().await?;
        Ok(())
    }

    /// 現在のトランザクション内かどうかを返す。
    pub fn in_transaction(&self) -> bool {
        self.inner.in_transaction()
    }

    /// SHOW WARNINGS の結果を返す。
    ///
    /// PyMySQL の `Connection.show_warnings` に相当する。
    /// 各行は (レベル, コード, メッセージ) の 3 カラムを持つ。
    pub async fn show_warnings(&mut self) -> Result<Vec<Vec<Value>>> {
        self.query("SHOW WARNINGS", false).await?;
        let rows = self
            .result()
            .and_then(|r| r.rows.clone())
            .unwrap_or_default();
        Ok(rows)
    }

    /// 接続先情報を取得する。
    ///
    /// PyMySQL の `Connection.get_host_info` に相当する。
    /// Unix ドメインソケット接続の場合はソケットのパスを返す。
    pub fn get_host_info(&self) -> String {
        self.inner.get_host_info()
    }

    /// プロトコルバージョンを取得する。
    ///
    /// PyMySQL の `Connection.get_proto_info` に相当する。
    pub fn get_proto_info(&self) -> u8 {
        self.inner.get_proto_info()
    }

    /// フィールド型ごとのデコーダーを登録する。
    ///
    /// 登録したデコーダーは組み込みのデコーダーより優先される。
    pub fn register_converter(&mut self, type_code: u8, converter: crate::converters::Converter) {
        self.inner.register_converter(type_code, converter);
    }

    /// トランザクションの破棄を記録する。
    ///
    /// `Transaction` が commit / rollback されずに破棄されたときに呼ばれる。
    pub(crate) fn mark_transaction_dirty(&mut self) {
        self.transaction_dirty = true;
    }

    /// 破棄されたトランザクションをロールバックする。
    ///
    /// 接続がプールに返却される直前や、次のトランザクション開始時に呼ぶ。
    /// MySQL では autocommit が無効な状態で DML を実行すると
    /// トランザクションが暗黙的に開始されるため、
    /// 破棄されたトランザクションだけでなく進行中のトランザクションも
    /// ロールバックする。
    pub(crate) async fn rollback_dirty_transaction(&mut self) -> Result<()> {
        if self.transaction_dirty || self.in_transaction() {
            self.query("ROLLBACK", false).await?;
            self.transaction_dirty = false;
            Ok(())
        } else {
            Ok(())
        }
    }
}

/// オプションファイルを適用する。
///
/// デフォルト値のままのフィールドだけを指定グループの値で補完する。
/// PyMySQL の `read_default_file` / `read_default_group` に相当する。
fn apply_option_file(mut options: ConnectOptions) -> Result<ConnectOptions> {
    let Some(path) = options.read_default_file.take() else {
        return Ok(options);
    };
    let group = options
        .read_default_group
        .clone()
        .unwrap_or_else(|| "client".to_string());
    let option_file = OptionFile::read(&path)?;
    let defaults = ConnectOptions::default();

    if options.host == defaults.host
        && let Some(value) = option_file.get(&group, "host")
    {
        options.host = value.to_string();
    }
    if options.port == defaults.port
        && let Some(value) = option_file.get(&group, "port")
    {
        options.port = value.parse().map_err(|_| Error::OperationalError {
            code: client_error::CR_UNKNOWN_ERROR,
            message: format!("Invalid port in option file {}: {}", path.display(), value),
        })?;
    }
    if options.user.is_empty()
        && let Some(value) = option_file.get(&group, "user")
    {
        options.user = value.to_string();
    }
    if options.password.is_empty()
        && let Some(value) = option_file.get(&group, "password")
    {
        options.password = value.as_bytes().to_vec();
    }
    if options.database.is_none()
        && let Some(value) = option_file.get(&group, "database")
    {
        options.database = Some(value.to_string());
    }
    // PyMySQL と同じく socket キーは Unix ドメインソケットのパスを表す。
    // host がデフォルト値のままのときに限り適用する。
    if options.host == defaults.host
        && let Some(value) = option_file.get(&group, "socket")
    {
        options.host = value.to_string();
    }
    if options.charset == defaults.charset
        && let Some(value) = option_file.get(&group, "default-character-set")
    {
        options.charset = value.to_string();
    }
    if options.ssl_ca.is_none()
        && let Some(value) = option_file.get(&group, "ssl-ca")
    {
        options.ssl_ca = Some(value.to_string());
    }
    if options.ssl_cert.is_none()
        && let Some(value) = option_file.get(&group, "ssl-cert")
    {
        options.ssl_cert = Some(value.to_string());
    }
    if options.ssl_key.is_none()
        && let Some(value) = option_file.get(&group, "ssl-key")
    {
        options.ssl_key = Some(value.to_string());
    }
    Ok(options)
}

/// `~` で始まるパスをユーザーのホームディレクトリに展開する。
fn expand_user(path: &str) -> String {
    if path == "~" {
        home_dir().unwrap_or_default()
    } else if let Some(rest) = path.strip_prefix("~/") {
        format!("{}/{}", home_dir().unwrap_or_default(), rest)
    } else {
        path.to_string()
    }
}

/// ホームディレクトリを取得する。
fn home_dir() -> Option<String> {
    std::env::var("HOME")
        .ok()
        .or_else(|| std::env::var("USERPROFILE").ok())
}

/// クエリ文字列に引数を埋め込む共通実装。
pub(crate) fn mogrify_query<F>(query: &str, args: Option<&[Value]>, literal: F) -> Result<String>
where
    F: FnMut(&Value) -> Result<String>,
{
    match args {
        None => Ok(query.to_string()),
        Some(args) => {
            let escaped: Vec<String> = args
                .iter()
                .map(literal)
                .collect::<std::result::Result<Vec<_>, _>>()?;
            let mut result = String::with_capacity(query.len());
            let mut chars = query.chars().peekable();
            let mut arg_iter = escaped.iter();
            while let Some(c) = chars.next() {
                if c == '%' && chars.peek() == Some(&'s') {
                    chars.next();
                    match arg_iter.next() {
                        Some(arg) => result.push_str(arg),
                        None => {
                            return Err(Error::ProgrammingError {
                                code: client_error::CR_INVALID_PARAMETER_NO,
                                message: "Not enough placeholders for arguments".to_string(),
                            });
                        }
                    }
                } else if c == '%' && chars.peek() == Some(&'%') {
                    chars.next();
                    result.push('%');
                } else {
                    result.push(c);
                }
            }
            if arg_iter.next().is_some() {
                return Err(Error::ProgrammingError {
                    code: client_error::CR_INVALID_PARAMETER_NO,
                    message: "Too many arguments for placeholders".to_string(),
                });
            }
            Ok(result)
        }
    }
}

/// ホスト名または IP アドレスから TLS の ServerName を生成する。
fn server_name_from_host(host: &str) -> Result<ServerName<'static>> {
    let host = host
        .strip_prefix('[')
        .and_then(|h| h.strip_suffix(']'))
        .unwrap_or(host);
    if let Ok(ip) = std::net::IpAddr::from_str(host) {
        return Ok(ServerName::IpAddress(ip.into()));
    }
    ServerName::try_from(host.to_string()).map_err(|_| Error::OperationalError {
        code: client_error::CR_SSL_CONNECTION_ERROR,
        message: "Invalid server hostname for TLS".to_string(),
    })
}

/// ホスト名検証のみをスキップする verifier。
///
/// 内部の verifier で証明書チェーン・署名検証は行い、
/// ホスト名不一致に起因するエラーのみを成功として変換する。
#[derive(Debug)]
struct NoHostnameVerifier {
    inner: Arc<dyn ServerCertVerifier>,
}

impl NoHostnameVerifier {
    fn new(inner: Arc<dyn ServerCertVerifier>) -> Self {
        Self { inner }
    }
}

impl ServerCertVerifier for NoHostnameVerifier {
    fn verify_server_cert(
        &self,
        end_entity: &rustls::pki_types::CertificateDer<'_>,
        intermediates: &[rustls::pki_types::CertificateDer<'_>],
        server_name: &ServerName<'_>,
        ocsp_response: &[u8],
        now: UnixTime,
    ) -> std::result::Result<ServerCertVerified, RustlsError> {
        match self.inner.verify_server_cert(
            end_entity,
            intermediates,
            server_name,
            ocsp_response,
            now,
        ) {
            Ok(assertion) => Ok(assertion),
            Err(RustlsError::InvalidCertificate(
                rustls::CertificateError::NotValidForName
                | rustls::CertificateError::NotValidForNameContext { .. },
            )) => Ok(ServerCertVerified::assertion()),
            Err(e) => Err(e),
        }
    }

    fn verify_tls12_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls12_signature(message, cert, dss)
    }

    fn verify_tls13_signature(
        &self,
        message: &[u8],
        cert: &rustls::pki_types::CertificateDer<'_>,
        dss: &DigitallySignedStruct,
    ) -> std::result::Result<HandshakeSignatureValid, RustlsError> {
        self.inner.verify_tls13_signature(message, cert, dss)
    }

    fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
        self.inner.supported_verify_schemes()
    }
}

/// TLS 設定を構築する。
async fn build_tls_config(options: &ConnectOptions) -> Result<rustls::ClientConfig> {
    let provider = Arc::new(rustls::crypto::aws_lc_rs::default_provider());
    let builder = rustls::ClientConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()
        .map_err(|e| Error::OperationalError {
            code: client_error::CR_SSL_CONNECTION_ERROR,
            message: format!("Failed to configure TLS protocol versions: {}", e),
        })?;

    let config = if options.ssl_verify_identity {
        if let Some(ca_path) = &options.ssl_ca {
            let mut root_store = rustls::RootCertStore::empty();
            let cert_file =
                tokio::fs::read(ca_path)
                    .await
                    .map_err(|e| Error::OperationalError {
                        code: client_error::CR_SSL_CONNECTION_ERROR,
                        message: format!("Failed to read CA file: {}", e),
                    })?;
            let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_file)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: format!("Failed to parse CA file: {}", e),
                })?;
            root_store.add_parsable_certificates(certs);
            builder.with_root_certificates(root_store)
        } else {
            builder
                .with_platform_verifier()
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: format!("Failed to configure platform verifier: {}", e),
                })?
        }
    } else {
        let verifier: Arc<dyn ServerCertVerifier> = if let Some(ca_path) = &options.ssl_ca {
            let mut root_store = rustls::RootCertStore::empty();
            let cert_file =
                tokio::fs::read(ca_path)
                    .await
                    .map_err(|e| Error::OperationalError {
                        code: client_error::CR_SSL_CONNECTION_ERROR,
                        message: format!("Failed to read CA file: {}", e),
                    })?;
            let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_file)
                .collect::<std::result::Result<Vec<_>, _>>()
                .map_err(|e| Error::OperationalError {
                    code: client_error::CR_SSL_CONNECTION_ERROR,
                    message: format!("Failed to parse CA file: {}", e),
                })?;
            root_store.add_parsable_certificates(certs);
            let inner =
                WebPkiServerVerifier::builder_with_provider(Arc::new(root_store), provider.clone())
                    .build()
                    .map_err(|e| Error::OperationalError {
                        code: client_error::CR_SSL_CONNECTION_ERROR,
                        message: format!("Failed to build webpki verifier: {}", e),
                    })?;
            Arc::new(NoHostnameVerifier::new(inner))
        } else {
            let inner = Verifier::new(provider).map_err(|e| Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: format!("Failed to create platform verifier: {}", e),
            })?;
            Arc::new(NoHostnameVerifier::new(Arc::new(inner)))
        };
        builder
            .dangerous()
            .with_custom_certificate_verifier(verifier)
    };

    if (options.ssl_cert.is_some()) != (options.ssl_key.is_some()) {
        return Err(Error::OperationalError {
            code: client_error::CR_SSL_CONNECTION_ERROR,
            message: "ssl_cert and ssl_key must be specified together".to_string(),
        });
    }

    let config = if let (Some(cert_path), Some(key_path)) = (&options.ssl_cert, &options.ssl_key) {
        let cert_file = tokio::fs::read(cert_path)
            .await
            .map_err(|e| Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: format!("Failed to read client cert file: {}", e),
            })?;
        let certs: Vec<CertificateDer<'static>> = CertificateDer::pem_slice_iter(&cert_file)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: format!("Failed to parse client cert file: {}", e),
            })?;

        let key_file = tokio::fs::read(key_path)
            .await
            .map_err(|e| Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: format!("Failed to read client key file: {}", e),
            })?;
        let keys: Vec<PrivatePkcs8KeyDer<'static>> = PrivatePkcs8KeyDer::pem_slice_iter(&key_file)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(|e| Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: format!("Failed to parse client key file: {}", e),
            })?;
        if keys.is_empty() {
            return Err(Error::OperationalError {
                code: client_error::CR_SSL_CONNECTION_ERROR,
                message: "No PKCS8 private key found".to_string(),
            });
        }
        config.with_client_auth_cert(
            certs,
            rustls::pki_types::PrivateKeyDer::Pkcs8(
                keys.into_iter()
                    .next()
                    .expect("at least one PKCS8 private key was verified above"),
            ),
        )
    } else {
        Ok(config.with_no_client_auth())
    };

    config.map_err(|e| Error::OperationalError {
        code: client_error::CR_SSL_CONNECTION_ERROR,
        message: format!("Failed to build TLS config: {}", e),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::converters::Value;

    fn dummy_literal(value: &Value) -> Result<String> {
        Ok(format!("[{}]", value.to_sql("utf8").unwrap()))
    }

    #[test]
    fn test_mogrify_query_basic() {
        let result = mogrify_query(
            "SELECT %s, %s",
            Some(&[Value::Int(1), Value::Int(2)]),
            dummy_literal,
        )
        .unwrap();
        assert_eq!(result, "SELECT [1], [2]");
    }

    #[test]
    fn test_mogrify_query_escape_placeholder() {
        let result = mogrify_query("SELECT %%s", Some(&[]), dummy_literal).unwrap();
        assert_eq!(result, "SELECT %s");
    }

    #[test]
    fn test_mogrify_query_not_enough_arguments() {
        let result = mogrify_query("SELECT %s, %s", Some(&[Value::Int(1)]), dummy_literal);
        assert!(result.is_err());
    }

    #[test]
    fn test_mogrify_query_too_many_arguments() {
        let result = mogrify_query(
            "SELECT %s",
            Some(&[Value::Int(1), Value::Int(2)]),
            dummy_literal,
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_server_name_from_host_ipv4() {
        let name = server_name_from_host("127.0.0.1").unwrap();
        assert!(matches!(name, ServerName::IpAddress(_)));
    }

    #[test]
    fn test_server_name_from_host_ipv6() {
        let name = server_name_from_host("::1").unwrap();
        assert!(matches!(name, ServerName::IpAddress(_)));
    }

    #[test]
    fn test_server_name_from_host_bracketed_ipv6() {
        let name = server_name_from_host("[::1]").unwrap();
        assert!(matches!(name, ServerName::IpAddress(_)));
    }

    #[test]
    fn test_server_name_from_host_dns() {
        let name = server_name_from_host("example.com").unwrap();
        assert!(matches!(name, ServerName::DnsName(_)));
    }
}
