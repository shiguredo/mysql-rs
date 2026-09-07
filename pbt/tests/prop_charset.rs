// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! charset モジュールの Property-Based Testing。
//!
//! 文字セット参照関数の一貫性と不変条件を検証する。

use std::cell::Cell;
use std::collections::HashSet;

use shiguredo_mysql_core::charset::{charset_by_id, charset_by_name, mblength};
use shiguredo_mysql_core::constants::field_type;
use shiguredo_mysql_core::protocol::FieldDescriptorPacket;

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

/// 既知の文字セット ID の一覧。
const KNOWN_IDS: [u16; 7] = [1, 33, 63, 192, 193, 224, 225];

/// ラウンドトリップ対象（`is_default` が真）の文字セット ID の一覧。
const DEFAULT_IDS: [u16; 3] = [1, 33, 63];

/// 既知の文字セット名の一覧。
const KNOWN_NAMES: [&str; 6] = ["big5", "utf8mb4", "latin1", "binary", "gb2312", "sjis"];

/// エンコーディング対応表の検証対象。
const ENCODING_NAMES: [&str; 6] = ["utf8mb4", "utf8mb3", "latin1", "koi8r", "koi8u", "big5"];

/// カラム長計算の検証対象の文字セット番号。
const MBLENGTH_CHARSETNRS: [u16; 5] = [8, 33, 63, 88, 91];

/// "utf8" の別名として "utf8mb4" が返る。
#[test]
fn prop_charset_by_name_utf8_alias() {
    let charset = charset_by_name("utf8").expect("utf8 の別名解決に失敗");
    assert_eq!(charset.name, "utf8mb4");
}

/// 既知の文字セット ID に対しては対応する文字セットが返り、id が一致する。
#[test]
fn prop_charset_by_id_known_ids() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全ての既知 ID が実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let id = noprop::sample_choice(ctx, &KNOWN_IDS);
        let charset = charset_by_id(id).expect("既知の文字セットが見つからない");
        assert_eq!(charset.id, id, "文字セット ID が一致しない");
        seen.borrow_mut().insert(id);
        Ok(())
    })?;
    let seen = seen.borrow();
    for id in KNOWN_IDS {
        assert!(
            seen.contains(&id),
            "既知 ID {id} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// 存在しない ID に対しては None が返る。
#[test]
fn prop_charset_by_id_unknown_returns_none() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 1000 以上の範囲は既知 ID と重ならない。
        // 両端の境界にも意味のある確率を与える。
        let id = noprop::sample_with_boundaries(
            ctx,
            &[1000u16, u16::MAX],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, 1000..=u16::MAX as usize) as u16,
        );
        assert!(
            charset_by_id(id).is_none(),
            "未知の ID {id} に対して文字セットが返された"
        );
        Ok(())
    })?;
    Ok(())
}

/// 既知の文字セット名に対しては対応する文字セットが返り、name が一致する。
#[test]
fn prop_charset_by_name_known_names() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全ての既知名が実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = noprop::sample_choice(ctx, &KNOWN_NAMES);
        let charset = charset_by_name(name).expect("既知の文字セットが見つからない");
        assert_eq!(&charset.name, name, "文字セット名が一致しない");
        seen.borrow_mut().insert(name);
        Ok(())
    })?;
    let seen = seen.borrow();
    for name in KNOWN_NAMES {
        assert!(
            seen.contains(name),
            "既知名 {name} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// 存在しない名前に対しては None が返る。
#[test]
fn prop_charset_by_name_unknown_returns_none() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        // 既知名は 10 文字未満のため、20 文字以上にすれば重ならない。
        let len = noprop::sample_usize_in(ctx, 20..=40);
        let name: String = (0..len)
            .map(|_| noprop::sample_choice(ctx, b"abcdefghijklmnopqrstuvwxyz0123456789_") as char)
            .collect();
        assert!(
            charset_by_name(&name).is_none(),
            "未知の名前 {name:?} に対して文字セットが返された"
        );
        Ok(())
    })?;
    Ok(())
}

/// ID で取得した is_default 文字セットの名前を使って名前検索すると、同じ文字セットが返る。
#[test]
fn prop_charset_id_name_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全ての対象 ID が実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let id = noprop::sample_choice(ctx, &DEFAULT_IDS);
        let by_id = charset_by_id(id).expect("既知の文字セットが見つからない");
        assert!(
            by_id.is_default,
            "ラウンドトリップ対象はデフォルト文字セットであるべき: {id}"
        );
        let by_name = charset_by_name(&by_id.name).expect("名前検索に失敗");
        assert_eq!(by_name.id, id, "ID が一致しない");
        assert_eq!(&by_name.name, &by_id.name, "名前が一致しない");
        seen.borrow_mut().insert(id);
        Ok(())
    })?;
    let seen = seen.borrow();
    for id in DEFAULT_IDS {
        assert!(
            seen.contains(&id),
            "対象 ID {id} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// Charset::encoding() は MySQL 名からエンコーディングラベルへの対応を満たす。
#[test]
fn prop_charset_encoding_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全ての対象名が実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = noprop::sample_choice(ctx, &ENCODING_NAMES);
        let charset = charset_by_name(name).expect("既知の文字セットが見つからない");
        let expected = match name {
            "utf8mb4" | "utf8mb3" => "utf8",
            "latin1" => "cp1252",
            "koi8r" => "koi8_r",
            "koi8u" => "koi8_u",
            other => other,
        };
        assert_eq!(charset.encoding(), expected, "エンコーディングが一致しない");
        seen.borrow_mut().insert(name);
        Ok(())
    })?;
    let seen = seen.borrow();
    for name in ENCODING_NAMES {
        assert!(
            seen.contains(name),
            "対象名 {name} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// mblength は常に 1 以上の値を返す。
#[test]
fn prop_mblength_positive() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let charsetnr = noprop::sample_u16(ctx);
        assert!(mblength(charsetnr) >= 1, "mblength は 1 以上であるべき");
        Ok(())
    })?;
    Ok(())
}

/// mblength は 0 にならないため、VAR_STRING のカラム長計算で除算エラーにならない。
#[test]
fn prop_mblength_and_column_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 全ての対象文字セット番号が実行されたことを記録する。
    let seen = std::cell::RefCell::new(HashSet::new());
    // 最大値のカラム長が実行されたことを記録する。
    let max_length = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let charsetnr = noprop::sample_choice(ctx, &MBLENGTH_CHARSETNRS);
        // 1 以上かつ境界（1・最大値）にも確率を与える。
        let length = noprop::sample_with_boundaries(
            ctx,
            &[1u32, u32::MAX],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_u64_in(ctx, 1..=u32::MAX as u64) as u32,
        );
        let mblen = mblength(charsetnr) as u32;
        assert!(mblen >= 1, "mblength は 1 以上であるべき");
        // FieldDescriptorPacket::get_column_length は length / mblen を返す。
        // catalog, db, table_name, org_table, name, org_name を空の length-encoded string で埋める。
        let mut payload = vec![0u8; 6];
        payload.push(0x0c); // filler
        payload.extend_from_slice(&charsetnr.to_le_bytes());
        payload.extend_from_slice(&length.to_le_bytes());
        payload.push(field_type::VAR_STRING);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        let fd = FieldDescriptorPacket::parse(payload, "utf8").expect("フィールド記述の解析に失敗");
        let column_length = fd.get_column_length();
        assert_eq!(
            column_length,
            length / mblen,
            "カラム長が length / mblen と一致しない"
        );
        seen.borrow_mut().insert(charsetnr);
        if length == u32::MAX {
            max_length.set(max_length.get() + 1);
        }
        Ok(())
    })?;
    let seen = seen.borrow();
    for charsetnr in MBLENGTH_CHARSETNRS {
        assert!(
            seen.contains(&charsetnr),
            "文字セット番号 {charsetnr} が 1 件も実行されなかった\n{runner}"
        );
    }
    assert!(
        max_length.get() > 0,
        "最大カラム長が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}
