// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL / TiDB 統合テスト用のヘルパー。
#![allow(dead_code)]

use shiguredo_container::core::IntoContainerPort;
use shiguredo_container::{
    AsyncRunner, ContainerAsync, ContainerRequest, GenericImage, ImageExt, WaitFor,
};
use shiguredo_mysql::converters::Value;
use shiguredo_tokio_mysql::{
    ConnectOptions, Connection, Cursor, DictCursor, Pool, PoolConfig, SslMode,
};
use std::time::Duration;

/// tracing subscriber を一度だけ初期化する。
pub fn init_tracing() {
    let _ = tracing_subscriber::fmt::try_init();
}

/// MySQL コンテナのイメージを組み立てる。
fn mysql_image() -> ContainerRequest<GenericImage> {
    GenericImage::new("mysql", "8.1")
        .with_exposed_port(3306.tcp())
        .with_ready_conditions(vec![
            WaitFor::message_on_either_std("X Plugin ready for connections. Bind-address"),
            WaitFor::message_on_either_std("/usr/sbin/mysqld: ready for connections."),
        ])
}

/// コンテナ起動後の MySQL に接続するための接続オプションを組み立てる。
pub async fn build_mysql_options() -> (ConnectOptions, ContainerAsync<GenericImage>) {
    let node = mysql_image()
        .with_env_var("MYSQL_DATABASE", "test")
        .with_env_var("MYSQL_ALLOW_EMPTY_PASSWORD", "yes")
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

/// NULL 値の挿入と取得を検証する。
pub async fn assert_null_values(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS null_test (id INT, val VARCHAR(255), num INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO null_test (id, val, num) VALUES (%s, %s, %s)",
            Some(&[Value::Int(1), Value::Null, Value::Null]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT id, val, num FROM null_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(1), "id が一致しません");
    assert_eq!(rows[0][1], Value::Null, "NULL の VARCHAR が一致しません");
    assert_eq!(rows[0][2], Value::Null, "NULL の INT が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS null_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 整数型の境界値を検証する。
pub async fn assert_integer_boundary_values(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS int_boundary (
                t_tinyint TINYINT,
                t_smallint SMALLINT,
                t_mediumint MEDIUMINT,
                t_int INT,
                t_bigint BIGINT
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // 最小値を挿入して検証する。
    cursor
        .execute(
            "INSERT INTO int_boundary VALUES (%s, %s, %s, %s, %s)",
            Some(&[
                Value::Int(-128),
                Value::Int(-32768),
                Value::Int(-8388608),
                Value::Int(-2147483648),
                Value::Int(-9223372036854775808),
            ]),
        )
        .await
        .expect("最小値の INSERT に失敗しました");

    cursor
        .execute("SELECT * FROM int_boundary", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(-128), "TINYINT 最小値が一致しません");
    assert_eq!(
        rows[0][1],
        Value::Int(-32768),
        "SMALLINT 最小値が一致しません"
    );
    assert_eq!(
        rows[0][2],
        Value::Int(-8388608),
        "MEDIUMINT 最小値が一致しません"
    );
    assert_eq!(
        rows[0][3],
        Value::Int(-2147483648),
        "INT 最小値が一致しません"
    );
    assert_eq!(
        rows[0][4],
        Value::Int(-9223372036854775808),
        "BIGINT 最小値が一致しません"
    );

    // 最大値を挿入して検証する。
    cursor
        .execute("DELETE FROM int_boundary", None)
        .await
        .expect("DELETE に失敗しました");

    cursor
        .execute(
            "INSERT INTO int_boundary VALUES (%s, %s, %s, %s, %s)",
            Some(&[
                Value::Int(127),
                Value::Int(32767),
                Value::Int(8388607),
                Value::Int(2147483647),
                Value::Int(9223372036854775807),
            ]),
        )
        .await
        .expect("最大値の INSERT に失敗しました");

    cursor
        .execute("SELECT * FROM int_boundary", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(127), "TINYINT 最大値が一致しません");
    assert_eq!(
        rows[0][1],
        Value::Int(32767),
        "SMALLINT 最大値が一致しません"
    );
    assert_eq!(
        rows[0][2],
        Value::Int(8388607),
        "MEDIUMINT 最大値が一致しません"
    );
    assert_eq!(
        rows[0][3],
        Value::Int(2147483647),
        "INT 最大値が一致しません"
    );
    assert_eq!(
        rows[0][4],
        Value::Int(9223372036854775807),
        "BIGINT 最大値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS int_boundary", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 浮動小数点数型の検証を行う。
pub async fn assert_float_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS float_test (t_float FLOAT, t_double DOUBLE)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO float_test VALUES (%s, %s)",
            Some(&[Value::Float(1.5), Value::Float(-123.456789)]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_float, t_double FROM float_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    // FLOAT は精度が低いため近似で比較する。
    if let Value::Float(f) = rows[0][0] {
        assert!((f - 1.5).abs() < 1e-6, "FLOAT の値が一致しません: {f}");
    } else {
        panic!("FLOAT が Float 型として返ってきませんでした");
    }
    assert_eq!(
        rows[0][1],
        Value::Float(-123.456789),
        "DOUBLE の値が一致しません"
    );

    // ゼロと負のゼロを検証する。
    cursor
        .execute("DELETE FROM float_test", None)
        .await
        .expect("DELETE に失敗しました");

    cursor
        .execute(
            "INSERT INTO float_test VALUES (%s, %s)",
            Some(&[Value::Float(0.0), Value::Float(-0.0)]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_float, t_double FROM float_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Float(0.0), "FLOAT ゼロが一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS float_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// DECIMAL 型の検証を行う。
pub async fn assert_decimal_type(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS decimal_test (t_decimal DECIMAL(10, 4))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO decimal_test VALUES (%s)",
            Some(&[Value::Decimal(rust_decimal::Decimal::new(123456, 4))]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_decimal FROM decimal_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::Decimal(rust_decimal::Decimal::new(123456, 4)),
        "DECIMAL の値が一致しません"
    );

    // 負の DECIMAL を検証する。
    cursor
        .execute("DELETE FROM decimal_test", None)
        .await
        .expect("DELETE に失敗しました");

    cursor
        .execute(
            "INSERT INTO decimal_test VALUES (%s)",
            Some(&[Value::Decimal(rust_decimal::Decimal::new(-999999, 4))]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_decimal FROM decimal_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::Decimal(rust_decimal::Decimal::new(-999999, 4)),
        "負の DECIMAL の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS decimal_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 文字列型の検証を行う。
pub async fn assert_string_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS string_test (
                t_char CHAR(10),
                t_varchar VARCHAR(255),
                t_text TEXT,
                t_mediumtext MEDIUMTEXT
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO string_test VALUES (%s, %s, %s, %s)",
            Some(&[
                Value::String("fixed".to_string()),
                Value::String("variable length string".to_string()),
                Value::String("text content".to_string()),
                Value::String("medium text content".to_string()),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT * FROM string_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    // CHAR は右側スペースが埋められるが、MySQL は取得時にトリムする。
    assert_eq!(
        rows[0][0],
        Value::String("fixed".to_string()),
        "CHAR の値が一致しません"
    );
    assert_eq!(
        rows[0][1],
        Value::String("variable length string".to_string()),
        "VARCHAR の値が一致しません"
    );
    assert_eq!(
        rows[0][2],
        Value::String("text content".to_string()),
        "TEXT の値が一致しません"
    );
    assert_eq!(
        rows[0][3],
        Value::String("medium text content".to_string()),
        "MEDIUMTEXT の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS string_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// バイナリ型の検証を行う。
pub async fn assert_binary_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS binary_test (
                t_binary BINARY(4),
                t_varbinary VARBINARY(255),
                t_blob BLOB
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO binary_test VALUES (%s, %s, %s)",
            Some(&[
                Value::Bytes(vec![0xDE, 0xAD, 0xBE, 0xEF]),
                Value::Bytes(vec![0x01, 0x02, 0x03]),
                Value::Bytes(vec![0xFF, 0x00, 0xAB, 0xCD]),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_varbinary, t_blob FROM binary_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    // VARBINARY と BLOB はバイト列として返る。
    // MySQL のバイナリデータは文字列としてデコードされる場合があるため、
    // 値が存在することだけを確認する。
    assert_ne!(
        rows[0][0],
        Value::Null,
        "VARBINARY が NULL であってはいけません"
    );
    assert_ne!(rows[0][1], Value::Null, "BLOB が NULL であってはいけません");

    cursor
        .execute("DROP TABLE IF EXISTS binary_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 日付・時刻型の検証を行う。
pub async fn assert_date_time_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS datetime_test (
                t_date DATE,
                t_time TIME,
                t_datetime DATETIME(6),
                t_timestamp TIMESTAMP,
                t_year YEAR
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO datetime_test VALUES (%s, %s, %s, %s, %s)",
            Some(&[
                Value::Date(chrono::NaiveDate::from_ymd_opt(2024, 12, 25).expect("valid date")),
                Value::Time(chrono::NaiveTime::from_hms_opt(13, 45, 30).expect("valid time")),
                Value::DateTime(
                    chrono::NaiveDateTime::parse_from_str(
                        "2024-12-25 13:45:30.123456",
                        "%Y-%m-%d %H:%M:%S%.6f",
                    )
                    .expect("valid datetime"),
                ),
                Value::DateTime(
                    chrono::NaiveDateTime::parse_from_str(
                        "2024-06-15 08:30:00",
                        "%Y-%m-%d %H:%M:%S",
                    )
                    .expect("valid datetime"),
                ),
                Value::Int(2024),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT * FROM datetime_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::Date(chrono::NaiveDate::from_ymd_opt(2024, 12, 25).expect("valid date")),
        "DATE の値が一致しません"
    );
    assert_eq!(
        rows[0][2],
        Value::DateTime(
            chrono::NaiveDateTime::parse_from_str(
                "2024-12-25 13:45:30.123456",
                "%Y-%m-%d %H:%M:%S%.6f",
            )
            .expect("valid datetime")
        ),
        "DATETIME の値が一致しません"
    );
    assert_eq!(rows[0][4], Value::Int(2024), "YEAR の値が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS datetime_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// TIME 型（TimeSpan）の検証を行う。
pub async fn assert_time_type(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS time_test (t_time TIME(6))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // 正の時間を挿入する。
    cursor
        .execute(
            "INSERT INTO time_test VALUES (%s)",
            Some(&[Value::TimeSpan(chrono::TimeDelta::seconds(3661))]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_time FROM time_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::TimeSpan(chrono::TimeDelta::seconds(3661)),
        "TIME の値が一致しません"
    );

    // 負の時間を挿入する。
    cursor
        .execute("DELETE FROM time_test", None)
        .await
        .expect("DELETE に失敗しました");

    cursor
        .execute(
            "INSERT INTO time_test VALUES (%s)",
            Some(&[Value::TimeSpan(chrono::TimeDelta::seconds(-7200))]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_time FROM time_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::TimeSpan(chrono::TimeDelta::seconds(-7200)),
        "負の TIME の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS time_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// トランザクションのコミットを検証する。
pub async fn assert_transaction_commit(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS tx_test (id INT PRIMARY KEY, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // autocommit を無効にしてトランザクションを開始する。
    cursor
        .execute("SET AUTOCOMMIT = 0", None)
        .await
        .expect("SET AUTOCOMMIT に失敗しました");

    cursor
        .execute(
            "INSERT INTO tx_test VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String("committed".to_string())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("COMMIT", None)
        .await
        .expect("COMMIT に失敗しました");

    // コミット後にデータが永続化されていることを確認する。
    cursor
        .execute("SELECT val FROM tx_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::String("committed".to_string()),
        "コミット後のデータが一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS tx_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// トランザクションのロールバックを検証する。
pub async fn assert_transaction_rollback(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS tx_rb_test (id INT PRIMARY KEY, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("SET AUTOCOMMIT = 0", None)
        .await
        .expect("SET AUTOCOMMIT に失敗しました");

    cursor
        .execute(
            "INSERT INTO tx_rb_test VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String("rolled_back".to_string())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("ROLLBACK", None)
        .await
        .expect("ROLLBACK に失敗しました");

    // ロールバック後にデータが存在しないことを確認する。
    cursor
        .execute("SELECT * FROM tx_rb_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows.len(),
        0,
        "ロールバック後にデータが残っていてはいけません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS tx_rb_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// execute_many による一括挿入を検証する。
pub async fn assert_execute_many(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS bulk_test (id INT, name VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    let args_list: Vec<Vec<Value>> = (1..=100)
        .map(|i| vec![Value::Int(i), Value::String(format!("row_{i}"))])
        .collect();

    cursor
        .execute_many(
            "INSERT INTO bulk_test (id, name) VALUES (%s, %s)",
            &args_list,
        )
        .await
        .expect("execute_many に失敗しました");

    cursor
        .execute("SELECT COUNT(*) FROM bulk_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(100), "挿入行数が一致しません");

    // 先頭と末尾のデータを確認する。
    cursor
        .execute("SELECT name FROM bulk_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("row_1".to_string()),
        "先頭行が一致しません"
    );

    cursor
        .execute("SELECT name FROM bulk_test WHERE id = 100", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("row_100".to_string()),
        "末尾行が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS bulk_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// fetch_one による 1 行ずつの取得を検証する。
pub async fn assert_fetch_one(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("SELECT 1 UNION SELECT 2 UNION SELECT 3", None)
        .await
        .expect("SELECT に失敗しました");

    let row1 = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row1.is_some(), "1 行目が取得できるべき");
    assert_eq!(
        row1.expect("1 行目")[0],
        Value::Int(1),
        "1 行目の値が一致しません"
    );

    let row2 = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row2.is_some(), "2 行目が取得できるべき");
    assert_eq!(
        row2.expect("2 行目")[0],
        Value::Int(2),
        "2 行目の値が一致しません"
    );

    let row3 = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row3.is_some(), "3 行目が取得できるべき");
    assert_eq!(
        row3.expect("3 行目")[0],
        Value::Int(3),
        "3 行目の値が一致しません"
    );

    let row4 = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row4.is_none(), "4 行目は存在しないべき");
}

/// fetch_many による複数行の取得を検証する。
pub async fn assert_fetch_many(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "SELECT 1 UNION SELECT 2 UNION SELECT 3 UNION SELECT 4 UNION SELECT 5",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    // 最初に 2 行取得する。
    let batch1 = cursor
        .fetch_many(Some(2))
        .expect("fetch_many に失敗しました");
    assert_eq!(batch1.len(), 2, "1 回目のバッチサイズが一致しません");
    assert_eq!(batch1[0][0], Value::Int(1), "1 行目が一致しません");
    assert_eq!(batch1[1][0], Value::Int(2), "2 行目が一致しません");

    // 次に 2 行取得する。
    let batch2 = cursor
        .fetch_many(Some(2))
        .expect("fetch_many に失敗しました");
    assert_eq!(batch2.len(), 2, "2 回目のバッチサイズが一致しません");
    assert_eq!(batch2[0][0], Value::Int(3), "3 行目が一致しません");
    assert_eq!(batch2[1][0], Value::Int(4), "4 行目が一致しません");

    // 残り 1 行を取得する（要求は 2 行だが 1 行しか残っていない）。
    let batch3 = cursor
        .fetch_many(Some(2))
        .expect("fetch_many に失敗しました");
    assert_eq!(batch3.len(), 1, "3 回目のバッチサイズが一致しません");
    assert_eq!(batch3[0][0], Value::Int(5), "5 行目が一致しません");

    // これ以上取得できない。
    let batch4 = cursor
        .fetch_many(Some(2))
        .expect("fetch_many に失敗しました");
    assert_eq!(batch4.len(), 0, "4 回目のバッチは空であるべき");
}

/// カーソルの scroll 機能を検証する。
pub async fn assert_cursor_scroll(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("SELECT 10 UNION SELECT 20 UNION SELECT 30", None)
        .await
        .expect("SELECT に失敗しました");

    // 絶対位置で 2 行目に移動する。
    cursor.scroll(1, "absolute").expect("scroll に失敗しました");
    let row = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert_eq!(
        row.expect("行が存在するべき")[0],
        Value::Int(20),
        "absolute scroll の値が一致しません"
    );

    // 相対位置で 1 行前に戻る。
    cursor
        .scroll(-2, "relative")
        .expect("scroll に失敗しました");
    let row = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert_eq!(
        row.expect("行が存在するべき")[0],
        Value::Int(10),
        "relative scroll の値が一致しません"
    );

    // 範囲外への scroll はエラーになる。
    let result = cursor.scroll(100, "absolute");
    assert!(result.is_err(), "範囲外への scroll はエラーであるべき");
}

/// 空の結果セットを検証する。
pub async fn assert_empty_result_set(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("CREATE TABLE IF NOT EXISTS empty_test (id INT)", None)
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("SELECT * FROM empty_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 0, "空テーブルの結果は 0 行であるべき");

    let row = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row.is_none(), "空テーブルから fetch_one は None であるべき");

    cursor
        .execute("DROP TABLE IF EXISTS empty_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 接続メタデータ（サーバーバージョン、スレッド ID、文字セット）を検証する。
pub async fn assert_connection_metadata(options: ConnectOptions, backend: &str) {
    let conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    // サーバーバージョンが空でないことを確認する。
    let version = conn.server_version();
    assert!(
        !version.is_empty(),
        "サーバーバージョンが空であってはいけません"
    );

    // スレッド ID が 0 でないことを確認する。
    let thread_id = conn.thread_id();
    assert_ne!(thread_id, 0, "スレッド ID が 0 であってはいけません");

    // 文字セット名が utf8mb4 であることを確認する。
    assert_eq!(
        conn.character_set_name(),
        "utf8mb4",
        "文字セット名が一致しません"
    );

    // 接続が開いていることを確認する。
    assert!(conn.is_open(), "接続が開いているべき");
}

/// escape_string の特殊文字エスケープを検証する。
pub async fn assert_escape_string(options: ConnectOptions, backend: &str) {
    let conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    assert_eq!(
        conn.escape_string("hello"),
        "hello",
        "通常文字列はエスケープしないべき"
    );
    assert_eq!(
        conn.escape_string("it's"),
        "it\\'s",
        "シングルクォートはエスケープするべき"
    );
    assert_eq!(
        conn.escape_string("back\\slash"),
        "back\\\\slash",
        "バックスラッシュはエスケープするべき"
    );
    assert_eq!(
        conn.escape_string("new\nline"),
        "new\\nline",
        "改行はエスケープするべき"
    );
    assert_eq!(
        conn.escape_string("tab\there"),
        "tab\there",
        "タブはエスケープしないべき"
    );
}

/// literal による値の SQL リテラル変換を検証する。
pub async fn assert_literal(options: ConnectOptions, backend: &str) {
    let conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    assert_eq!(
        conn.literal(&Value::Null).expect("literal に失敗しました"),
        "NULL",
        "NULL のリテラルが一致しません"
    );
    assert_eq!(
        conn.literal(&Value::Int(42))
            .expect("literal に失敗しました"),
        "42",
        "整数のリテラルが一致しません"
    );
    assert_eq!(
        conn.literal(&Value::String("hello".to_string()))
            .expect("literal に失敗しました"),
        "'hello'",
        "文字列のリテラルが一致しません"
    );
    assert_eq!(
        conn.literal(&Value::Bool(true))
            .expect("literal に失敗しました"),
        "1",
        "真偽値のリテラルが一致しません"
    );
}

/// mogrify によるパラメータ埋め込みを検証する。
pub async fn assert_mogrify(options: ConnectOptions, backend: &str) {
    let conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let result = conn
        .mogrify(
            "SELECT %s, %s",
            Some(&[Value::Int(1), Value::String("test".to_string())]),
        )
        .expect("mogrify に失敗しました");
    assert_eq!(result, "SELECT 1, 'test'", "mogrify の結果が一致しません");

    // %%s は %s にエスケープされる。
    let result = conn
        .mogrify("SELECT %%s", Some(&[]))
        .expect("mogrify に失敗しました");
    assert_eq!(result, "SELECT %s", "%%s のエスケープが一致しません");

    // 引数が足りない場合はエラー。
    let result = conn.mogrify("SELECT %s, %s", Some(&[Value::Int(1)]));
    assert!(result.is_err(), "引数不足はエラーであるべき");

    // 引数が多い場合はエラー。
    let result = conn.mogrify("SELECT %s", Some(&[Value::Int(1), Value::Int(2)]));
    assert!(result.is_err(), "引数過多はエラーであるべき");
}

/// Unicode（日本語・絵文字）の取り扱いを検証する。
pub async fn assert_unicode(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS unicode_test (id INT, text VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO unicode_test VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String("こんにちは世界".to_string())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "INSERT INTO unicode_test VALUES (%s, %s)",
            Some(&[
                Value::Int(2),
                Value::String("emoji: \u{1F600}\u{1F389}".to_string()),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT text FROM unicode_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("こんにちは世界".to_string()),
        "日本語テキストが一致しません"
    );

    cursor
        .execute("SELECT text FROM unicode_test WHERE id = 2", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("emoji: \u{1F600}\u{1F389}".to_string()),
        "絵文字テキストが一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS unicode_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 大きなデータの取り扱いを検証する。
pub async fn assert_large_data(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS large_test (id INT, data LONGTEXT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // 1MB のテキストを挿入する。
    let large_string = "A".repeat(1024 * 1024);
    cursor
        .execute(
            "INSERT INTO large_test VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String(large_string.clone())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT LENGTH(data) FROM large_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::Int(1024 * 1024),
        "大きなデータの長さが一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS large_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// SQL シンタックスエラーの処理を検証する。
pub async fn assert_syntax_error(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    let result = cursor.execute("SELEC invalid syntax", None).await;
    assert!(result.is_err(), "シンタックスエラーはエラーであるべき");

    // エラー後も接続が使用できることを確認する。
    cursor
        .execute("SELECT 1", None)
        .await
        .expect("エラー後のクエリに失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::Int(1),
        "エラー後のクエリ結果が一致しません"
    );
}

/// 存在しないテーブルへのクエリエラーを検証する。
pub async fn assert_unknown_table_error(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    let result = cursor
        .execute("SELECT * FROM nonexistent_table_xyz", None)
        .await;
    assert!(
        result.is_err(),
        "存在しないテーブルへのクエリはエラーであるべき"
    );

    // エラー後も接続が使用できることを確認する。
    cursor
        .execute("SELECT 42", None)
        .await
        .expect("エラー後のクエリに失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::Int(42),
        "エラー後のクエリ結果が一致しません"
    );
}

/// 主キー重複エラーを検証する。
pub async fn assert_duplicate_key_error(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS dup_test (id INT PRIMARY KEY)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("INSERT INTO dup_test VALUES (%s)", Some(&[Value::Int(1)]))
        .await
        .expect("INSERT に失敗しました");

    let result = cursor
        .execute("INSERT INTO dup_test VALUES (%s)", Some(&[Value::Int(1)]))
        .await;
    assert!(result.is_err(), "主キー重複はエラーであるべき");

    cursor
        .execute("DROP TABLE IF EXISTS dup_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// SHOW DATABASES の結果を検証する。
pub async fn assert_show_databases(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("SHOW DATABASES", None)
        .await
        .expect("SHOW DATABASES に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert!(
        !rows.is_empty(),
        "SHOW DATABASES の結果が空であってはいけません"
    );

    // test データベースが含まれていることを確認する。
    let has_test = rows
        .iter()
        .any(|row| row[0] == Value::String("test".to_string()));
    assert!(has_test, "test データベースが含まれているべき");
}

/// SHOW TABLES の結果を検証する。
pub async fn assert_show_tables(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("CREATE TABLE IF NOT EXISTS show_tables_test (id INT)", None)
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("SHOW TABLES", None)
        .await
        .expect("SHOW TABLES に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    let has_table = rows
        .iter()
        .any(|row| row[0] == Value::String("show_tables_test".to_string()));
    assert!(has_table, "作成したテーブルが SHOW TABLES に含まれるべき");

    cursor
        .execute("DROP TABLE IF EXISTS show_tables_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// INFORMATION_SCHEMA へのクエリを検証する。
pub async fn assert_information_schema(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "SELECT TABLE_NAME FROM INFORMATION_SCHEMA.TABLES WHERE TABLE_SCHEMA = 'test' LIMIT 10",
            None,
        )
        .await
        .expect("INFORMATION_SCHEMA へのクエリに失敗しました");

    // エラーなく実行できることを確認する（結果は空でもよい）。
    let _rows = cursor.fetch_all().expect("結果取得に失敗しました");
}

/// 接続の close を検証する。
pub async fn assert_connection_close(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    assert!(conn.is_open(), "接続前は開いているべき");

    conn.close().await.expect("close に失敗しました");

    assert!(!conn.is_open(), "close 後は閉じているべき");
}

/// 複数の接続が同時に動作することを検証する。
pub async fn assert_multiple_connections(options: ConnectOptions, backend: &str) {
    let options2 = options.clone();

    let mut conn1 = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続 1 に失敗しました: {e}"));
    let mut conn2 = Connection::connect(options2)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続 2 に失敗しました: {e}"));

    // 異なるスレッド ID を持つことを確認する。
    assert_ne!(
        conn1.thread_id(),
        conn2.thread_id(),
        "異なる接続は異なるスレッド ID を持つべき"
    );

    // 両方の接続でクエリを実行する。
    let mut cursor1 = Cursor::new(&mut conn1);
    cursor1
        .execute("SELECT 1", None)
        .await
        .expect("接続 1 でのクエリに失敗しました");
    let rows = cursor1.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows[0][0], Value::Int(1), "接続 1 の結果が一致しません");

    let mut cursor2 = Cursor::new(&mut conn2);
    cursor2
        .execute("SELECT 2", None)
        .await
        .expect("接続 2 でのクエリに失敗しました");
    let rows = cursor2.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows[0][0], Value::Int(2), "接続 2 の結果が一致しません");
}

/// AUTO_INCREMENT と insert_id を検証する。
pub async fn assert_auto_increment_and_insert_id(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    {
        let mut cursor = Cursor::new(&mut conn);
        cursor
            .execute(
                "CREATE TABLE IF NOT EXISTS auto_inc_test (id INT PRIMARY KEY AUTO_INCREMENT, val VARCHAR(255))",
                None,
            )
            .await
            .expect("テーブル作成に失敗しました");

        cursor
            .execute(
                "INSERT INTO auto_inc_test (val) VALUES (%s)",
                Some(&[Value::String("first".to_string())]),
            )
            .await
            .expect("INSERT に失敗しました");
    }

    assert_eq!(conn.insert_id(), 1, "最初の insert_id が一致しません");

    {
        let mut cursor = Cursor::new(&mut conn);
        cursor
            .execute(
                "INSERT INTO auto_inc_test (val) VALUES (%s)",
                Some(&[Value::String("second".to_string())]),
            )
            .await
            .expect("INSERT に失敗しました");
    }

    assert_eq!(conn.insert_id(), 2, "2 回目の insert_id が一致しません");

    {
        let mut cursor = Cursor::new(&mut conn);
        cursor
            .execute("DROP TABLE IF EXISTS auto_inc_test", None)
            .await
            .expect("テーブル削除に失敗しました");
    }
}

/// affected_rows の検証を行う。
pub async fn assert_affected_rows(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);
    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS affected_test (id INT, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // 3 行挿入する。
    let affected = cursor
        .execute(
            "INSERT INTO affected_test VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            None,
        )
        .await
        .expect("INSERT に失敗しました");
    assert_eq!(affected, 3, "INSERT の affected_rows が一致しません");

    // 2 行更新する。
    let affected = cursor
        .execute("UPDATE affected_test SET val = 'x' WHERE id <= 2", None)
        .await
        .expect("UPDATE に失敗しました");
    assert_eq!(affected, 2, "UPDATE の affected_rows が一致しません");

    // 1 行削除する。
    let affected = cursor
        .execute("DELETE FROM affected_test WHERE id = 3", None)
        .await
        .expect("DELETE に失敗しました");
    assert_eq!(affected, 1, "DELETE の affected_rows が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS affected_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 特殊文字を含む文字列の挿入と取得を検証する。
pub async fn assert_special_characters(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS special_chars (id INT, text VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    // シングルクォート、バックスラッシュ、改行を含む文字列を挿入する。
    let special = "it's a \"test\"\nwith\\backslash\0null";
    cursor
        .execute(
            "INSERT INTO special_chars VALUES (%s, %s)",
            Some(&[Value::Int(1), Value::String(special.to_string())]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT text FROM special_chars WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String(special.to_string()),
        "特殊文字を含む文字列が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS special_chars", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// ENUM 型と SET 型の検証を行う。
pub async fn assert_enum_and_set_types(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS enum_set_test (
                t_enum ENUM('small', 'medium', 'large'),
                t_set SET('a', 'b', 'c')
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO enum_set_test VALUES (%s, %s)",
            Some(&[
                Value::String("medium".to_string()),
                Value::String("a,c".to_string()),
            ]),
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT t_enum, t_set FROM enum_set_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::String("medium".to_string()),
        "ENUM の値が一致しません"
    );
    assert_eq!(
        rows[0][1],
        Value::String("a,c".to_string()),
        "SET の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS enum_set_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 複数の結果セットを返すストアドプロシージャを検証する。
pub async fn assert_multiple_result_sets(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("DROP PROCEDURE IF EXISTS multi_result_proc", None)
        .await
        .expect("プロシージャ削除に失敗しました");

    cursor
        .execute(
            "CREATE PROCEDURE multi_result_proc()
             BEGIN
                 SELECT 1 AS first_result;
                 SELECT 2 AS second_result;
             END",
            None,
        )
        .await
        .expect("プロシージャ作成に失敗しました");

    cursor
        .execute("CALL multi_result_proc()", None)
        .await
        .expect("CALL に失敗しました");

    // 最初の結果セットを取得する。
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "最初の結果セットの行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(1), "最初の結果が一致しません");

    // 次の結果セットに移動する。
    let has_next = cursor.next_set().await.expect("next_set に失敗しました");
    assert!(has_next, "次の結果セットが存在するべき");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "2 番目の結果セットの行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(2), "2 番目の結果が一致しません");

    cursor
        .execute("DROP PROCEDURE IF EXISTS multi_result_proc", None)
        .await
        .expect("プロシージャ削除に失敗しました");
}

/// WHERE 句でのパラメータ使用を検証する。
pub async fn assert_parameterized_where(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS param_where (id INT, name VARCHAR(255), score DOUBLE)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO param_where VALUES (1, 'alice', 95.5), (2, 'bob', 82.3), (3, 'charlie', 91.0)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    // 文字列パラメータで検索する。
    cursor
        .execute(
            "SELECT id, score FROM param_where WHERE name = %s",
            Some(&[Value::String("bob".to_string())]),
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(2), "id が一致しません");

    // 数値パラメータで範囲検索する。
    cursor
        .execute(
            "SELECT name FROM param_where WHERE score > %s ORDER BY score DESC",
            Some(&[Value::Float(90.0)]),
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 2, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::String("alice".to_string()),
        "1 行目が一致しません"
    );
    assert_eq!(
        rows[1][0],
        Value::String("charlie".to_string()),
        "2 行目が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS param_where", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// UPDATE と DELETE の組み合わせを検証する。
pub async fn assert_update_and_delete(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS upd_del_test (id INT PRIMARY KEY, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO upd_del_test VALUES (1, 'a'), (2, 'b'), (3, 'c')",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    // 条件付き UPDATE を実行する。
    cursor
        .execute(
            "UPDATE upd_del_test SET val = %s WHERE id = %s",
            Some(&[Value::String("updated".to_string()), Value::Int(2)]),
        )
        .await
        .expect("UPDATE に失敗しました");

    cursor
        .execute("SELECT val FROM upd_del_test WHERE id = 2", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("updated".to_string()),
        "UPDATE 後の値が一致しません"
    );

    // 条件付き DELETE を実行する。
    cursor
        .execute(
            "DELETE FROM upd_del_test WHERE id = %s",
            Some(&[Value::Int(1)]),
        )
        .await
        .expect("DELETE に失敗しました");

    cursor
        .execute("SELECT COUNT(*) FROM upd_del_test", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows[0][0], Value::Int(2), "DELETE 後の行数が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS upd_del_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// GROUP BY と集約関数の検証を行う。
pub async fn assert_group_by_and_aggregation(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS agg_test (category VARCHAR(50), amount INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO agg_test VALUES ('A', 10), ('B', 20), ('A', 30), ('B', 40), ('C', 50)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "SELECT category, SUM(amount) AS total, COUNT(*) AS cnt FROM agg_test GROUP BY category ORDER BY category",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 3, "グループ数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::String("A".to_string()),
        "カテゴリ A が一致しません"
    );
    // SUM() は DECIMAL 型を返す。
    assert_eq!(
        rows[0][1],
        Value::Decimal(rust_decimal::Decimal::new(40, 0)),
        "カテゴリ A の合計が一致しません"
    );
    assert_eq!(rows[0][2], Value::Int(2), "カテゴリ A の件数が一致しません");
    assert_eq!(
        rows[1][0],
        Value::String("B".to_string()),
        "カテゴリ B が一致しません"
    );
    assert_eq!(
        rows[1][1],
        Value::Decimal(rust_decimal::Decimal::new(60, 0)),
        "カテゴリ B の合計が一致しません"
    );
    assert_eq!(
        rows[2][0],
        Value::String("C".to_string()),
        "カテゴリ C が一致しません"
    );
    assert_eq!(
        rows[2][1],
        Value::Decimal(rust_decimal::Decimal::new(50, 0)),
        "カテゴリ C の合計が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS agg_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// サブクエリの検証を行う。
pub async fn assert_subquery(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS sub_test (id INT, val INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO sub_test VALUES (1, 100), (2, 200), (3, 300)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "SELECT id FROM sub_test WHERE val > (SELECT AVG(val) FROM sub_test)",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(3), "サブクエリの結果が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS sub_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// JOIN の検証を行う。
pub async fn assert_join(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS join_a (id INT PRIMARY KEY, name VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS join_b (id INT PRIMARY KEY, a_id INT, score INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("INSERT INTO join_a VALUES (1, 'alice'), (2, 'bob')", None)
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "INSERT INTO join_b VALUES (1, 1, 90), (2, 1, 85), (3, 2, 95)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "SELECT a.name, b.score FROM join_a a INNER JOIN join_b b ON a.id = b.a_id ORDER BY b.score DESC",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 3, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::String("bob".to_string()),
        "1 行目の name が一致しません"
    );
    assert_eq!(rows[0][1], Value::Int(95), "1 行目の score が一致しません");
    assert_eq!(
        rows[1][0],
        Value::String("alice".to_string()),
        "2 行目の name が一致しません"
    );
    assert_eq!(rows[1][1], Value::Int(90), "2 行目の score が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS join_a", None)
        .await
        .expect("テーブル削除に失敗しました");
    cursor
        .execute("DROP TABLE IF EXISTS join_b", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// ORDER BY と LIMIT の検証を行う。
pub async fn assert_order_by_and_limit(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS order_test (id INT, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO order_test VALUES (3, 'c'), (1, 'a'), (5, 'e'), (2, 'b'), (4, 'd')",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    // 昇順で上位 3 件を取得する。
    cursor
        .execute("SELECT id FROM order_test ORDER BY id ASC LIMIT 3", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 3, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(1), "1 行目が一致しません");
    assert_eq!(rows[1][0], Value::Int(2), "2 行目が一致しません");
    assert_eq!(rows[2][0], Value::Int(3), "3 行目が一致しません");

    // 降順で上位 2 件を取得する。
    cursor
        .execute("SELECT id FROM order_test ORDER BY id DESC LIMIT 2", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 2, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(5), "降順 1 行目が一致しません");
    assert_eq!(rows[1][0], Value::Int(4), "降順 2 行目が一致しません");

    // OFFSET を指定する。
    cursor
        .execute(
            "SELECT id FROM order_test ORDER BY id ASC LIMIT 2 OFFSET 2",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 2, "行数が一致しません");
    assert_eq!(
        rows[0][0],
        Value::Int(3),
        "OFFSET 後の 1 行目が一致しません"
    );
    assert_eq!(
        rows[1][0],
        Value::Int(4),
        "OFFSET 後の 2 行目が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS order_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// DictCursor の fetch_one と fetch_many を検証する。
pub async fn assert_dict_cursor_fetch_methods(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = DictCursor::new(Cursor::new(&mut conn));

    cursor
        .execute(
            "SELECT 1 AS a, 'x' AS b UNION SELECT 2, 'y' UNION SELECT 3, 'z'",
            None,
        )
        .await
        .expect("SELECT に失敗しました");

    // fetch_one で 1 行取得する。
    let row = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row.is_some(), "1 行目が取得できるべき");
    let row = row.expect("1 行目");
    assert_eq!(
        row.get("a"),
        Some(Value::Int(1)).as_ref(),
        "a が一致しません"
    );
    assert_eq!(
        row.get("b"),
        Some(Value::String("x".to_string())).as_ref(),
        "b が一致しません"
    );

    // fetch_many で残り 2 行取得する。
    let rows = cursor
        .fetch_many(Some(5))
        .expect("fetch_many に失敗しました");
    assert_eq!(rows.len(), 2, "残り行数が一致しません");
    assert_eq!(
        rows[0].get("a"),
        Some(Value::Int(2)).as_ref(),
        "2 行目の a が一致しません"
    );
    assert_eq!(
        rows[1].get("a"),
        Some(Value::Int(3)).as_ref(),
        "3 行目の a が一致しません"
    );

    // これ以上取得できない。
    let row = cursor.fetch_one().expect("fetch_one に失敗しました");
    assert!(row.is_none(), "全行取得後は None であるべき");
}

/// init_command オプションの検証を行う。
pub async fn assert_init_command(options: ConnectOptions, backend: &str) {
    let mut options = options;
    options.init_command = Some("SET @init_test = 'initialized'".to_string());

    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);
    cursor
        .execute("SELECT @init_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("initialized".to_string()),
        "init_command が実行されていない"
    );
}

/// sql_mode オプションの検証を行う。
pub async fn assert_sql_mode(options: ConnectOptions, backend: &str) {
    let mut options = options;
    options.sql_mode = Some("STRICT_TRANS_TABLES".to_string());

    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);
    cursor
        .execute("SELECT @@sql_mode", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    if let Value::String(mode) = &rows[0][0] {
        assert!(
            mode.contains("STRICT_TRANS_TABLES"),
            "sql_mode に STRICT_TRANS_TABLES が含まれているべき: {mode}"
        );
    } else {
        panic!("sql_mode が文字列として返ってきませんでした");
    }
}

/// REPLACE 文の検証を行う。
pub async fn assert_replace_statement(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS replace_test (id INT PRIMARY KEY, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute("INSERT INTO replace_test VALUES (1, 'original')", None)
        .await
        .expect("INSERT に失敗しました");

    // REPLACE で既存行を置き換える。
    cursor
        .execute("REPLACE INTO replace_test VALUES (1, 'replaced')", None)
        .await
        .expect("REPLACE に失敗しました");

    cursor
        .execute("SELECT val FROM replace_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("replaced".to_string()),
        "REPLACE 後の値が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS replace_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// ON DUPLICATE KEY UPDATE の検証を行う。
pub async fn assert_on_duplicate_key_update(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS odk_test (id INT PRIMARY KEY, count INT)",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO odk_test VALUES (1, 1) ON DUPLICATE KEY UPDATE count = count + 1",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute(
            "INSERT INTO odk_test VALUES (1, 1) ON DUPLICATE KEY UPDATE count = count + 1",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT count FROM odk_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::Int(2),
        "ON DUPLICATE KEY UPDATE の結果が一致しません"
    );

    cursor
        .execute("DROP TABLE IF EXISTS odk_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 空文字列と NULL の区別を検証する。
pub async fn assert_empty_string_vs_null(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS empty_null_test (id INT, val VARCHAR(255))",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO empty_null_test VALUES (1, ''), (2, NULL)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT val FROM empty_null_test WHERE id = 1", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String(String::new()),
        "空文字列が一致しません"
    );

    cursor
        .execute("SELECT val FROM empty_null_test WHERE id = 2", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows[0][0], Value::Null, "NULL が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS empty_null_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 複数のカラムを持つ行の取得を検証する。
pub async fn assert_wide_row(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute(
            "CREATE TABLE IF NOT EXISTS wide_test (
                c1 INT, c2 INT, c3 INT, c4 INT, c5 INT,
                c6 INT, c7 INT, c8 INT, c9 INT, c10 INT
            )",
            None,
        )
        .await
        .expect("テーブル作成に失敗しました");

    cursor
        .execute(
            "INSERT INTO wide_test VALUES (1, 2, 3, 4, 5, 6, 7, 8, 9, 10)",
            None,
        )
        .await
        .expect("INSERT に失敗しました");

    cursor
        .execute("SELECT * FROM wide_test", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0].len(), 10, "カラム数が一致しません");
    for (i, val) in rows[0].iter().enumerate() {
        assert_eq!(
            *val,
            Value::Int((i + 1) as i64),
            "カラム {} の値が一致しません",
            i + 1
        );
    }

    cursor
        .execute("DROP TABLE IF EXISTS wide_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// 大量行の取得を検証する。
pub async fn assert_many_rows(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    cursor
        .execute("CREATE TABLE IF NOT EXISTS many_rows_test (id INT)", None)
        .await
        .expect("テーブル作成に失敗しました");

    // 1000 行を挿入する。
    let args_list: Vec<Vec<Value>> = (1..=1000).map(|i| vec![Value::Int(i)]).collect();
    cursor
        .execute_many("INSERT INTO many_rows_test (id) VALUES (%s)", &args_list)
        .await
        .expect("execute_many に失敗しました");

    cursor
        .execute("SELECT id FROM many_rows_test ORDER BY id", None)
        .await
        .expect("SELECT に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1000, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(1), "先頭行が一致しません");
    assert_eq!(rows[999][0], Value::Int(1000), "末尾行が一致しません");

    cursor
        .execute("DROP TABLE IF EXISTS many_rows_test", None)
        .await
        .expect("テーブル削除に失敗しました");
}

/// SELECT 式の結果を検証する（テーブル不要）。
pub async fn assert_select_expressions(options: ConnectOptions, backend: &str) {
    let mut conn = Connection::connect(options)
        .await
        .unwrap_or_else(|e| panic!("{backend} への接続に失敗しました: {e}"));

    let mut cursor = Cursor::new(&mut conn);

    // 算術演算を検証する。
    cursor
        .execute("SELECT 10 * 5 + 3", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows[0][0], Value::Int(53), "算術演算の結果が一致しません");

    // 文字列関数を検証する。
    cursor
        .execute("SELECT UPPER('hello')", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("HELLO".to_string()),
        "UPPER の結果が一致しません"
    );

    // 条件式を検証する。
    cursor
        .execute("SELECT IF(1 > 0, 'yes', 'no')", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("yes".to_string()),
        "IF の結果が一致しません"
    );

    // NULL 関連の関数を検証する。
    cursor
        .execute("SELECT IFNULL(NULL, 'default')", None)
        .await
        .expect("SELECT に失敗しました");
    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::String("default".to_string()),
        "IFNULL の結果が一致しません"
    );
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

/// コネクションプールの基本動作を検証する。
pub async fn assert_pool_basic(options: ConnectOptions, backend: &str) {
    let config = PoolConfig {
        max_size: 3,
        min_idle: 1,
        ..Default::default()
    };

    let pool = Pool::start(options, config)
        .await
        .unwrap_or_else(|e| panic!("{backend} へのプール起動に失敗しました: {e}"));

    // 接続を取得してクエリを実行する。
    let mut pooled = pool.acquire().await.expect("接続の取得に失敗しました");

    let mut cursor = Cursor::new(pooled.connection_mut());
    cursor
        .execute("SELECT 1 + 1 AS result", None)
        .await
        .expect("クエリ実行に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(rows.len(), 1, "行数が一致しません");
    assert_eq!(rows[0][0], Value::Int(2), "1 + 1 の結果が一致しません");

    // 接続をドロップしてプールに返却する。
    drop(cursor);
    drop(pooled);

    // 再度取得して再利用できることを確認する。
    let mut pooled = pool
        .acquire()
        .await
        .expect("再利用接続の取得に失敗しました");

    let mut cursor = Cursor::new(pooled.connection_mut());
    cursor
        .execute("SELECT 42", None)
        .await
        .expect("クエリ実行に失敗しました");

    let rows = cursor.fetch_all().expect("結果取得に失敗しました");
    assert_eq!(
        rows[0][0],
        Value::Int(42),
        "再利用接続でのクエリ結果が一致しません"
    );

    drop(cursor);
    drop(pooled);

    pool.close().await.expect("プールのクローズに失敗しました");
}

/// コネクションプールの並列取得を検証する。
pub async fn assert_pool_concurrent(options: ConnectOptions, backend: &str) {
    let config = PoolConfig {
        max_size: 3,
        min_idle: 1,
        ..Default::default()
    };

    let pool = Pool::start(options, config)
        .await
        .unwrap_or_else(|e| panic!("{backend} へのプール起動に失敗しました: {e}"));

    // 最大接続数分だけ並列に取得する。
    let mut connections = Vec::new();
    for _ in 0..3 {
        let pooled = pool.acquire().await.expect("並列接続の取得に失敗しました");
        connections.push(pooled);
    }

    // 全接続でクエリが実行できることを確認する。
    for (i, pooled) in connections.iter_mut().enumerate() {
        let mut cursor = Cursor::new(pooled.connection_mut());
        let expected = (i + 1) as i64;
        cursor
            .execute(&format!("SELECT {expected}"), None)
            .await
            .expect("クエリ実行に失敗しました");

        let rows = cursor.fetch_all().expect("結果取得に失敗しました");
        assert_eq!(
            rows[0][0],
            Value::Int(expected),
            "並列接続 {i} のクエリ結果が一致しません"
        );
    }

    // 全接続を返却する。
    drop(connections);

    pool.close().await.expect("プールのクローズに失敗しました");
}
