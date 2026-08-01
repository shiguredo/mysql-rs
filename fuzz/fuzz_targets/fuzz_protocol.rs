// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

#![no_main]

use libfuzzer_sys::fuzz_target;
use shiguredo_mysql_core::connection::lenenc_int;
use shiguredo_mysql_core::protocol::{
    EofPacketWrapper, FieldDescriptorPacket, LoadLocalPacketWrapper, MysqlPacket,
    OkPacketWrapper,
};

fuzz_target!(|data: &[u8]| {
    // 任意のバイト列を MysqlPacket としてパースしてもパニックしないことを検証する。
    let mut packet = MysqlPacket::new(data.to_vec());
    let _ = packet.read_length_encoded_integer();
    let _ = packet.read_string();
    let _ = packet.read_uint8();
    let _ = packet.read_uint16();
    let _ = packet.read_uint24();
    let _ = packet.read_uint32();
    let _ = packet.read_uint64();
    // カーソル位置は常にデータ長以下に留まる。
    assert!(packet.position() <= data.len());

    // 各種パケットラッパーのパースもパニックしない。
    let mut packet = MysqlPacket::new(data.to_vec());
    let _ = OkPacketWrapper::from_packet(&mut packet);

    let mut packet = MysqlPacket::new(data.to_vec());
    let _ = EofPacketWrapper::from_packet(&mut packet);

    let packet = MysqlPacket::new(data.to_vec());
    let _ = LoadLocalPacketWrapper::from_packet(&packet);

    let _ = FieldDescriptorPacket::parse(data.to_vec(), "utf8");

    // パケット種別判定の排他性を検証。
    let packet = MysqlPacket::new(data.to_vec());
    let is_ok = packet.is_ok_packet();
    let is_error = packet.is_error_packet();
    let is_load_local = packet.is_load_local_packet();
    assert!(!(is_ok && is_error));
    assert!(!(is_ok && is_load_local));
    assert!(!(is_error && is_load_local));

    // lenenc_int のエンコード長が仕様通りであることを検証。
    let value = data.len() as u64;
    let encoded = lenenc_int(value as usize);
    let expected_len = if value < 0xFB {
        1
    } else if value < (1 << 16) {
        3
    } else if value < (1 << 24) {
        4
    } else {
        9
    };
    assert_eq!(encoded.len(), expected_len);
});
