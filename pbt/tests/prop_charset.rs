// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! charset モジュールの Property-Based Testing。
//!
//! 文字セット参照関数の一貫性と不変条件を検証する。

use proptest::prelude::*;
use shiguredo_mysql::charset::{charset_by_id, charset_by_name, mblength};
use shiguredo_mysql::constants::field_type;
use shiguredo_mysql::protocol::FieldDescriptorPacket;

/// "utf8" の別名として "utf8mb4" が返る。
#[test]
fn prop_charset_by_name_utf8_alias() {
    let charset = charset_by_name("utf8").expect("utf8 alias");
    assert_eq!(charset.name, "utf8mb4");
}

proptest! {
    /// 既知の文字セット ID に対しては対応する文字セットが返り、id が一致する。
    #[test]
    fn prop_charset_by_id_known_ids(
        id in prop_oneof![Just(1u16), Just(33u16), Just(63u16), Just(192u16), Just(193u16), Just(224u16), Just(225u16)],
    ) {
        let charset = charset_by_id(id).expect("known charset");
        prop_assert_eq!(charset.id, id);
    }

    /// 存在しない ID に対しては None が返る。
    #[test]
    fn prop_charset_by_id_unknown_returns_none(id in 1000u16..=u16::MAX) {
        prop_assert!(charset_by_id(id).is_none());
    }

    /// 既知の文字セット名に対しては対応する文字セットが返り、name が一致する。
    #[test]
    fn prop_charset_by_name_known_names(
        name in prop_oneof![Just("big5"), Just("utf8mb4"), Just("latin1"), Just("binary"), Just("gb2312"), Just("sjis")],
    ) {
        let charset = charset_by_name(name).expect("known charset");
        prop_assert_eq!(&charset.name, name);
    }

    /// 存在しない名前に対しては None が返る。
    #[test]
    fn prop_charset_by_name_unknown_returns_none(name in "[a-z0-9_]{20,40}") {
        prop_assert!(charset_by_name(&name).is_none());
    }

    /// ID で取得した is_default 文字セットの名前を使って名前検索すると、同じ文字セットが返る。
    #[test]
    fn prop_charset_id_name_roundtrip(
        id in prop_oneof![Just(1u16), Just(33u16), Just(63u16)],
    ) {
        let by_id = charset_by_id(id).expect("known charset");
        prop_assert!(by_id.is_default, "roundtrip target must be default charset");
        let by_name = charset_by_name(&by_id.name).expect("charset by name");
        prop_assert_eq!(by_name.id, id);
        prop_assert_eq!(&by_name.name, &by_id.name);
    }

    /// Charset::encoding() は MySQL 名からエンコーディングラベルへの対応を満たす。
    #[test]
    fn prop_charset_encoding_consistency(
        name in prop_oneof![Just("utf8mb4"), Just("utf8mb3"), Just("latin1"), Just("koi8r"), Just("koi8u"), Just("big5")],
    ) {
        let charset = charset_by_name(name).expect("known charset");
        let expected = match name {
            "utf8mb4" | "utf8mb3" => "utf8",
            "latin1" => "cp1252",
            "koi8r" => "koi8_r",
            "koi8u" => "koi8_u",
            other => other,
        };
        prop_assert_eq!(charset.encoding(), expected);
    }

    /// mblength は常に 1 以上の値を返す。
    #[test]
    fn prop_mblength_positive(charsetnr in any::<u16>()) {
        prop_assert!(mblength(charsetnr) >= 1);
    }

    /// mblength は 0 にならないため、VAR_STRING のカラム長計算で除算エラーにならない。
    #[test]
    fn prop_mblength_and_column_length(
        charsetnr in prop_oneof![Just(8u16), Just(33u16), Just(63u16), Just(88u16), Just(91u16)],
        length in 1u32..=u32::MAX,
    ) {
        let mblen = mblength(charsetnr) as u32;
        prop_assert!(mblen >= 1);
        // FieldDescriptorPacket::get_column_length は length / mblen を返す。
        // catalog, db, table_name, org_table, name, org_name を空の length-encoded string で埋める。
        let mut payload = vec![0u8; 6];
        payload.push(0x0c); // filler
        payload.extend_from_slice(&charsetnr.to_le_bytes());
        payload.extend_from_slice(&length.to_le_bytes());
        payload.push(field_type::VAR_STRING);
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        let fd = FieldDescriptorPacket::parse(payload, "utf8").expect("valid field descriptor");
        let column_length = fd.get_column_length();
        prop_assert_eq!(column_length, length / mblen);
    }
}
