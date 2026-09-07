// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! optionfile モジュールの Property-Based Testing。
//!
//! 任意のセクション・キー・値から組み立てたテキストをパースしたとき、
//! `get` で元の値 (クォート・空白除去済み) を取り出せることを検証する。

use std::cell::Cell;

use shiguredo_mysql_core::optionfile::OptionFile;

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

/// キー・セクション名に使える文字のプール。
const NAME_POOL: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";

/// キー名・セクション名をサンプリングする（`[a-zA-Z0-9_]+` 相当）。
fn sample_name(ctx: &mut noprop::TestCaseContext) -> String {
    // 長さ 1 以上・両端の境界にも確率を与える。
    let len =
        noprop::sample_with_boundaries(ctx, &[1usize, 16], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 1..=16)
        });
    (0..len)
        .map(|_| noprop::sample_choice(ctx, NAME_POOL) as char)
        .collect()
}

/// 値をサンプリングする（`.{0,32}` 相当・改行を含まない印字可能 ASCII）。
fn sample_option_value(ctx: &mut noprop::TestCaseContext) -> String {
    // 空・最大長にも確率を与えて境界を重点的に生成する。
    let len =
        noprop::sample_with_boundaries(ctx, &[0usize, 1, 32], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 0..=32)
        });
    noprop::sample_ascii_printable_string(ctx, len)
}

/// キーと値のペアをサンプリングする。
fn sample_key_value(ctx: &mut noprop::TestCaseContext) -> (String, String) {
    (sample_name(ctx), sample_option_value(ctx))
}

/// エントリー列をサンプリングする（0〜8 件）。
fn sample_entries(ctx: &mut noprop::TestCaseContext) -> Vec<(String, String)> {
    let len =
        noprop::sample_with_boundaries(ctx, &[0usize, 1, 8], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 0..=8)
        });
    (0..len).map(|_| sample_key_value(ctx)).collect()
}

/// 両端のクォートを除去する (OptionFile の正規化と同一の規則)。
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

/// パース時の値の正規化 (trim + クォート除去) を再現する。
fn normalize_value(value: &str) -> String {
    strip_quotes(value.trim())
}

/// パースしたオプションファイルから、書いた値が正規化済みで取り出せる。
#[test]
fn prop_option_file_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空のエントリー列の両方が実行されたことを記録する。
    let empty_entries = Cell::new(0usize);
    let non_empty_entries = Cell::new(0usize);
    // 正規化で変化する値（前後空白・クォート付き）が実行されたことを記録する。
    let normalized_changed = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let entries = sample_entries(ctx);
        let section = sample_name(ctx);
        // キーは小文字化と _ → - の正規化が入るため、正規化後のキーを期待値として使う。
        // 正規化後に衝突しないよう、インデックスを付けて一意にする。
        let normalized: Vec<(String, String)> = entries
            .iter()
            .enumerate()
            .map(|(i, (key, value))| {
                let key = format!("{}_{}", i, key).to_lowercase().replace('_', "-");
                (key, normalize_value(value))
            })
            .collect();

        let mut text = format!("[{}]\n", section);
        for (i, (key, value)) in entries.iter().enumerate() {
            text.push_str(&format!("{}_{}={}\n", i, key, value));
        }

        let file = OptionFile::parse(&text).expect("オプションファイルの解析に失敗");
        for (key, expected) in &normalized {
            assert_eq!(
                file.get(&section, key),
                Some(expected.as_str()),
                "書き込んだ値が取り出せない: [{section}] {key}"
            );
        }
        if entries.is_empty() {
            empty_entries.set(empty_entries.get() + 1);
        } else {
            non_empty_entries.set(non_empty_entries.get() + 1);
        }
        if entries.iter().any(|(_, v)| normalize_value(v) != *v) {
            normalized_changed.set(normalized_changed.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty_entries.get() > 0,
        "空のエントリー列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty_entries.get() > 0,
        "非空のエントリー列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        normalized_changed.get() > 0,
        "正規化で変化する値が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// パース結果は決定的である (同じ入力から同じ出力)。
#[test]
fn prop_option_file_deterministic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty_entries = Cell::new(0usize);
    let non_empty_entries = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let entries = sample_entries(ctx);
        let mut text = String::from("[client]\n");
        for (i, (key, value)) in entries.iter().enumerate() {
            text.push_str(&format!("{}_{}={}\n", i, key, value));
        }
        let file1 = OptionFile::parse(&text).expect("オプションファイルの解析に失敗");
        let file2 = OptionFile::parse(&text).expect("オプションファイルの解析に失敗");
        for (i, (key, value)) in entries.iter().enumerate() {
            let key = format!("{}_{}", i, key).to_lowercase().replace('_', "-");
            let expected = normalize_value(value);
            assert_eq!(
                file1.get("client", &key),
                file2.get("client", &key),
                "同じ入力に対する解析結果が一致しない: {key}"
            );
            assert_eq!(
                file1.get("client", &key),
                Some(expected.as_str()),
                "書き込んだ値が取り出せない: {key}"
            );
        }
        if entries.is_empty() {
            empty_entries.set(empty_entries.get() + 1);
        } else {
            non_empty_entries.set(non_empty_entries.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty_entries.get() > 0,
        "空のエントリー列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty_entries.get() > 0,
        "非空のエントリー列が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}
