// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! shiguredo_container を使った MySQL 接続統合テスト。
//!
//! コンテナ上で MySQL コンテナを起動し、tokio_mysql クレートから
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

#[tokio::test]
async fn test_null_values() {
    helpers::init_tracing();
    helpers::assert_null_values(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_integer_boundary_values() {
    helpers::init_tracing();
    helpers::assert_integer_boundary_values(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_float_types() {
    helpers::init_tracing();
    helpers::assert_float_types(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_decimal_type() {
    helpers::init_tracing();
    helpers::assert_decimal_type(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_string_types() {
    helpers::init_tracing();
    helpers::assert_string_types(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_binary_types() {
    helpers::init_tracing();
    helpers::assert_binary_types(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_date_time_types() {
    helpers::init_tracing();
    helpers::assert_date_time_types(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_time_type() {
    helpers::init_tracing();
    helpers::assert_time_type(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_transaction_commit() {
    helpers::init_tracing();
    helpers::assert_transaction_commit(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_transaction_rollback() {
    helpers::init_tracing();
    helpers::assert_transaction_rollback(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_execute_many() {
    helpers::init_tracing();
    helpers::assert_execute_many(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_fetch_one() {
    helpers::init_tracing();
    helpers::assert_fetch_one(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_fetch_many() {
    helpers::init_tracing();
    helpers::assert_fetch_many(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_cursor_scroll() {
    helpers::init_tracing();
    helpers::assert_cursor_scroll(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_empty_result_set() {
    helpers::init_tracing();
    helpers::assert_empty_result_set(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_connection_metadata() {
    helpers::init_tracing();
    helpers::assert_connection_metadata(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_escape_string() {
    helpers::init_tracing();
    helpers::assert_escape_string(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_literal() {
    helpers::init_tracing();
    helpers::assert_literal(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_mogrify() {
    helpers::init_tracing();
    helpers::assert_mogrify(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_unicode() {
    helpers::init_tracing();
    helpers::assert_unicode(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_large_data() {
    helpers::init_tracing();
    helpers::assert_large_data(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_syntax_error() {
    helpers::init_tracing();
    helpers::assert_syntax_error(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_unknown_table_error() {
    helpers::init_tracing();
    helpers::assert_unknown_table_error(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_duplicate_key_error() {
    helpers::init_tracing();
    helpers::assert_duplicate_key_error(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_show_databases() {
    helpers::init_tracing();
    helpers::assert_show_databases(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_show_tables() {
    helpers::init_tracing();
    helpers::assert_show_tables(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_information_schema() {
    helpers::init_tracing();
    helpers::assert_information_schema(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_connection_close() {
    helpers::init_tracing();
    helpers::assert_connection_close(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_multiple_connections() {
    helpers::init_tracing();
    helpers::assert_multiple_connections(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_auto_increment_and_insert_id() {
    helpers::init_tracing();
    helpers::assert_auto_increment_and_insert_id(helpers::build_mysql_options().await.0, "MySQL")
        .await;
}

#[tokio::test]
async fn test_affected_rows() {
    helpers::init_tracing();
    helpers::assert_affected_rows(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_special_characters() {
    helpers::init_tracing();
    helpers::assert_special_characters(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_enum_and_set_types() {
    helpers::init_tracing();
    helpers::assert_enum_and_set_types(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_multiple_result_sets() {
    helpers::init_tracing();
    helpers::assert_multiple_result_sets(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_parameterized_where() {
    helpers::init_tracing();
    helpers::assert_parameterized_where(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_update_and_delete() {
    helpers::init_tracing();
    helpers::assert_update_and_delete(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_group_by_and_aggregation() {
    helpers::init_tracing();
    helpers::assert_group_by_and_aggregation(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_subquery() {
    helpers::init_tracing();
    helpers::assert_subquery(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_join() {
    helpers::init_tracing();
    helpers::assert_join(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_order_by_and_limit() {
    helpers::init_tracing();
    helpers::assert_order_by_and_limit(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_dict_cursor_fetch_methods() {
    helpers::init_tracing();
    helpers::assert_dict_cursor_fetch_methods(helpers::build_mysql_options().await.0, "MySQL")
        .await;
}

#[tokio::test]
async fn test_init_command() {
    helpers::init_tracing();
    helpers::assert_init_command(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_sql_mode() {
    helpers::init_tracing();
    helpers::assert_sql_mode(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_replace_statement() {
    helpers::init_tracing();
    helpers::assert_replace_statement(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_on_duplicate_key_update() {
    helpers::init_tracing();
    helpers::assert_on_duplicate_key_update(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_empty_string_vs_null() {
    helpers::init_tracing();
    helpers::assert_empty_string_vs_null(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_wide_row() {
    helpers::init_tracing();
    helpers::assert_wide_row(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_many_rows() {
    helpers::init_tracing();
    helpers::assert_many_rows(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_select_expressions() {
    helpers::init_tracing();
    helpers::assert_select_expressions(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_pool_basic() {
    helpers::init_tracing();
    helpers::assert_pool_basic(helpers::build_mysql_options().await.0, "MySQL").await;
}

#[tokio::test]
async fn test_pool_concurrent() {
    helpers::init_tracing();
    helpers::assert_pool_concurrent(helpers::build_mysql_options().await.0, "MySQL").await;
}
