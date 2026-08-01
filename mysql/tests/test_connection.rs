// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! 実際の MySQL サーバーに対する結合テスト。
//!
//! PyMySQL にはあるが mysql-rs にない機能 (ping / kill / select_db /
//! show_warnings / トランザクション / アンバッファードカーソル /
//! LOAD DATA LOCAL INFILE / オプションファイル) を検証する。

use shiguredo_container::core::IntoContainerPort;
use shiguredo_container::{AsyncRunner, ContainerAsync, GenericImage, ImageExt, WaitFor};
use shiguredo_mysql::{ConnectOptions, Connection, SslMode, UnbufferedDictCursor};
use shiguredo_mysql_core::constants::field_type;
use shiguredo_mysql_core::converters::Value;
use std::time::Duration;

/// テスト用のログを初期化する。
fn init_tracing() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .try_init();
}

/// テスト対象の MySQL バージョンを取得する。
///
/// `MYSQL_VERSION` 環境変数で切り替えられる (例: `MYSQL_VERSION=26`)。
/// デフォルトは LTS の 8.4。
fn mysql_version() -> String {
    std::env::var("MYSQL_VERSION").unwrap_or_else(|_| "8.4".to_string())
}

/// MySQL 8.1 コンテナを起動する。
///
/// LOAD DATA LOCAL INFILE を検証するため、サーバー側の local_infile を有効にする。
async fn start_mysql() -> ContainerAsync<GenericImage> {
    GenericImage::new("mysql", &mysql_version())
        .with_exposed_port(3306.tcp())
        .with_cmd(["--local-infile=1"])
        .with_ready_conditions(vec![
            WaitFor::message_on_either_std("X Plugin ready for connections. Bind-address"),
            WaitFor::message_on_either_std("/usr/sbin/mysqld: ready for connections."),
        ])
        .with_env_var("MYSQL_DATABASE", "test")
        .with_env_var("MYSQL_ALLOW_EMPTY_PASSWORD", "yes")
        .start()
        .await
        .expect("MySQL コンテナの起動に失敗しました")
}

/// コンテナに対する接続オプションを作成する。
async fn connect_options(node: &ContainerAsync<GenericImage>) -> ConnectOptions {
    let host = node
        .get_host()
        .await
        .expect("コンテナのホスト取得に失敗しました")
        .to_string();
    let port = node
        .get_host_port_ipv4(3306)
        .await
        .expect("コンテナのポート取得に失敗しました");
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

/// コンテナに接続する。
async fn connect(node: &ContainerAsync<GenericImage>) -> Connection {
    Connection::connect(connect_options(node).await)
        .await
        .expect("MySQL への接続に失敗しました")
}

/// クエリを実行して結果の行を返す。
async fn query_rows(conn: &mut Connection, sql: &str) -> Vec<Vec<Value>> {
    conn.query(sql, false)
        .await
        .expect("クエリの実行に失敗しました");
    conn.result()
        .expect("結果セットがありません")
        .rows
        .clone()
        .unwrap_or_default()
}

/// ping / 接続情報 / select_db / show_warnings / register_converter / kill を検証する。
#[tokio::test]
async fn test_connection_management() {
    init_tracing();
    let node = start_mysql().await;
    let mut conn = connect(&node).await;

    // ping
    conn.ping().await.expect("ping に失敗しました");

    // 接続情報
    let host = node
        .get_host()
        .await
        .expect("コンテナのホスト取得に失敗しました")
        .to_string();
    let port = node
        .get_host_port_ipv4(3306)
        .await
        .expect("コンテナのポート取得に失敗しました");
    assert_eq!(
        conn.get_host_info(),
        format!("{}:{}", host, port),
        "get_host_info は host:port 形式であるべき"
    );
    assert_eq!(
        conn.get_proto_info(),
        10,
        "プロトコルバージョンは 10 であるべき"
    );
    assert!(conn.thread_id() > 0, "スレッド ID は 0 より大きいはず");
    assert!(
        conn.server_version().starts_with(&mysql_version()),
        "サーバーバージョンはテスト対象の {} 系であるべき",
        mysql_version()
    );

    // select_db: データベース未選択の接続から切り替える
    let mut conn2 = Connection::connect(ConnectOptions {
        database: None,
        ..connect_options(&node).await
    })
    .await
    .expect("データベース未選択での接続に失敗しました");
    let rows = query_rows(&mut conn2, "SELECT DATABASE()").await;
    assert_eq!(
        rows[0][0],
        Value::Null,
        "データベース未選択なら NULL になるべき"
    );
    conn2
        .select_db("test")
        .await
        .expect("select_db に失敗しました");
    let rows = query_rows(&mut conn2, "SELECT DATABASE()").await;
    assert_eq!(
        rows[0][0],
        Value::String("test".to_string()),
        "select_db 後にデータベースが切り替わるべき"
    );

    // show_warnings: 数値変換の切り詰めで警告を発生させる
    conn.query("SELECT CAST('abc' AS UNSIGNED)", false)
        .await
        .expect("クエリの実行に失敗しました");
    let warnings = conn
        .show_warnings()
        .await
        .expect("show_warnings に失敗しました");
    assert!(
        !warnings.is_empty(),
        "CAST の切り詰めは警告を発生させるべき"
    );
    assert_eq!(warnings[0].len(), 3, "SHOW WARNINGS は 3 カラムであるべき");

    // register_converter: LONGLONG のデコーダーを差し替える
    conn.register_converter(field_type::LONGLONG, |s| {
        Value::String(format!("custom:{}", s))
    });
    let rows = query_rows(&mut conn, "SELECT 42").await;
    assert_eq!(
        rows[0][0],
        Value::String("custom:42".to_string()),
        "登録したデコーダーが適用されるべき"
    );

    // kill: 別接続を終了させる
    let mut victim = connect(&node).await;
    let target = victim.thread_id();
    conn.kill(target).await.expect("kill に失敗しました");
    let result = victim.ping().await;
    assert!(result.is_err(), "kill された接続は ping に失敗するべき");
}

/// トランザクション (直メソッド / ガード型 / セーブポイント) を検証する。
#[tokio::test]
async fn test_transactions() {
    let node = start_mysql().await;
    let mut conn = connect(&node).await;
    conn.query("CREATE TABLE IF NOT EXISTS tx_test (id INT)", false)
        .await
        .expect("テーブル作成に失敗しました");
    conn.query("DELETE FROM tx_test", false)
        .await
        .expect("テーブル初期化に失敗しました");

    // 直メソッド: begin → insert → rollback
    conn.begin().await.expect("begin に失敗しました");
    conn.query("INSERT INTO tx_test VALUES (1)", false)
        .await
        .expect("INSERT に失敗しました");
    conn.rollback().await.expect("rollback に失敗しました");
    let rows = query_rows(&mut conn, "SELECT COUNT(*) FROM tx_test").await;
    assert_eq!(
        rows[0][0],
        Value::Int(0),
        "ロールバック後は行が残らないべき"
    );

    // 直メソッド: begin → insert → commit
    conn.begin().await.expect("begin に失敗しました");
    conn.query("INSERT INTO tx_test VALUES (1)", false)
        .await
        .expect("INSERT に失敗しました");
    conn.commit().await.expect("commit に失敗しました");
    let rows = query_rows(&mut conn, "SELECT COUNT(*) FROM tx_test").await;
    assert_eq!(rows[0][0], Value::Int(1), "コミット後は行が残るべき");

    // ガード型: コミットせずに drop → 次の begin でロールバックされる
    {
        let mut tx = conn.begin().await.expect("begin に失敗しました");
        tx.query("INSERT INTO tx_test VALUES (2)", false)
            .await
            .expect("INSERT に失敗しました");
        // drop で dirty マークされる
    }
    let mut tx = conn.begin().await.expect("2 回目の begin に失敗しました");
    tx.query("INSERT INTO tx_test VALUES (3)", false)
        .await
        .expect("INSERT に失敗しました");
    tx.commit().await.expect("commit に失敗しました");
    let rows = query_rows(&mut conn, "SELECT id FROM tx_test ORDER BY id").await;
    assert_eq!(
        rows.len(),
        2,
        "drop されたトランザクションはロールバックされるべき"
    );
    assert_eq!(rows[0][0], Value::Int(1));
    assert_eq!(rows[1][0], Value::Int(3));

    // セーブポイント: セーブポイント以降の変更だけが破棄される
    let mut tx = conn.begin().await.expect("begin に失敗しました");
    tx.query("INSERT INTO tx_test VALUES (4)", false)
        .await
        .expect("INSERT に失敗しました");
    tx.savepoint("sp1").await.expect("savepoint に失敗しました");
    tx.query("INSERT INTO tx_test VALUES (5)", false)
        .await
        .expect("INSERT に失敗しました");
    tx.rollback_to("sp1")
        .await
        .expect("セーブポイントへのロールバックに失敗しました");
    tx.query("INSERT INTO tx_test VALUES (6)", false)
        .await
        .expect("INSERT に失敗しました");
    tx.commit().await.expect("commit に失敗しました");
    let rows = query_rows(&mut conn, "SELECT id FROM tx_test ORDER BY id").await;
    assert_eq!(
        rows,
        vec![
            vec![Value::Int(1)],
            vec![Value::Int(3)],
            vec![Value::Int(4)],
            vec![Value::Int(6)],
        ],
        "セーブポイント以降の変更だけが破棄されるべき"
    );
}

/// アンバッファードカーソル (SS カーソル相当) を検証する。
#[tokio::test]
async fn test_unbuffered_cursors() {
    let node = start_mysql().await;
    let mut conn = connect(&node).await;
    conn.query("CREATE TABLE IF NOT EXISTS unbuf_test (id INT)", false)
        .await
        .expect("テーブル作成に失敗しました");
    conn.query("DELETE FROM unbuf_test", false)
        .await
        .expect("テーブル初期化に失敗しました");
    let mut sql = String::from("INSERT INTO unbuf_test VALUES ");
    for i in 0..100 {
        if i > 0 {
            sql.push(',');
        }
        sql.push_str(&format!("({})", i));
    }
    conn.query(&sql, false)
        .await
        .expect("バルク INSERT に失敗しました");

    // UnbufferedCursor: fetch_one / fetch_many / fetch_all で行を逐次読む
    {
        let mut cursor = conn.unbuffered_cursor();
        cursor
            .execute("SELECT id FROM unbuf_test ORDER BY id", None)
            .await
            .expect("execute に失敗しました");
        assert_eq!(
            cursor
                .fetch_one()
                .await
                .expect("fetch_one に失敗しました")
                .unwrap()[0],
            Value::Int(0)
        );
        let batch = cursor
            .fetch_many(Some(10))
            .await
            .expect("fetch_many に失敗しました");
        assert_eq!(batch.len(), 10, "fetch_many は指定行数を返すべき");
        assert_eq!(batch[0][0], Value::Int(1));
        let rest = cursor.fetch_all().await.expect("fetch_all に失敗しました");
        assert_eq!(rest.len(), 89, "残りの行は 89 行あるべき");
        assert!(
            cursor
                .fetch_one()
                .await
                .expect("fetch_one に失敗しました")
                .is_none(),
            "末尾に達したら None を返すべき"
        );

        // 読み切らずに次の execute を呼んでも残りの行が消費される
        cursor
            .execute("SELECT id FROM unbuf_test ORDER BY id LIMIT 5", None)
            .await
            .expect("execute に失敗しました");
        cursor
            .execute("SELECT COUNT(*) FROM unbuf_test", None)
            .await
            .expect("execute に失敗しました");
        let row = cursor
            .fetch_one()
            .await
            .expect("fetch_one に失敗しました")
            .unwrap();
        assert_eq!(row[0], Value::Int(100));
        cursor.close().await.expect("close に失敗しました");
    }

    // UnbufferedDictCursor: 辞書形式で行を返す
    {
        let mut dict = UnbufferedDictCursor::new(conn.unbuffered_cursor());
        dict.execute(
            "SELECT id, id + 1 AS next_id FROM unbuf_test ORDER BY id LIMIT 3",
            None,
        )
        .await
        .expect("execute に失敗しました");
        let row = dict
            .fetch_one()
            .await
            .expect("fetch_one に失敗しました")
            .unwrap();
        assert_eq!(row["id"], Value::Int(0));
        assert_eq!(row["next_id"], Value::Int(1));
        let rest = dict.fetch_all().await.expect("fetch_all に失敗しました");
        assert_eq!(rest.len(), 2, "残りの行は 2 行あるべき");
    }
}

/// LOAD DATA LOCAL INFILE を検証する。
#[tokio::test]
async fn test_load_data_local_infile() {
    init_tracing();
    let node = start_mysql().await;
    let mut conn = Connection::connect(ConnectOptions {
        local_infile: true,
        ..connect_options(&node).await
    })
    .await
    .expect("MySQL への接続に失敗しました");
    conn.query(
        "CREATE TABLE IF NOT EXISTS load_local_test (a INT, b VARCHAR(20))",
        false,
    )
    .await
    .expect("テーブル作成に失敗しました");
    conn.query("DELETE FROM load_local_test", false)
        .await
        .expect("テーブル初期化に失敗しました");

    // CSV ファイルを一時ディレクトリに作成する
    let path = std::env::temp_dir().join(format!("mysql_rs_load_local_{}.csv", std::process::id()));
    std::fs::write(&path, "1,foo\n2,bar\n3,baz\n").expect("CSV ファイルの書き込みに失敗しました");

    let query = format!(
        "LOAD DATA LOCAL INFILE '{}' INTO TABLE load_local_test FIELDS TERMINATED BY ','",
        path.display()
    );
    conn.query(&query, false)
        .await
        .expect("LOAD DATA LOCAL INFILE に失敗しました");
    assert_eq!(conn.affected_rows(), 3, "3 行ロードされるべき");

    let rows = query_rows(&mut conn, "SELECT a, b FROM load_local_test ORDER BY a").await;
    assert_eq!(
        rows,
        vec![
            vec![Value::Int(1), Value::String("foo".to_string())],
            vec![Value::Int(2), Value::String("bar".to_string())],
            vec![Value::Int(3), Value::String("baz".to_string())],
        ],
        "CSV の内容がロードされるべき"
    );

    let _ = std::fs::remove_file(&path);
}

/// オプションファイル (my.cnf) の自動適用を検証する。
#[tokio::test]
async fn test_option_file() {
    let node = start_mysql().await;
    let host = node
        .get_host()
        .await
        .expect("コンテナのホスト取得に失敗しました")
        .to_string();
    let port = node
        .get_host_port_ipv4(3306)
        .await
        .expect("コンテナのポート取得に失敗しました");

    // 接続情報を書いたオプションファイルを一時ディレクトリに作成する
    let path = std::env::temp_dir().join(format!("my_mysql_rs_{}.cnf", std::process::id()));
    std::fs::write(
        &path,
        format!("[client]\nhost={}\nport={}\nuser=root\n", host, port),
    )
    .expect("オプションファイルの書き込みに失敗しました");

    // デフォルト値のまま read_default_file だけを設定して接続する
    let options = ConnectOptions {
        read_default_file: Some(path.clone()),
        connect_timeout: Duration::from_secs(60),
        ssl_mode: SslMode::Disabled,
        ..Default::default()
    };
    let mut conn = Connection::connect(options)
        .await
        .expect("オプションファイルによる接続に失敗しました");
    conn.ping().await.expect("ping に失敗しました");

    let _ = std::fs::remove_file(&path);
}
