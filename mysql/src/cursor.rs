// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! カーソル実装。

use crate::connection::Connection;
use crate::connection::MySQLResult;
use crate::converters::Value;
use crate::error::{Error, Result};
use crate::protocol::ColumnDescription;
use regex::Regex;
use std::collections::HashMap;
use std::sync::OnceLock;

/// 挿入文の VALUES 句を検出する正規表現。
fn insert_values_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(
            r"(?is)\A\s*((?:INSERT|REPLACE)\b.+\bVALUES?\s*)(\(\s*%s\s*(?:,\s*%s\s*)*\))(\s*(?:ON\s+DUPLICATE\s+KEY\s+UPDATE\b[^;]*)?);?\s*\z"
        )
        .expect("static INSERT/REPLACE regex pattern is valid")
    })
}

/// 標準カーソル。
pub struct Cursor<'a> {
    connection: &'a mut Connection,
    warning_count: u16,
    description: Option<Vec<ColumnDescription>>,
    row_number: usize,
    row_count: i64,
    pub(crate) array_size: usize,
    executed: Option<String>,
    result: Option<MySQLResult>,
    rows: Option<Vec<Vec<Value>>>,
    closed: bool,
}

impl<'a> Cursor<'a> {
    /// 新規カーソルを作成する。
    pub fn new(connection: &'a mut Connection) -> Self {
        Self {
            connection,
            warning_count: 0,
            description: None,
            row_number: 0,
            row_count: -1,
            array_size: 1,
            executed: None,
            result: None,
            rows: None,
            closed: false,
        }
    }

    /// カーソルを閉じる。
    pub async fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        while self.next_set().await? {
            // 残りの結果セットをすべて消費する。
        }
        self.closed = true;
        Ok(())
    }

    fn check_closed(&self) -> Result<()> {
        if self.closed {
            Err(Error::ProgrammingError {
                code: crate::constants::client_error::CR_COMMANDS_OUT_OF_SYNC,
                message: "Cursor closed".to_string(),
            })
        } else {
            Ok(())
        }
    }

    /// クエリを文字列化する。
    pub fn mogrify(&mut self, query: &str, args: Option<&[Value]>) -> Result<String> {
        self.check_closed()?;
        self.connection.mogrify(query, args)
    }

    /// クエリを実行する。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        self.check_closed()?;

        // 前の結果セットをすべて消費する。
        while self.next_set().await? {
            // 残りの結果セットをすべて消費する。
        }

        let query = self.connection.mogrify(query, args)?;
        let affected = self.connection.query(&query, false).await?;
        self.refresh_result();
        self.executed = Some(query);
        Ok(affected)
    }

    /// 同じクエリを複数回実行する。
    pub async fn execute_many(&mut self, query: &str, args_list: &[Vec<Value>]) -> Result<i64> {
        self.check_closed()?;
        if args_list.is_empty() {
            return Ok(0);
        }

        if let Some(caps) = insert_values_re().captures(query) {
            let prefix = caps
                .get(1)
                .expect("regex capture group 1 is present for INSERT/REPLACE prefix")
                .as_str();
            let values = caps
                .get(2)
                .expect("regex capture group 2 is present for INSERT/REPLACE values")
                .as_str();
            let postfix = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            return self
                .do_execute_many(prefix, values, postfix, args_list)
                .await;
        }

        let mut total = 0_i64;
        for args in args_list {
            total += self.execute(query, Some(args)).await?;
        }
        self.row_count = total;
        Ok(total)
    }

    async fn do_execute_many(
        &mut self,
        prefix: &str,
        values: &str,
        postfix: &str,
        args_list: &[Vec<Value>],
    ) -> Result<i64> {
        self.check_closed()?;
        let mut sql = prefix.to_string();
        let mut rows = 0_i64;
        let max_len = 1024000_usize;

        for (i, args) in args_list.iter().enumerate() {
            let value_str = self.connection.mogrify(values, Some(args))?;
            let separator_len = if i > 0 { 1 } else { 0 };
            if sql.len() + value_str.len() + postfix.len() + separator_len > max_len {
                if i == 0 {
                    return Err(Error::DataError {
                        code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                        message: "execute_many row exceeds maximum query length".to_string(),
                    });
                }
                rows += self.execute(&(sql.clone() + postfix), None).await?;
                sql = prefix.to_string();
            }
            if i > 0 {
                sql.push(',');
            }
            sql.push_str(&value_str);
        }
        rows += self.execute(&(sql + postfix), None).await?;
        self.row_count = rows;
        Ok(rows)
    }

    /// ストアドプロシージャを呼び出す。
    pub async fn call_procedure(&mut self, procname: &str, args: &[Value]) -> Result<Vec<Value>> {
        self.check_closed()?;
        let escaped_procname = procname.replace('`', "``");

        if !args.is_empty() {
            let mut set_query_parts = Vec::new();
            for (i, arg) in args.iter().enumerate() {
                set_query_parts.push(format!(
                    "@`_{}_{}`={}",
                    escaped_procname,
                    i,
                    self.connection.literal(arg)?
                ));
            }
            let set_query = set_query_parts.join(",");
            self.execute(&format!("SET {}", set_query), None).await?;
        }

        let call_args = (0..args.len())
            .map(|i| format!("@`_{}_{}`", escaped_procname, i))
            .collect::<Vec<_>>()
            .join(",");
        let query = format!("CALL `{}`({})", escaped_procname, call_args);
        self.execute(&query, None).await?;

        // CALL 実行後に残っている結果セットをすべて消費する。
        while self.next_set().await? {}

        if args.is_empty() {
            return Ok(Vec::new());
        }

        // OUT/INOUT 引数に対応するユーザー変数を SELECT して値を置き換える。
        let select_parts: Vec<String> = (0..args.len())
            .map(|i| format!("@`_{}_{}` AS `{}`", escaped_procname, i, i))
            .collect();
        self.execute(&format!("SELECT {}", select_parts.join(",")), None)
            .await?;
        let row = self.fetch_one()?.ok_or_else(|| Error::ProgrammingError {
            code: crate::constants::client_error::CR_NO_RESULT_SET,
            message: "Failed to fetch procedure output variables".to_string(),
        })?;
        let mut result = args.to_vec();
        for (i, value) in row.iter().enumerate() {
            if let Some(slot) = result.get_mut(i) {
                *slot = value.clone();
            }
        }
        Ok(result)
    }

    /// 次の行を取得する。
    pub fn fetch_one(&mut self) -> Result<Option<&Vec<Value>>> {
        self.check_executed()?;
        match &self.rows {
            None => Ok(None),
            Some(rows) => {
                if self.row_number >= rows.len() {
                    Ok(None)
                } else {
                    let row = &rows[self.row_number];
                    self.row_number += 1;
                    Ok(Some(row))
                }
            }
        }
    }

    /// 複数行を取得する。
    pub fn fetch_many(&mut self, size: Option<usize>) -> Result<Vec<&Vec<Value>>> {
        self.check_executed()?;
        match &self.rows {
            None => Ok(Vec::new()),
            Some(rows) => {
                let size = size.unwrap_or(self.array_size);
                let end = (self.row_number + size).min(rows.len());
                let result: Vec<_> = rows[self.row_number..end].iter().collect();
                self.row_number = end;
                Ok(result)
            }
        }
    }

    /// 全行を取得する。
    pub fn fetch_all(&mut self) -> Result<Vec<&Vec<Value>>> {
        self.check_executed()?;
        match &self.rows {
            None => Ok(Vec::new()),
            Some(rows) => {
                let result: Vec<_> = rows[self.row_number..].iter().collect();
                self.row_number = rows.len();
                Ok(result)
            }
        }
    }

    /// カーソル位置を移動する。
    pub fn scroll(&mut self, value: isize, mode: &str) -> Result<()> {
        self.check_executed()?;
        let rows = self.rows.as_ref().ok_or_else(|| Error::ProgrammingError {
            code: crate::constants::client_error::CR_NO_RESULT_SET,
            message: "No result set".to_string(),
        })?;
        let new_pos = match mode {
            "relative" => self.row_number as isize + value,
            "absolute" => value,
            _ => {
                return Err(Error::ProgrammingError {
                    code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                    message: format!("unknown scroll mode {}", mode),
                });
            }
        };
        if new_pos < 0 || new_pos as usize >= rows.len() {
            return Err(Error::ProgrammingError {
                code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                message: "out of range".to_string(),
            });
        }
        self.row_number = new_pos as usize;
        Ok(())
    }

    fn check_executed(&self) -> Result<()> {
        if self.executed.is_none() {
            Err(Error::ProgrammingError {
                code: crate::constants::client_error::CR_NO_RESULT_SET,
                message: "execute() first".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn refresh_result(&mut self) {
        let result = self.connection.result().cloned();
        if let Some(result) = result {
            self.row_count = result.affected_rows;
            self.warning_count = result.warning_count;
            self.description = result.description.clone();
            self.rows = result.rows.clone();
            self.result = Some(result);
            self.row_number = 0;
        }
    }

    /// 次の結果セットに移動する。
    pub async fn next_set(&mut self) -> Result<bool> {
        let current_result = match &self.result {
            Some(r) => r,
            None => return Ok(false),
        };
        if !current_result.has_next {
            return Ok(false);
        }
        let _ = self.connection.next_result(false).await?;
        self.refresh_result();
        Ok(self.result.as_ref().map(|r| r.has_next).unwrap_or(false))
    }
}

impl Drop for Cursor<'_> {
    fn drop(&mut self) {
        if !self.closed {
            // 非同期処理は drop では実行できないため、closed フラグのみ立てる。
            self.closed = true;
        }
    }
}

/// 辞書形式で結果を返すカーソル。
pub struct DictCursor<'a> {
    inner: Cursor<'a>,
    fields: Vec<String>,
    fields_dirty: bool,
}

impl<'a> DictCursor<'a> {
    pub fn new(cursor: Cursor<'a>) -> Self {
        Self {
            inner: cursor,
            fields: Vec::new(),
            fields_dirty: true,
        }
    }

    fn build_fields(&mut self) {
        self.fields = self
            .inner
            .description
            .as_ref()
            .map(|desc| desc.iter().map(|d| d.name.clone()).collect())
            .unwrap_or_default();
    }

    fn ensure_fields(&mut self) {
        if self.fields_dirty {
            self.build_fields();
            self.fields_dirty = false;
        }
    }

    /// クエリを実行する。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        let affected = self.inner.execute(query, args).await?;
        self.fields_dirty = true;
        Ok(affected)
    }

    /// 同じクエリを複数回実行する。
    pub async fn execute_many(&mut self, query: &str, args_list: &[Vec<Value>]) -> Result<i64> {
        self.inner.execute_many(query, args_list).await
    }

    /// ストアドプロシージャを呼び出す。
    pub async fn call_procedure(&mut self, procname: &str, args: &[Value]) -> Result<Vec<Value>> {
        self.inner.call_procedure(procname, args).await
    }

    /// カーソルを閉じる。
    pub async fn close(&mut self) -> Result<()> {
        self.inner.close().await
    }

    /// 次の結果セットに移動する。
    pub async fn next_set(&mut self) -> Result<bool> {
        let has_next = self.inner.next_set().await?;
        self.fields_dirty = true;
        Ok(has_next)
    }

    /// クエリ文字列に引数を埋め込む。
    pub fn mogrify(&mut self, query: &str, args: Option<&[Value]>) -> Result<String> {
        self.inner.mogrify(query, args)
    }

    /// 次の行を辞書形式で取得する。
    pub fn fetch_one(&mut self) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_fields();
        match self.inner.fetch_one()? {
            None => Ok(None),
            Some(row) => {
                let mut dict = HashMap::new();
                for (field, value) in self.fields.iter().zip(row.iter()) {
                    dict.insert(field.clone(), value.clone());
                }
                Ok(Some(dict))
            }
        }
    }

    /// 複数行を辞書形式で取得する。
    pub fn fetch_many(&mut self, size: Option<usize>) -> Result<Vec<HashMap<String, Value>>> {
        let mut result = Vec::new();
        let size = size.unwrap_or(self.inner.array_size);
        for _ in 0..size {
            match self.fetch_one()? {
                Some(row) => result.push(row),
                None => break,
            }
        }
        Ok(result)
    }

    /// 全行を辞書形式で取得する。
    pub fn fetch_all(&mut self) -> Result<Vec<HashMap<String, Value>>> {
        let mut result = Vec::new();
        while let Some(row) = self.fetch_one()? {
            result.push(row);
        }
        Ok(result)
    }

    /// カーソル位置を移動する。
    pub fn scroll(&mut self, value: isize, mode: &str) -> Result<()> {
        self.inner.scroll(value, mode)
    }
}

/// アンバッファードカーソル。
///
/// 行をメモリに蓄えず、fetch のたびにサーバーから読み込む。
/// 巨大な結果セットや低速ネットワーク向けに PyMySQL の `SSCursor` に相当する。
///
/// 行の合計数は最後まで読み込まないと分からない。
/// 後方スクロールはできない。
/// ストアドプロシージャには `Cursor` を使用すること。
pub struct UnbufferedCursor<'a> {
    connection: &'a mut Connection,
    array_size: usize,
    row_number: usize,
    row_count: i64,
    description: Option<Vec<ColumnDescription>>,
    executed: bool,
    closed: bool,
}

impl<'a> UnbufferedCursor<'a> {
    /// 新規カーソルを作成する。
    pub fn new(connection: &'a mut Connection) -> Self {
        Self {
            connection,
            array_size: 1,
            row_number: 0,
            row_count: -1,
            description: None,
            executed: false,
            closed: false,
        }
    }

    /// カーソルを閉じる。
    ///
    /// アンバッファードクエリの残りの行をすべて消費してから閉じる。
    pub async fn close(&mut self) -> Result<()> {
        if self.closed {
            return Ok(());
        }
        self.connection.finish_unbuffered_query().await?;
        self.closed = true;
        Ok(())
    }

    fn check_closed(&self) -> Result<()> {
        if self.closed {
            Err(Error::ProgrammingError {
                code: crate::constants::client_error::CR_COMMANDS_OUT_OF_SYNC,
                message: "Cursor closed".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn check_executed(&self) -> Result<()> {
        if !self.executed {
            Err(Error::ProgrammingError {
                code: crate::constants::client_error::CR_NO_RESULT_SET,
                message: "execute() first".to_string(),
            })
        } else {
            Ok(())
        }
    }

    fn refresh_description(&mut self) {
        self.description = self.connection.result().and_then(|r| r.description.clone());
    }

    /// クエリを文字列化する。
    pub fn mogrify(&mut self, query: &str, args: Option<&[Value]>) -> Result<String> {
        self.check_closed()?;
        self.connection.mogrify(query, args)
    }

    /// クエリを実行する。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        self.check_closed()?;
        let query = self.connection.mogrify(query, args)?;
        let affected = self.connection.query(&query, true).await?;
        self.executed = true;
        self.row_number = 0;
        self.row_count = affected;
        self.refresh_description();
        Ok(affected)
    }

    /// 同じクエリを複数回実行する。
    ///
    /// 実行のたびに結果セットをすべて消費する。
    pub async fn execute_many(&mut self, query: &str, args_list: &[Vec<Value>]) -> Result<i64> {
        self.check_closed()?;
        if args_list.is_empty() {
            return Ok(0);
        }

        if let Some(caps) = insert_values_re().captures(query) {
            let prefix = caps
                .get(1)
                .expect("regex capture group 1 is present for INSERT/REPLACE prefix")
                .as_str();
            let values = caps
                .get(2)
                .expect("regex capture group 2 is present for INSERT/REPLACE values")
                .as_str();
            let postfix = caps.get(3).map(|m| m.as_str()).unwrap_or("");
            let mut sql = prefix.to_string();
            let mut rows = 0_i64;
            let max_len = 1024000_usize;

            for (i, args) in args_list.iter().enumerate() {
                let value_str = self.connection.mogrify(values, Some(args))?;
                let separator_len = if i > 0 { 1 } else { 0 };
                if sql.len() + value_str.len() + postfix.len() + separator_len > max_len {
                    if i == 0 {
                        return Err(Error::DataError {
                            code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                            message: "execute_many row exceeds maximum query length".to_string(),
                        });
                    }
                    rows += self.execute(&(sql.clone() + postfix), None).await?;
                    sql = prefix.to_string();
                }
                if i > 0 {
                    sql.push(',');
                }
                sql.push_str(&value_str);
            }
            rows += self.execute(&(sql + postfix), None).await?;
            self.row_count = rows;
            return Ok(rows);
        }

        let mut total = 0_i64;
        for args in args_list {
            total += self.execute(query, Some(args)).await?;
        }
        self.row_count = total;
        Ok(total)
    }

    /// 次の行を取得する。
    ///
    /// 結果セットの末尾に達した場合は `None` を返す。
    pub async fn fetch_one(&mut self) -> Result<Option<Vec<Value>>> {
        self.check_closed()?;
        self.check_executed()?;
        let row = self.connection.next_unbuffered_row().await?;
        if row.is_some() {
            self.row_number += 1;
        }
        Ok(row)
    }

    /// 複数行を取得する。
    pub async fn fetch_many(&mut self, size: Option<usize>) -> Result<Vec<Vec<Value>>> {
        self.check_closed()?;
        self.check_executed()?;
        let size = size.unwrap_or(self.array_size);
        let mut rows = Vec::new();
        for _ in 0..size {
            let Some(row) = self.connection.next_unbuffered_row().await? else {
                break;
            };
            self.row_number += 1;
            rows.push(row);
        }
        Ok(rows)
    }

    /// 全行を取得する。
    pub async fn fetch_all(&mut self) -> Result<Vec<Vec<Value>>> {
        self.check_closed()?;
        self.check_executed()?;
        let mut rows = Vec::new();
        while let Some(row) = self.connection.next_unbuffered_row().await? {
            self.row_number += 1;
            rows.push(row);
        }
        Ok(rows)
    }

    /// カーソル位置を移動する。
    ///
    /// 後方スクロールはできない。前方への移動は行を読み飛ばすことで行う。
    pub async fn scroll(&mut self, value: isize, mode: &str) -> Result<()> {
        self.check_closed()?;
        self.check_executed()?;
        let skip = match mode {
            "relative" => {
                if value < 0 {
                    return Err(Error::NotSupportedError {
                        code: crate::constants::client_error::CR_UNKNOWN_ERROR,
                        message: "Backwards scrolling not supported by this cursor".to_string(),
                    });
                }
                value as usize
            }
            "absolute" => {
                let target = value as usize;
                if target < self.row_number {
                    return Err(Error::NotSupportedError {
                        code: crate::constants::client_error::CR_UNKNOWN_ERROR,
                        message: "Backwards scrolling not supported by this cursor".to_string(),
                    });
                }
                target - self.row_number
            }
            _ => {
                return Err(Error::ProgrammingError {
                    code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                    message: format!("unknown scroll mode {}", mode),
                });
            }
        };
        for _ in 0..skip {
            match self.connection.next_unbuffered_row().await? {
                Some(_) => self.row_number += 1,
                None => {
                    return Err(Error::ProgrammingError {
                        code: crate::constants::client_error::CR_INVALID_PARAMETER_NO,
                        message: "out of range".to_string(),
                    });
                }
            }
        }
        Ok(())
    }

    /// 次の結果セットに移動する。
    pub async fn next_set(&mut self) -> Result<bool> {
        self.check_closed()?;
        // 現在の結果セットの残りの行をすべて消費する。
        self.connection.finish_unbuffered_query().await?;
        let has_next = self
            .connection
            .result()
            .map(|r| r.has_next)
            .unwrap_or(false);
        if !has_next {
            return Ok(false);
        }
        let _ = self.connection.next_result(true).await?;
        self.row_number = 0;
        self.refresh_description();
        Ok(true)
    }
}

impl Drop for UnbufferedCursor<'_> {
    fn drop(&mut self) {
        if !self.closed {
            // 非同期処理は drop では実行できないため、closed フラグのみ立てる。
            // 残りの行は次のクエリ実行時に消費される。
            self.closed = true;
        }
    }
}

/// アンバッファードカーソルの辞書形式版。
///
/// PyMySQL の `SSDictCursor` に相当する。
pub struct UnbufferedDictCursor<'a> {
    inner: UnbufferedCursor<'a>,
    fields: Vec<String>,
    fields_dirty: bool,
}

impl<'a> UnbufferedDictCursor<'a> {
    /// 新規カーソルを作成する。
    pub fn new(cursor: UnbufferedCursor<'a>) -> Self {
        Self {
            inner: cursor,
            fields: Vec::new(),
            fields_dirty: true,
        }
    }

    fn ensure_fields(&mut self) {
        if self.fields_dirty {
            self.fields = self
                .inner
                .description
                .as_ref()
                .map(|desc| desc.iter().map(|d| d.name.clone()).collect())
                .unwrap_or_default();
            self.fields_dirty = false;
        }
    }

    /// クエリを実行する。
    pub async fn execute(&mut self, query: &str, args: Option<&[Value]>) -> Result<i64> {
        let affected = self.inner.execute(query, args).await?;
        self.fields_dirty = true;
        Ok(affected)
    }

    /// 同じクエリを複数回実行する。
    pub async fn execute_many(&mut self, query: &str, args_list: &[Vec<Value>]) -> Result<i64> {
        self.inner.execute_many(query, args_list).await
    }

    /// カーソルを閉じる。
    pub async fn close(&mut self) -> Result<()> {
        self.inner.close().await
    }

    /// 次の結果セットに移動する。
    pub async fn next_set(&mut self) -> Result<bool> {
        let has_next = self.inner.next_set().await?;
        self.fields_dirty = true;
        Ok(has_next)
    }

    /// クエリ文字列に引数を埋め込む。
    pub fn mogrify(&mut self, query: &str, args: Option<&[Value]>) -> Result<String> {
        self.inner.mogrify(query, args)
    }

    /// 次の行を辞書形式で取得する。
    pub async fn fetch_one(&mut self) -> Result<Option<HashMap<String, Value>>> {
        self.ensure_fields();
        match self.inner.fetch_one().await? {
            None => Ok(None),
            Some(row) => {
                let mut dict = HashMap::new();
                for (field, value) in self.fields.iter().zip(row.iter()) {
                    dict.insert(field.clone(), value.clone());
                }
                Ok(Some(dict))
            }
        }
    }

    /// 複数行を辞書形式で取得する。
    pub async fn fetch_many(&mut self, size: Option<usize>) -> Result<Vec<HashMap<String, Value>>> {
        let mut result = Vec::new();
        let size = size.unwrap_or(self.inner.array_size);
        for _ in 0..size {
            match self.fetch_one().await? {
                Some(row) => result.push(row),
                None => break,
            }
        }
        Ok(result)
    }

    /// 全行を辞書形式で取得する。
    pub async fn fetch_all(&mut self) -> Result<Vec<HashMap<String, Value>>> {
        let mut result = Vec::new();
        while let Some(row) = self.fetch_one().await? {
            result.push(row);
        }
        Ok(result)
    }

    /// カーソル位置を移動する。
    pub async fn scroll(&mut self, value: isize, mode: &str) -> Result<()> {
        self.inner.scroll(value, mode).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_insert_values_re_allows_on_duplicate_key_update() {
        let re = insert_values_re();
        let query = "INSERT INTO t (a) VALUES (%s) ON DUPLICATE KEY UPDATE a=1";
        assert!(re.is_match(query));
    }

    #[test]
    fn test_insert_values_re_rejects_semicolon_in_postfix() {
        let re = insert_values_re();
        let query = "INSERT INTO t (a) VALUES (%s); DROP TABLE t; --";
        assert!(!re.is_match(query));
    }

    #[test]
    fn test_insert_values_re_allows_empty_postfix() {
        let re = insert_values_re();
        let query = "INSERT INTO t (a) VALUES (%s)";
        assert!(re.is_match(query));
    }

    #[test]
    fn test_insert_values_re_matches_multiline() {
        let re = insert_values_re();
        let query = "INSERT INTO t (a)\nVALUES (%s)";
        assert!(re.is_match(query));
    }

    #[tokio::test]
    async fn test_execute_many_first_row_exceeds_max_len() {
        use shiguredo_container::core::IntoContainerPort;
        use shiguredo_container::{AsyncRunner, GenericImage, ImageExt, WaitFor};
        use std::time::Duration;

        let version = std::env::var("MYSQL_VERSION").unwrap_or_else(|_| "8.4".to_string());
        let node = GenericImage::new("mysql", &version)
            .with_exposed_port(3306.tcp())
            .with_ready_conditions(vec![
                WaitFor::message_on_either_std("X Plugin ready for connections. Bind-address"),
                WaitFor::message_on_either_std("/usr/sbin/mysqld: ready for connections."),
            ])
            .with_env_var("MYSQL_DATABASE", "test")
            .with_env_var("MYSQL_ALLOW_EMPTY_PASSWORD", "yes")
            .start()
            .await
            .expect("MySQL コンテナの起動に失敗しました");
        let host = node
            .get_host()
            .await
            .expect("コンテナのホスト取得に失敗しました")
            .to_string();
        let port = node
            .get_host_port_ipv4(3306)
            .await
            .expect("コンテナのポート取得に失敗しました");

        let options = crate::connection::ConnectOptions {
            host,
            port,
            user: "root".to_string(),
            password: Vec::new(),
            database: Some("test".to_string()),
            charset: "utf8mb4".to_string(),
            connect_timeout: Duration::from_secs(60),
            ssl_mode: crate::connection::SslMode::Disabled,
            ..Default::default()
        };

        let mut conn = crate::connection::Connection::connect(options)
            .await
            .expect("MySQL への接続に失敗しました");
        let mut cursor = Cursor::new(&mut conn);
        cursor
            .execute(
                "CREATE TABLE IF NOT EXISTS bulk_len_test (v LONGTEXT)",
                None,
            )
            .await
            .expect("テーブル作成に失敗しました");

        let long = "x".repeat(1024001);
        let result = cursor
            .execute_many(
                "INSERT INTO bulk_len_test (v) VALUES (%s)",
                &[vec![Value::String(long)]],
            )
            .await;
        assert!(
            result.is_err(),
            "最初の行だけで max_len を超える場合はエラーにする"
        );
    }
}
