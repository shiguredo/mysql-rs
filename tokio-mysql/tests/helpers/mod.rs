// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL / TiDB 統合テスト用のヘルパー。
#![allow(dead_code)]

use shiguredo_mysql::converters::Value;
use shiguredo_tokio_mysql::{ConnectOptions, Connection, Cursor, DictCursor, SslMode};
use std::time::Duration;
use testcontainers::core::{IntoContainerPort, WaitFor};
use testcontainers::runners::AsyncRunner;
use testcontainers::{ContainerAsync, GenericImage};
use testcontainers_modules::mysql::Mysql;

/// tracing subscriber を一度だけ初期化する。
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt::try_init();
}

/// コンテナ起動後の MySQL に接続するための接続オプションを組み立てる。
pub async fn build_mysql_options() -> (ConnectOptions, ContainerAsync<Mysql>) {
    let node = Mysql::default()
        .start()
        .await
        .expect("MySQL コンテナの起動に失敗しました");

    let host = node
        .get_host()
        .await
        .expect("コンテナのホスト取得に失敗しました");
    let port = node
        .get_host_port_ipv4(3306)
        .await
        .expect("コンテナのポート取得に失敗しました");

    let options = build_options(host.to_string(), port);
    (options, node)
}

/// コンテナ起動後の TiDB に接続するための接続オプションを組み立てる。
pub async fn build_tidb_options() -> (ConnectOptions, ContainerAsync<GenericImage>) {
    let node = GenericImage::new("pingcap/tidb", "v8.4.0")
        .with_exposed_port(4000.tcp())
        .with_wait_for(WaitFor::message_on_stdout(
            "server is running MySQL protocol",
        ))
        .start()
        .await
        .expect("TiDB コンテナの起動に失敗しました");

    let host = node
        .get_host()
        .await
        .expect("コンテナのホスト取得に失敗しました");
    let port = node
        .get_host_port_ipv4(4000)
        .await
        .expect("コンテナのポート取得に失敗しました");

    let options = build_options(host.to_string(), port);
    (options, node)
}

/// ホストとポートから接続オプションを組み立てる。
fn build_options(host: String, port: u16) -> ConnectOptions {
    ConnectOptions {
        host,
        port,
        user: "root".to_string(),
        password: Vec::new(),
        database: Some("test".to_string()),
        charset: "utf8mb4".to_string(),
        connect_timeout: Duration::from_secs(60),
        ssl_mode: SslMode::Disabled,
        ..Default::default()
    }
}

/// 圧縮を有効にした接続オプションを返す。
pub fn with_compress(mut options: ConnectOptions) -> ConnectOptions {
    options.compress = true;
    options
}

/// 圧縮が有効になっていることを検証する。
pub async fn assert_compression_enabled(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);
    cursor
        .execute("SHOW STATUS LIKE 'Compression'", None)
        .await
        .expect("SHOW STATUS の実行に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][1],
        Value::String("ON".to_string()),
        "圧縮が有効になっていません"
    );

    // 圧縮状態でも通常のクエリが動作することを確認する。
    cursor
        .execute("SELECT 1 + 1 AS result", None)
        .await
        .expect("クエリ実行に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(2), "1 + 1 の結果が一致しません");
}

/// 1 + 1 クエリを実行して結果を検証する。
pub async fn assert_select_one_plus_one(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);
    cursor
        .execute("SELECT 1 + 1 AS result", None)
        .await
        .expect("クエリ実行に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(2), "1 + 1 の結果が一致しません");
}

/// テーブル作成・挿入・選択を検証する。
pub async fn assert_create_insert_and_select(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    // テーブル作成
    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS users (id INT PRIMARY KEY AUTO_INCREMENT, name VARCHAR(255), age INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // データ挿入
    cursor
        .execute(
            "INSERT INTO users (name, age) VALUES (%s, %s)",
            Some(&[Value::String("alice".to_string()), Value::Int(30)]),
        )
        .await
        .expect("INSERT に失敗しました");

    // データ取得
    cursor
        .execute("SELECT id, name, age FROM users ORDER BY id", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][1],
        Value::String("alice".to_string()),
        "name が一致しません"
    );
    assert_eq!(rows[0][2], Value::Int(30), "age が一致しません");

    // 後片付け
    cursor
        .execute("DROP TABLE IF EXISTS users", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// DictCursor で結果を辞書形式で取得できることを検証する。
pub async fn assert_dict_cursor(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = DictCursor::new(Cursor::new(&mut conn));

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS dict_sample (id INT, value VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO dict_sample (id, value) VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String("hello".to_string())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT id, value FROM dict_sample", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    let row = &rows[0];
    assert_eq!(
        row.get("id"),
        Some(Value::Int(1)).as_ref(),
        "id が一致しません"
    );
    assert_eq!(
        row.get("value"),
        Some(Value::String("hello".to_string())).as_ref(),
        "value が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS dict_sample", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 複数のステートメントと型を検証する。
#[expect(
    clippy::approx_constant,
    reason = "test uses 3.14 as an arbitrary float value, not PI"
)]
pub async fn assert_multiple_statements_and_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS type_sample (\
                t_int INT, \
                t_float DOUBLE, \
                t_str VARCHAR(255), \
                t_date DATE, \
                t_datetime DATETIME(6)\
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO type_sample VALUES (%s, %s, %s, %s, %s)",
            Some(&[
                Value::Int(42),
                Value::Float(3.14),
                Value::String("rust".to_string()),
                Value::Date(chrono::NaiveDate::from_ymd_opt(2024, 6, 13).expect("valid date")),
                Value::DateTime(
                    chrono::NaiveDateTime::parse_from_str(
                        "2024-06-13 12:34:56.123456",
                        "%Y-%m-%d %H:%M:%S%.6f",
                    )
                    .expect("valid datetime"),
                ),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "SELECT t_int, t_float, t_str, t_date, t_datetime FROM type_sample",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(42), "INT の値が一致しません");
    assert_eq!(rows[0][1], Value::Float(3.14), "DOUBLE の値が一致しません");
    assert_eq!(
        rows[0][2],
        Value::String("rust".to_string()),
        "VARCHAR の値が一致しません"
    );
    assert_eq!(
        rows[0][3],
        Value::Date(chrono::NaiveDate::from_ymd_opt(2024, 6, 13).expect("valid date")),
        "DATE の値が一致しません"
    );
    assert_eq!(
        rows[0][4],
        Value::DateTime(
            chrono::NaiveDateTime::parse_from_str(
                "2024-06-13 12:34:56.123456",
                "%Y-%m-%d %H:%M:%S%.6f",
            )
            .expect("valid datetime")
        ),
        "DATETIME の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS type_sample", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// ストアドプロシージャ呼び出し（IN/OUT/INOUT）を検証する。
pub async fn assert_call_procedure(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("DROP PROCEDURE IF EXISTS call_proc_test", None)
        .await
        .expect("プロシージャ削除に失敗しました");

    cursor
        .execute(
            "CREATE PROCEDURE call_proc_test(IN a INT, OUT b INT, INOUT c INT) \
             SET b = a * 2, c = c + 1",
            None,
        )
        .await
        .expect("プロシージャ作成に失敗しました");

    let result = cursor
        .call_procedure(
            "call_proc_test",
            &[Value::Int(5), Value::Int(0), Value::Int(10)],
        )
        .await
        .expect("プロシージャ呼び出しに失敗しました");

    assert_eq!(result.len(), 3, "引数の数が一致しません");
    assert_eq!(result[0], Value::Int(5), "IN 引数が一致しません");
    assert_eq!(result[1], Value::Int(10), "OUT 引数が一致しません");
    assert_eq!(result[2], Value::Int(11), "INOUT 引数が一致しません");

    cursor
        .execute("DROP PROCEDURE IF EXISTS call_proc_test", None)
        .await
        .expect("プロシージャ削除に失敗しました");
}
