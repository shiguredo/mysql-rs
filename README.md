# mysql-rs

[![License](https://img.shields.io/badge/License-Apache%202.0-blue.svg)](https://opensource.org/licenses/Apache-2.0)

## 時雨堂のオープンソースソフトウェアについて

利用前に <https://github.com/shiguredo/oss> をお読みください。

> [!WARNING]
> このリポジトリはお試し実装です。正式リリースは行いません。

## 概要

`mysql-rs` は MySQL クライアントの Rust 実装です。

- `shiguredo_mysql_core` - Sans I/O な MySQL プロトコル実装
- `shiguredo_mysql` - tokio 上で動作する非同期 MySQL クライアント

主な機能:

- 単純クエリプロトコル (パラメータ付きクエリ、PyMySQL 互換の `%s` プレースホルダ)
- カーソル: バッファード / アンバッファード / ディクショナリ / アンバッファードディクショナリ (PyMySQL の SSCursor / DictCursor 相当)
- トランザクション (自動ロールバック、PyMySQL 互換の直メソッド)
- コネクションプール (min_idle 補充、最大生存時間)
- 認証: mysql_native_password / caching_sha2_password / sha256_password / client_ed25519
- TLS (sslmode: disabled / preferred / required)
- 圧縮プロトコル (ZLIB)
- 文字セット自動変換
- オプションファイル (my.cnf) による接続設定
- 型変換: 数値 / 浮動小数 / 日付時刻 / TIME / DECIMAL / 文字列 / バイト列 / リスト / 集合

## 使い方

### 単一接続

```rust
use shiguredo_mysql::{ConnectOptions, Connection, Cursor, SslMode};
use shiguredo_mysql_core::converters::Value;

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: b"password".to_vec(),
        database: Some("mydb".to_string()),
        ssl_mode: SslMode::Disabled,
        ..Default::default()
    };

    let mut conn = Connection::connect(options).await.unwrap();
    let mut cursor = Cursor::new(&mut conn);

    // パラメータ付きクエリ
    cursor
        .execute(
            "SELECT id, name FROM users WHERE age > %s",
            Some(&[Value::Int(20)]),
        )
        .await
        .unwrap();

    for row in cursor.fetch_all().unwrap() {
        println!("{:?}", row);
    }

    conn.close().await.unwrap();
}
```

### 非同期並列クエリ

```rust
use shiguredo_mysql::{ConnectOptions, Connection, Cursor};

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: b"password".to_vec(),
        database: Some("mydb".to_string()),
        ..Default::default()
    };

    // 複数の接続を並列に確立してクエリを実行する
    let handles: Vec<_> = (0..4)
        .map(|i| {
            let opts = options.clone();
            tokio::spawn(async move {
                let mut conn = Connection::connect(opts).await.unwrap();
                let mut cursor = Cursor::new(&mut conn);
                cursor
                    .execute(&format!("SELECT {i} AS num"), None)
                    .await
                    .unwrap();
                let rows = cursor.fetch_all().unwrap();
                rows[0][0].clone()
            })
        })
        .collect();

    for handle in handles {
        println!("{:?}", handle.await.unwrap());
    }
}
```

### コネクションプール

```rust
use shiguredo_mysql::{ConnectOptions, Pool, PoolConfig, Cursor};
use std::time::Duration;

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: b"password".to_vec(),
        database: Some("mydb".to_string()),
        ..Default::default()
    };

    let config = PoolConfig {
        max_size: 10,
        min_idle: 2,
        max_idle_time: Duration::from_secs(600),
        max_lifetime: Duration::from_secs(1800),
        acquire_timeout: Duration::from_secs(30),
    };

    let pool = Pool::start(options, config).await.unwrap();

    // 複数タスクからプールを共有する
    let mut handles = Vec::new();
    for i in 0..8 {
        let pool = pool.clone();
        handles.push(tokio::spawn(async move {
            // acquire で接続を借りる。drop で自動的に返却される
            let mut pooled = pool.acquire().await.unwrap();
            let mut cursor = Cursor::new(pooled.connection_mut());
            cursor
                .execute(&format!("SELECT {i} AS task_id"), None)
                .await
                .unwrap();
            let rows = cursor.fetch_all().unwrap();
            rows[0][0].clone()
        }));
    }

    for handle in handles {
        println!("{:?}", handle.await.unwrap());
    }

    pool.close().await.unwrap();
}
```

### トランザクション

`begin()` が返す `Transaction` ガード型は、`commit` / `rollback` せずに破棄すると
次の `begin()` 時またはプールへの返却時に自動でロールバックされる。

```rust
use shiguredo_mysql::{ConnectOptions, Connection};

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: b"password".to_vec(),
        database: Some("mydb".to_string()),
        ..Default::default()
    };

    let mut conn = Connection::connect(options).await.unwrap();

    let mut tx = conn.begin().await.unwrap();
    tx.query("INSERT INTO users (name) VALUES ('alice')", false)
        .await
        .unwrap();
    // コミットせずに drop するとロールバックされる
    tx.commit().await.unwrap();

    // autocommit を無効にして接続した場合の PyMySQL 互換の直メソッド
    conn.begin().await.unwrap();
    conn.query("INSERT INTO users (name) VALUES ('bob')", false)
        .await
        .unwrap();
    conn.commit().await.unwrap();

    conn.close().await.unwrap();
}
```

### アンバッファードカーソル

`unbuffered_cursor()` は行をメモリに蓄えず、fetch のたびにサーバーから読み込む。
巨大な結果セットを扱う場合に使う (PyMySQL の `SSCursor` 相当)。

```rust
use shiguredo_mysql::{ConnectOptions, Connection};

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        host: "127.0.0.1".to_string(),
        port: 3306,
        user: "root".to_string(),
        password: b"password".to_vec(),
        database: Some("mydb".to_string()),
        ..Default::default()
    };

    let mut conn = Connection::connect(options).await.unwrap();
    let mut cursor = conn.unbuffered_cursor();

    cursor
        .execute("SELECT id, name FROM users", None)
        .await
        .unwrap();

    while let Some(row) = cursor.fetch_one().await.unwrap() {
        println!("{:?}", row);
    }

    conn.close().await.unwrap();
}
```

### オプションファイル (my.cnf)

`read_default_file` を設定すると、接続時にオプションファイルを読み、
デフォルト値のままのフィールドを指定グループ (既定は `client`) の値で補完する。

```rust
use shiguredo_mysql::{ConnectOptions, Connection};
use std::path::PathBuf;

#[tokio::main]
async fn main() {
    let options = ConnectOptions {
        // デフォルト値のままのフィールド (host / port / user など) が補完される
        read_default_file: Some(PathBuf::from("/etc/my.cnf")),
        ..Default::default()
    };

    let mut conn = Connection::connect(options).await.unwrap();
    conn.close().await.unwrap();
}
```

## ライセンス

Apache License 2.0

```text
Copyright 2026 Shiguredo Inc.

Licensed under the Apache License, Version 2.0 (the "License");
you may not use this file except in compliance with the License.
You may obtain a copy of the License at

    http://www.apache.org/licenses/LICENSE-2.0

Unless required by applicable law or agreed to in writing, software
distributed under the License is distributed on an "AS IS" BASIS,
WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
See the License for the specific language governing permissions and
limitations under the License.
```
