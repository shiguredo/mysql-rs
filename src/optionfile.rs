// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL オプションファイル (my.cnf) のパース。
//!
//! PyMySQL の `optionfile.Parser` (configparser.RawConfigParser) に相当する。
//! キー名は小文字化され、`_` は `-` に正規化される。
//! 値は両端のクォート (`'` / `"`) が除去される。

use crate::error::{Error, Result};
use std::collections::HashMap;
use std::path::Path;

/// MySQL オプションファイルの内容。
#[derive(Debug, Default, Clone)]
pub struct OptionFile {
    groups: HashMap<String, HashMap<String, String>>,
}

impl OptionFile {
    /// テキストをパースする。
    pub fn parse(text: &str) -> Result<Self> {
        let mut file = Self::default();
        let mut current_group: Option<String> = None;
        for (line_no, raw_line) in text.lines().enumerate() {
            let line = raw_line.trim();
            // 空行と `#` / `;` で始まるコメント行はスキップする。
            if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
                continue;
            }
            if line.starts_with('[') {
                if !line.ends_with(']') {
                    return Err(Self::parse_error(line_no, "unclosed section header"));
                }
                let name = line[1..line.len() - 1].trim();
                if name.is_empty() {
                    return Err(Self::parse_error(line_no, "empty section name"));
                }
                current_group = Some(name.to_string());
                continue;
            }
            let Some(group) = &current_group else {
                return Err(Self::parse_error(line_no, "option without a section"));
            };
            let (key, value) = split_key_value(line);
            let key = key.trim().to_lowercase().replace('_', "-");
            if key.is_empty() {
                return Err(Self::parse_error(line_no, "empty option name"));
            }
            let value = strip_quotes(value.trim());
            file.groups
                .entry(group.clone())
                .or_default()
                .insert(key, value);
        }
        Ok(file)
    }

    /// ファイルを読み込んでパースする。
    pub fn read<P: AsRef<Path>>(path: P) -> Result<Self> {
        let path = path.as_ref();
        let text = std::fs::read_to_string(path).map_err(|e| Error::OperationalError {
            code: crate::constants::client_error::CR_UNKNOWN_ERROR,
            message: format!("Failed to read option file {}: {}", path.display(), e),
        })?;
        Self::parse(&text)
    }

    /// 指定セクションのオプション値を取得する。
    pub fn get(&self, group: &str, key: &str) -> Option<&str> {
        self.groups
            .get(group)
            .and_then(|options| options.get(key))
            .map(String::as_str)
    }

    fn parse_error(line_no: usize, message: &str) -> Error {
        Error::OperationalError {
            code: crate::constants::client_error::CR_UNKNOWN_ERROR,
            message: format!("Invalid option file at line {}: {}", line_no + 1, message),
        }
    }
}

/// キーと値を分割する。
///
/// configparser と同じく `=` または `:` で区切る。
/// 区切りがない場合はキーのみ (値は空文字列) として扱う。
fn split_key_value(line: &str) -> (&str, &str) {
    for (i, ch) in line.char_indices() {
        if ch == '=' || ch == ':' {
            return (&line[..i], &line[i + 1..]);
        }
    }
    (line, "")
}

/// 両端が同じクォートであれば除去する。
fn strip_quotes(value: &str) -> String {
    let bytes = value.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        if (first == b'\'' || first == b'"') && bytes[bytes.len() - 1] == first {
            return value[1..value.len() - 1].to_string();
        }
    }
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_split_key_value() {
        assert_eq!(split_key_value("user=root"), ("user", "root"));
        assert_eq!(split_key_value("user: root"), ("user", " root"));
        assert_eq!(split_key_value("compress"), ("compress", ""));
    }

    #[test]
    fn test_strip_quotes() {
        assert_eq!(strip_quotes("\"secret\""), "secret");
        assert_eq!(strip_quotes("'secret'"), "secret");
        assert_eq!(strip_quotes("secret"), "secret");
        assert_eq!(strip_quotes("'mixed\""), "'mixed\"");
        assert_eq!(strip_quotes("\"\""), "");
    }
}
