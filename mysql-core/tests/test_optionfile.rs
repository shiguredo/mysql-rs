// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

use shiguredo_mysql_core::optionfile::OptionFile;

#[test]
fn test_parse_basic() {
    let file = OptionFile::parse(
        "[client]\n\
         user = root\n\
         password = \"secret\"\n\
         host = 'localhost'\n\
         port = 3307\n",
    )
    .unwrap();
    assert_eq!(file.get("client", "user"), Some("root"));
    assert_eq!(file.get("client", "password"), Some("secret"));
    assert_eq!(file.get("client", "host"), Some("localhost"));
    assert_eq!(file.get("client", "port"), Some("3307"));
}

#[test]
fn test_parse_key_normalization() {
    // `_` は `-` に正規化される。
    let file = OptionFile::parse("[client]\ndefault_character_set = utf8mb4\n").unwrap();
    assert_eq!(file.get("client", "default-character-set"), Some("utf8mb4"));
}

#[test]
fn test_parse_multiple_sections() {
    let file = OptionFile::parse(
        "[client]\n\
         user = root\n\
         [mysqld]\n\
         port = 3306\n",
    )
    .unwrap();
    assert_eq!(file.get("client", "user"), Some("root"));
    assert_eq!(file.get("mysqld", "port"), Some("3306"));
    assert_eq!(file.get("client", "port"), None);
}

#[test]
fn test_parse_colon_separator() {
    let file = OptionFile::parse("[client]\nuser: root\n").unwrap();
    assert_eq!(file.get("client", "user"), Some("root"));
}

#[test]
fn test_parse_allow_no_value() {
    // 値のないフラグ形式のキーは空文字列として扱う。
    let file = OptionFile::parse("[client]\ncompress\n").unwrap();
    assert_eq!(file.get("client", "compress"), Some(""));
}

#[test]
fn test_parse_comments() {
    let file = OptionFile::parse(
        "# comment\n\
         ; comment\n\
         [client]\n\
         user = root  # inline comment is not stripped\n",
    )
    .unwrap();
    // configparser と同じくインラインコメントは除去されない。
    assert_eq!(
        file.get("client", "user"),
        Some("root  # inline comment is not stripped")
    );
}

#[test]
fn test_parse_missing_section() {
    let result = OptionFile::parse("user = root\n");
    assert!(result.is_err(), "セクション外のキーはエラーにするべき");
}

#[test]
fn test_parse_unclosed_section() {
    let result = OptionFile::parse("[client\n");
    assert!(result.is_err(), "閉じていないセクションはエラーにするべき");
}

#[test]
fn test_read_missing_file() {
    let result = OptionFile::read("/nonexistent/my.cnf");
    assert!(result.is_err(), "存在しないファイルはエラーにするべき");
}
