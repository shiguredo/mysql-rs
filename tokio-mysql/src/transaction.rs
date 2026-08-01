// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! トランザクション。
//!
//! `Connection::begin` で開始し、`commit` / `rollback` で終了する。
//! セーブポイントは `Transaction::savepoint` で作成する。
//!
//! `commit` / `rollback` のどちらも呼ばずに `Transaction` を破棄した場合は、
//! 次の `begin()` 時またはプールへの返却時にロールバックされる。

use crate::connection::Connection;
use shiguredo_mysql::constants::client_error;
use shiguredo_mysql::converters::Value;
use shiguredo_mysql::error::{Error, Result};

/// トランザクションの分離レベル。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum IsolationLevel {
    ReadUncommitted,
    ReadCommitted,
    RepeatableRead,
    Serializable,
}

impl IsolationLevel {
    fn as_sql(self) -> &'static str {
        match self {
            IsolationLevel::ReadUncommitted => "READ UNCOMMITTED",
            IsolationLevel::ReadCommitted => "READ COMMITTED",
            IsolationLevel::RepeatableRead => "REPEATABLE READ",
            IsolationLevel::Serializable => "SERIALIZABLE",
        }
    }
}

/// トランザクションの開始オプション。
#[derive(Debug, Clone, Copy, Default)]
pub struct TxOptions {
    /// 分離レベル。`None` の場合はサーバーのデフォルトを使う。
    pub isolation_level: Option<IsolationLevel>,
    /// 読み取り専用トランザクションにするかどうか。
    pub read_only: bool,
}

/// トランザクション。
pub struct Transaction<'a> {
    conn: &'a mut Connection,
    finished: bool,
}

impl<'a> Transaction<'a> {
    /// トランザクションを開始する。
    ///
    /// 既にトランザクション内の場合はエラーを返す。
    pub(crate) async fn begin(conn: &'a mut Connection) -> Result<Self> {
        Self::begin_with(conn, TxOptions::default()).await
    }

    /// オプションを指定してトランザクションを開始する。
    pub(crate) async fn begin_with(conn: &'a mut Connection, options: TxOptions) -> Result<Self> {
        // 破棄されたトランザクションを先にロールバックする。
        conn.rollback_dirty_transaction().await?;
        if conn.in_transaction() {
            return Err(Error::ProgrammingError {
                code: client_error::CR_COMMANDS_OUT_OF_SYNC,
                message: "Cannot begin a transaction while already in one".to_string(),
            });
        }
        if let Some(level) = options.isolation_level {
            // 分離レベルは次のトランザクションにのみ適用されるため、
            // トランザクション開始前に設定する。
            let sql = format!("SET TRANSACTION ISOLATION LEVEL {}", level.as_sql());
            conn.query(&sql, false).await?;
        }
        let sql = if options.read_only {
            "START TRANSACTION READ ONLY"
        } else {
            "START TRANSACTION"
        };
        conn.query(sql, false).await?;
        Ok(Self {
            conn,
            finished: false,
        })
    }

    /// トランザクションをコミットする。
    pub async fn commit(mut self) -> Result<()> {
        self.finished = true;
        self.conn.query("COMMIT", false).await?;
        tracing::debug!("Transaction committed");
        Ok(())
    }

    /// トランザクションをロールバックする。
    pub async fn rollback(mut self) -> Result<()> {
        self.finished = true;
        self.conn.query("ROLLBACK", false).await?;
        tracing::debug!("Transaction rolled back");
        Ok(())
    }

    /// トランザクション内でクエリを実行する (引数付き)。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        self.conn.execute(query, args).await
    }

    /// トランザクション内でクエリを実行する。
    pub async fn query(&mut self, sql: &str, unbuffered: bool) -> Result<i64> {
        self.conn.query(sql, unbuffered).await
    }

    /// セーブポイントを作成する。
    ///
    /// `name` は SQL 識別子としてそのまま埋め込まれるため、
    /// 識別子として有効な名前を渡すこと。
    /// `rollback_to` でセーブポイントまで戻り、`release_savepoint` で破棄する。
    /// セーブポイントはトランザクション終了時に自動的に破棄される。
    pub async fn savepoint(&mut self, name: &str) -> Result<()> {
        let sql = format!("SAVEPOINT {}", name);
        self.conn.query(&sql, false).await.map(|_| ())
    }

    /// セーブポイントまでロールバックする。
    ///
    /// セーブポイント作成後の変更は破棄されるが、
    /// トランザクション自体は継続する。
    pub async fn rollback_to(&mut self, name: &str) -> Result<()> {
        let sql = format!("ROLLBACK TO SAVEPOINT {}", name);
        self.conn.query(&sql, false).await.map(|_| ())
    }

    /// セーブポイントを破棄する。
    pub async fn release_savepoint(&mut self, name: &str) -> Result<()> {
        let sql = format!("RELEASE SAVEPOINT {}", name);
        self.conn.query(&sql, false).await.map(|_| ())
    }

    /// 内部の接続への可変参照を取得する。
    ///
    /// トランザクション内でカーソル等を使用する場合に使う。
    pub fn conn(&mut self) -> &mut Connection {
        self.conn
    }
}

impl Drop for Transaction<'_> {
    fn drop(&mut self) {
        if !self.finished {
            // 非同期コンテキスト外のためここではロールバックできない。
            // 次の begin() またはプールへの返却時にロールバックされる。
            tracing::warn!("Transaction dropped without commit or rollback");
            self.conn.mark_transaction_dirty();
        }
    }
}
