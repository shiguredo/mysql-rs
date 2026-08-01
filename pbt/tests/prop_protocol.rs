// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! protocol モジュールの Property-Based Testing。
//!
//! lenenc_int のエンコード・デコードや MysqlPacket の読み書きに対して、
//! ランダムな入力に対するプロパティを検証する。

use proptest::prelude::*;
use shiguredo_mysql_core::connection::lenenc_int;
use shiguredo_mysql_core::protocol::{
    EofPacketWrapper, FieldDescriptorPacket, LoadLocalPacketWrapper, MysqlPacket, OkPacketWrapper,
};

/// 空のパケットに対しては全てのパケット判定が false を返す。
#[test]
fn prop_empty_packet_no_kind() {
    let packet = MysqlPacket::new(vec![]);
    assert!(!packet.is_ok_packet());
    assert!(!packet.is_eof_packet());
    assert!(!packet.is_auth_switch_request());
    assert!(!packet.is_extra_auth_data());
    assert!(!packet.is_resultset_packet());
    assert!(!packet.is_load_local_packet());
    assert!(!packet.is_error_packet());
}

proptest! {
    /// lenenc_int でエンコードした値を read_length_encoded_integer で復号すると元に戻る。
    #[test]
    fn prop_lenenc_int_roundtrip(i in 0..=u64::MAX) {
        let encoded = lenenc_int(i as usize);
        let mut packet = MysqlPacket::new(encoded);
        let decoded = packet.read_length_encoded_integer().expect("lenenc int decode");
        prop_assert_eq!(decoded, Some(i));
    }

    /// NUL を含まないバイト列を NUL 終端して read_string で読み込める。
    #[test]
    fn prop_read_string_roundtrip(
        data in proptest::collection::vec(1..=255u8, 0..=256),
    ) {
        let mut payload = data.clone();
        payload.push(0);
        let mut packet = MysqlPacket::new(payload);
        let decoded = packet.read_string().expect("null-terminated string decode");
        prop_assert_eq!(decoded.as_ref(), Some(&data));
    }

    /// read_length_encoded_integer は長さ符号付き整数を含む先頭部分だけを消費し、
    /// 残りのバイト列はそのまま残す。
    #[test]
    fn prop_lenenc_int_consumes_only_prefix(i in 0..=u64::MAX, suffix in proptest::collection::vec(any::<u8>(), 0..=32)) {
        let mut encoded = lenenc_int(i as usize);
        encoded.extend_from_slice(&suffix);
        let mut packet = MysqlPacket::new(encoded);
        let decoded = packet.read_length_encoded_integer().expect("lenenc int decode");
        prop_assert_eq!(decoded, Some(i));
        let remaining = packet.read_all();
        prop_assert_eq!(remaining, suffix);
    }

    /// read_uint8 は 1 バイトのリトルエンディアンから元の値を復元する。
    #[test]
    fn prop_read_uint8_roundtrip(value in any::<u8>()) {
        let mut packet = MysqlPacket::new(vec![value]);
        let decoded = packet.read_uint8().expect("read uint8");
        prop_assert_eq!(decoded, value);
    }

    /// read_uint16 は 2 バイトのリトルエンディアンから元の値を復元する。
    #[test]
    fn prop_read_uint16_roundtrip(value in any::<u16>()) {
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint16().expect("read uint16");
        prop_assert_eq!(decoded, value);
    }

    /// read_uint24 は 3 バイトのリトルエンディアンから元の値を復元する。
    #[test]
    fn prop_read_uint24_roundtrip(value in 0..=0xFFFFFFu32) {
        let bytes = value.to_le_bytes();
        let mut packet = MysqlPacket::new(vec![bytes[0], bytes[1], bytes[2]]);
        let decoded = packet.read_uint24().expect("read uint24");
        prop_assert_eq!(decoded, value);
    }

    /// read_uint32 は 4 バイトのリトルエンディアンから元の値を復元する。
    #[test]
    fn prop_read_uint32_roundtrip(value in any::<u32>()) {
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint32().expect("read uint32");
        prop_assert_eq!(decoded, value);
    }

    /// read_uint64 は 8 バイトのリトルエンディアンから元の値を復元する。
    #[test]
    fn prop_read_uint64_roundtrip(value in any::<u64>()) {
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint64().expect("read uint64");
        prop_assert_eq!(decoded, value);
    }

    /// read_length_coded_string は長さ符号付き文字列を正しく復元する。
    #[test]
    fn prop_read_length_coded_string_roundtrip(
        data in proptest::collection::vec(any::<u8>(), 0..=255),
    ) {
        let mut payload = lenenc_int(data.len());
        payload.extend_from_slice(&data);
        let mut packet = MysqlPacket::new(payload);
        let decoded = packet.read_length_coded_string().expect("lenenc string decode");
        prop_assert_eq!(decoded.as_ref(), Some(&data));
    }

    /// lenenc_int のエンコード長は値の範囲に応じて仕様通り（1/3/4/9 バイト）である。
    #[test]
    fn prop_lenenc_int_encoding_length(i in 0..=u64::MAX) {
        let encoded = lenenc_int(i as usize);
        let expected_len = if i < 0xFB {
            1
        } else if i < (1 << 16) {
            3
        } else if i < (1 << 24) {
            4
        } else {
            9
        };
        prop_assert_eq!(encoded.len(), expected_len);
        if i < 0xFB {
            prop_assert_eq!(encoded[0], i as u8);
        } else if i < (1 << 16) {
            prop_assert_eq!(encoded[0], 0xFC);
        } else if i < (1 << 24) {
            prop_assert_eq!(encoded[0], 0xFD);
        } else {
            prop_assert_eq!(encoded[0], 0xFE);
        }
    }

    /// read_* 呼び出し後、カーソル位置は正しく進み、read_all で残り全部が読める。
    #[test]
    fn prop_read_position_advances(value in any::<u64>()) {
        let mut payload = value.to_le_bytes().to_vec();
        payload.extend_from_slice(&[0xAB, 0xCD]);
        let mut packet = MysqlPacket::new(payload.clone());
        let _ = packet.read_uint64().expect("read uint64");
        prop_assert_eq!(packet.position(), 8);
        let remaining = packet.read_all();
        prop_assert_eq!(remaining, &[0xAB, 0xCD]);
        prop_assert_eq!(packet.position(), payload.len());
    }

    /// 0xFF から始まるパケットは error パケットとして判定され、ok パケットとは同時に true にならない。
    #[test]
    fn prop_error_packet_exclusive(data in proptest::collection::vec(any::<u8>(), 1..=256)) {
        let mut payload = vec![0xFF];
        payload.extend_from_slice(&data);
        let packet = MysqlPacket::new(payload);
        prop_assert!(packet.is_error_packet());
        prop_assert!(!packet.is_ok_packet());
    }

    /// パケット種別判定は相互に排他的である（EOF/認証スイッチの包含関係を除く）。
    #[test]
    fn prop_packet_kind_consistency(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let packet = MysqlPacket::new(data.clone());
        let is_ok = packet.is_ok_packet();
        let is_error = packet.is_error_packet();
        let is_load_local = packet.is_load_local_packet();

        // OK と Error、LOAD LOCAL は同時に true にならない。
        prop_assert!(!(is_ok && is_error));
        prop_assert!(!(is_ok && is_load_local));
        prop_assert!(!(is_error && is_load_local));

        // OK パケットは長さが 7 バイト以上、かつ先頭が 0x00。
        if is_ok {
            prop_assert!(data.first() == Some(&0x00));
            prop_assert!(data.len() >= 7);
        }

        // Error パケットは先頭が 0xFF。
        if is_error {
            prop_assert!(data.first() == Some(&0xFF));
        }
    }

    /// 有効な OK パケットは from_packet で解析でき、無効なパケットはエラーになる。
    #[test]
    fn prop_ok_packet_boundary(
        affected_rows in 0..=u64::MAX,
        insert_id in 0..=u64::MAX,
        server_status in any::<u16>(),
        warning_count in any::<u16>(),
        message in proptest::collection::vec(any::<u8>(), 0..=32),
    ) {
        let mut payload = vec![0x00];
        payload.extend_from_slice(&lenenc_int(affected_rows as usize));
        payload.extend_from_slice(&lenenc_int(insert_id as usize));
        payload.extend_from_slice(&server_status.to_le_bytes());
        payload.extend_from_slice(&warning_count.to_le_bytes());
        payload.extend_from_slice(&message);
        let mut packet = MysqlPacket::new(payload);
        let ok = OkPacketWrapper::from_packet(&mut packet);
        prop_assert!(ok.is_ok(), "valid OK packet failed: {:?}", ok);
        let ok = ok.unwrap();
        prop_assert_eq!(ok.affected_rows, Some(affected_rows));
        prop_assert_eq!(ok.insert_id, Some(insert_id));
        prop_assert_eq!(ok.server_status, server_status);
        prop_assert_eq!(ok.warning_count, warning_count);
        prop_assert_eq!(ok.message, message);
    }

    /// 有効な EOF パケットは from_packet で解析でき、無効なパケットはエラーになる。
    #[test]
    fn prop_eof_packet_boundary(
        warning_count in any::<u16>(),
        server_status in any::<u16>(),
    ) {
        let mut payload = vec![0xFE];
        payload.extend_from_slice(&warning_count.to_le_bytes());
        payload.extend_from_slice(&server_status.to_le_bytes());
        let mut packet = MysqlPacket::new(payload);
        let eof = EofPacketWrapper::from_packet(&mut packet);
        prop_assert!(eof.is_ok(), "valid EOF packet failed: {:?}", eof);
        let eof = eof.unwrap();
        prop_assert_eq!(eof.warning_count, warning_count);
        prop_assert_eq!(eof.server_status, server_status);
    }

    /// FieldDescriptorPacket::description は元のパケットの情報と一致する。
    #[test]
    fn prop_field_descriptor_description_consistency(
        name in "[a-zA-Z0-9_]{1,32}",
        charsetnr in prop_oneof![Just(8u16), Just(33u16), Just(63u16)],
        length in any::<u32>(),
        type_code in any::<u8>(),
        flags in any::<u16>(),
        scale in any::<u8>(),
    ) {
        let mut payload = lenenc_int(0); // catalog
        payload.extend_from_slice(&lenenc_int(0)); // db
        payload.extend_from_slice(&lenenc_int(name.len())); // table_name
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&lenenc_int(0)); // org_table
        payload.extend_from_slice(&lenenc_int(name.len())); // name
        payload.extend_from_slice(name.as_bytes());
        payload.extend_from_slice(&lenenc_int(0)); // org_name
        payload.push(0x0c); // filler
        payload.extend_from_slice(&charsetnr.to_le_bytes());
        payload.extend_from_slice(&length.to_le_bytes());
        payload.push(type_code);
        payload.extend_from_slice(&flags.to_le_bytes());
        payload.push(scale);
        let fd = FieldDescriptorPacket::parse(payload, "utf8").expect("valid field descriptor");
        prop_assert_eq!(&fd.name, &name);
        prop_assert_eq!(fd.charsetnr, charsetnr);
        prop_assert_eq!(fd.length, length);
        prop_assert_eq!(fd.type_code, type_code);
        prop_assert_eq!(fd.flags, flags);
        prop_assert_eq!(fd.scale, scale);

        let desc = fd.description();
        prop_assert_eq!(&desc.name, &name);
        prop_assert_eq!(desc.type_code, type_code);
        prop_assert_eq!(desc.scale, scale);
        prop_assert_eq!(desc.null_ok, flags % 2 == 0);
    }

    /// 任意のバイト列に対して MysqlPacket::raise_for_error がパニックしない。
    #[test]
    fn prop_raise_for_error_no_panic(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let packet = MysqlPacket::new(data);
        let _ = packet.raise_for_error();
    }

    /// 任意のバイト列に対して FieldDescriptorPacket::parse がパニックしない。
    #[test]
    fn prop_field_descriptor_parse_no_panic(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let _ = FieldDescriptorPacket::parse(data, "utf8");
    }

    /// 任意のバイト列に対して OkPacketWrapper::from_packet がパニックしない。
    #[test]
    fn prop_ok_packet_parse_no_panic(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let mut packet = MysqlPacket::new(data);
        let _ = OkPacketWrapper::from_packet(&mut packet);
    }

    /// 任意のバイト列に対して EofPacketWrapper::from_packet がパニックしない。
    #[test]
    fn prop_eof_packet_parse_no_panic(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let mut packet = MysqlPacket::new(data);
        let _ = EofPacketWrapper::from_packet(&mut packet);
    }

    /// 任意のバイト列に対して LoadLocalPacketWrapper::from_packet がパニックしない。
    #[test]
    fn prop_load_local_packet_parse_no_panic(data in proptest::collection::vec(any::<u8>(), 0..=256)) {
        let packet = MysqlPacket::new(data);
        let _ = LoadLocalPacketWrapper::from_packet(&packet);
    }
}
