// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

use shiguredo_mysql::connection::lenenc_int;
use shiguredo_mysql::protocol::MysqlPacket;

#[test]
fn test_lenenc_int_small() {
    assert_eq!(lenenc_int(0), vec![0x00], "0 は 1 バイトで表現するべき");
    assert_eq!(
        lenenc_int(250),
        vec![0xFA],
        "250 以下は 1 バイトで表現するべき"
    );
}

#[test]
fn test_lenenc_int_medium() {
    assert_eq!(
        lenenc_int(0xFB),
        vec![0xFC, 0xFB, 0x00],
        "0xFB は 2 バイト長前置きで表現するべき"
    );
    assert_eq!(
        lenenc_int(0xFFFF),
        vec![0xFC, 0xFF, 0xFF],
        "0xFFFF は 2 バイト長前置きで表現するべき"
    );
}

#[test]
fn test_lenenc_int_large() {
    assert_eq!(
        lenenc_int(0x10000),
        vec![0xFD, 0x00, 0x00, 0x01],
        "0x10000 は 3 バイト長前置きで表現するべき"
    );
    assert_eq!(
        lenenc_int(0xFFFFFF),
        vec![0xFD, 0xFF, 0xFF, 0xFF],
        "0xFFFFFF は 3 バイト長前置きで表現するべき"
    );
}

#[test]
fn test_lenenc_int_huge() {
    assert_eq!(
        lenenc_int(0x1000000),
        vec![0xFE, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x00],
        "0x1000000 は 8 バイト長前置きで表現するべき"
    );
}

#[test]
fn test_packet_read_uints() {
    let data = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet.read_uint8().expect("uint8 の読み込みに失敗"),
        0x01,
        "uint8 の値が一致するべき"
    );
    assert_eq!(
        packet.read_uint16().expect("uint16 の読み込みに失敗"),
        0x0302,
        "uint16 はリトルエンディアンで読み込むべき"
    );
    assert_eq!(
        packet.read_uint24().expect("uint24 の読み込みに失敗"),
        0x060504,
        "uint24 はリトルエンディアンで読み込むべき"
    );
    assert_eq!(
        packet.read_uint16().expect("uint16 の読み込みに失敗"),
        0x0807,
        "uint16 の値が一致するべき"
    );
}

#[test]
fn test_packet_read_length_encoded_integer() {
    let data = vec![0x05];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_length_encoded_integer()
            .expect("length-encoded integer の読み込みに失敗"),
        Some(5),
        "1 バイト値はそのまま解釈するべき"
    );

    let data = vec![0xFC, 0x34, 0x12];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_length_encoded_integer()
            .expect("2 バイト length-encoded integer の読み込みに失敗"),
        Some(0x1234),
        "0xFC 前置きは 2 バイト整数として解釈するべき"
    );

    let data = vec![0xFD, 0x34, 0x12, 0x00];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_length_encoded_integer()
            .expect("3 バイト length-encoded integer の読み込みに失敗"),
        Some(0x0000_1234),
        "0xFD 前置きは 3 バイト整数として解釈するべき"
    );

    let data = vec![0xFE, 0x34, 0x12, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_length_encoded_integer()
            .expect("8 バイト length-encoded integer の読み込みに失敗"),
        Some(0x1234),
        "0xFE 前置きは 8 バイト整数として解釈するべき"
    );

    let data = vec![0xFB];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_length_encoded_integer()
            .expect("NULL length-encoded integer の読み込みに失敗"),
        None,
        "0xFB は NULL として解釈するべき"
    );
}

#[test]
fn test_packet_read_string() {
    let data = vec![b'h', b'e', b'l', b'l', b'o', 0, b'w'];
    let mut packet = MysqlPacket::new(data);
    assert_eq!(
        packet
            .read_string()
            .expect("NUL 終端文字列の読み込みに失敗"),
        Some(b"hello".to_vec()),
        "NUL 終端文字列が正しく読めるべき"
    );
    assert_eq!(
        packet
            .read_string()
            .expect("NUL 終端文字列の読み込みに失敗"),
        None,
        "NUL が無い場合は None を返すべき"
    );
}
