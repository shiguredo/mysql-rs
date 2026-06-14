// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! testcontainers を使った MySQL 接続統合テスト。
//!
//! Docker 上で MySQL コンテナを起動し、tokio_mysql クレートから
//! 実際に接続・クエリ実行・結果取得ができることを確認する。

mod helpers;

#[tokio::test]
async fn test_select_one_plus_one() {
    helpers::init_tracing();
    helpers::assert_select_one_plus_one(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_create_insert_and_select() {
    helpers::init_tracing();
    helpers::assert_create_insert_and_select(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_dict_cursor() {
    helpers::init_tracing();
    helpers::assert_dict_cursor(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_multiple_statements_and_types() {
    helpers::init_tracing();
    helpers::assert_multiple_statements_and_types(helpers::build_mysql_options().await.0, "MySQL")
        .await;
}

#[tokio::test]
async fn test_call_procedure() {
    helpers::init_tracing();
    helpers::assert_call_procedure(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_compression_enabled() {
    helpers::init_tracing();
    helpers::assert_compression_enabled(
        helpers::with_compress(helpers::build_mysql_options().await.0),
        "MySQL",
    )
    .await;
}
