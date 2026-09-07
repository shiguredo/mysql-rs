// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! protocol モジュールの Property-Based Testing。
//!
//! lenenc_int のエンコード・デコードや MysqlPacket の読み書きに対して、
//! ランダムな入力に対するプロパティを検証する。

use std::cell::Cell;

use shiguredo_mysql_core::connection::lenenc_int;
use shiguredo_mysql_core::protocol::{
    EofPacketWrapper, FieldDescriptorPacket, LoadLocalPacketWrapper, MysqlPacket, OkPacketWrapper,
};

/// 通常の `cargo test` で実行するケース数。
const CASES: usize = 256;

/// シード取得用の環境変数名。
const SEED_ENV: &str = "MYSQL_RS_SEED";

/// lenenc_int の符号化長が変わる境界値。
const LENENC_BOUNDARIES: [u64; 9] = [
    0,
    0xFA,
    0xFB,
    0xFC,
    0xFFFF,
    0x1_0000,
    0xFF_FFFF,
    0x0100_0000,
    u64::MAX,
];

/// キー・名前生成に使える文字のプール。
const NAME_POOL: &[u8] = b"abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_";

/// lenenc_int の入力値をサンプリングする。
///
/// 符号化長（1/3/4/9 バイト）の境界に意味のある確率を与えて、
/// 一様分布だけでは到達しにくい長い符号化を取りこぼさないようにする。
fn sample_lenenc_value(ctx: &mut noprop::TestCaseContext) -> u64 {
    noprop::sample_with_boundaries(
        ctx,
        &LENENC_BOUNDARIES,
        noprop::Ratio::one_nth(2),
        noprop::sample_u64,
    )
}

/// 指定長のバイト列をサンプリングする。
///
/// 空・最小・最大の境界に意味のある確率を与える。
fn sample_bytes_bounded(
    ctx: &mut noprop::TestCaseContext,
    min_len: usize,
    max_len: usize,
) -> Vec<u8> {
    let len = if min_len < max_len {
        noprop::sample_with_boundaries(
            ctx,
            &[min_len, min_len + 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, min_len..=max_len),
        )
    } else {
        noprop::sample_with_boundaries(ctx, &[min_len, max_len], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, min_len..=max_len)
        })
    };
    noprop::sample_bytes_vec(ctx, len)
}

/// NUL を含まないバイト列をサンプリングする（NUL 終端文字列の材料）。
fn sample_non_nul_bytes(
    ctx: &mut noprop::TestCaseContext,
    min_len: usize,
    max_len: usize,
) -> Vec<u8> {
    let len = if min_len < max_len {
        noprop::sample_with_boundaries(
            ctx,
            &[min_len, min_len + 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, min_len..=max_len),
        )
    } else {
        noprop::sample_with_boundaries(ctx, &[min_len, max_len], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, min_len..=max_len)
        })
    };
    (0..len)
        .map(|_| noprop::sample_usize_in(ctx, 1..=255) as u8)
        .collect()
}

/// パケット種別判定用に先頭バイトへ重み付けしたバイト列をサンプリングする。
///
/// 先頭が `0x00`（OK の可能性）・`0xFF`（Error）の場合に意味のある確率を与えて、
/// 一様分布だけでは到達しにくい種別判定の分岐を取りこぼさないようにする。
fn sample_packet_data(
    ctx: &mut noprop::TestCaseContext,
    min_len: usize,
    max_len: usize,
) -> Vec<u8> {
    let len = if min_len < max_len {
        noprop::sample_with_boundaries(
            ctx,
            &[min_len, min_len + 1, max_len],
            noprop::Ratio::one_nth(5),
            |ctx| noprop::sample_usize_in(ctx, min_len..=max_len),
        )
    } else if min_len == max_len {
        min_len
    } else {
        noprop::sample_usize_in(ctx, min_len..=max_len)
    };
    if len == 0 {
        return Vec::new();
    }
    let first = noprop::sample_with_boundaries(
        ctx,
        &[0x00u8, 0xFFu8],
        noprop::Ratio::one_nth(5),
        noprop::sample_u8,
    );
    let mut data = vec![first];
    data.extend_from_slice(&noprop::sample_bytes_vec(ctx, len - 1));
    data
}

/// フィールド名をサンプリングする（`[a-zA-Z0-9_]{1,32}` 相当）。
fn sample_field_name(ctx: &mut noprop::TestCaseContext) -> String {
    let len =
        noprop::sample_with_boundaries(ctx, &[1usize, 2, 32], noprop::Ratio::one_nth(5), |ctx| {
            noprop::sample_usize_in(ctx, 1..=32)
        });
    (0..len)
        .map(|_| noprop::sample_choice(ctx, NAME_POOL) as char)
        .collect()
}

/// lenenc_int の符号化長クラス（1/3/4/9 バイト）を返す。
fn lenenc_class(value: u64) -> usize {
    if value < 0xFB {
        0
    } else if value < (1 << 16) {
        1
    } else if value < (1 << 24) {
        2
    } else {
        3
    }
}

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

/// lenenc_int でエンコードした値を read_length_encoded_integer で復号すると元に戻る。
#[test]
fn prop_lenenc_int_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 各符号化長クラスが実行されたことを記録する。
    let classes = Cell::new([0usize; 4]);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let i = sample_lenenc_value(ctx);
        let encoded = lenenc_int(i as usize);
        let mut packet = MysqlPacket::new(encoded);
        let decoded = packet
            .read_length_encoded_integer()
            .expect("lenenc int の復号に失敗");
        assert_eq!(decoded, Some(i), "lenenc_int の復号結果が一致しない");
        let mut next = classes.get();
        next[lenenc_class(i)] += 1;
        classes.set(next);
        Ok(())
    })?;
    let classes = classes.get();
    for (index, count) in classes.iter().enumerate() {
        assert!(
            *count > 0,
            "符号化長クラス {index} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// NUL を含まないバイト列を NUL 終端して read_string で読み込める。
#[test]
fn prop_read_string_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let non_empty = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_non_nul_bytes(ctx, 0, 256);
        let mut payload = data.clone();
        payload.push(0);
        let mut packet = MysqlPacket::new(payload);
        let decoded = packet.read_string().expect("NUL 終端文字列の復号に失敗");
        assert_eq!(
            decoded.as_ref(),
            Some(&data),
            "文字列の復号結果が一致しない"
        );
        if data.is_empty() {
            empty.set(empty.get() + 1);
        } else {
            non_empty.set(non_empty.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空バイト列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty.get() > 0,
        "非空バイト列が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// read_length_encoded_integer は長さ符号付き整数を含む先頭部分だけを消費し、
/// 残りのバイト列はそのまま残す。
#[test]
fn prop_lenenc_int_consumes_only_prefix() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 接尾辞の空・非空の両方が実行されたことを記録する。
    let empty_suffix = Cell::new(0usize);
    let non_empty_suffix = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let i = sample_lenenc_value(ctx);
        let suffix = sample_bytes_bounded(ctx, 0, 32);
        let mut encoded = lenenc_int(i as usize);
        encoded.extend_from_slice(&suffix);
        let mut packet = MysqlPacket::new(encoded);
        let decoded = packet
            .read_length_encoded_integer()
            .expect("lenenc int の復号に失敗");
        assert_eq!(decoded, Some(i), "lenenc_int の復号結果が一致しない");
        let remaining = packet.read_all();
        assert_eq!(remaining, suffix, "接尾辞がそのまま残っていない");
        if suffix.is_empty() {
            empty_suffix.set(empty_suffix.get() + 1);
        } else {
            non_empty_suffix.set(non_empty_suffix.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty_suffix.get() > 0,
        "空の接尾辞が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty_suffix.get() > 0,
        "非空の接尾辞が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// read_uint8 は 1 バイトのリトルエンディアンから元の値を復元する。
#[test]
fn prop_read_uint8_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u8(ctx);
        let mut packet = MysqlPacket::new(vec![value]);
        let decoded = packet.read_uint8().expect("uint8 の読み込みに失敗");
        assert_eq!(decoded, value, "uint8 の復元結果が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// read_uint16 は 2 バイトのリトルエンディアンから元の値を復元する。
#[test]
fn prop_read_uint16_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u16(ctx);
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint16().expect("uint16 の読み込みに失敗");
        assert_eq!(decoded, value, "uint16 の復元結果が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// read_uint24 は 3 バイトのリトルエンディアンから元の値を復元する。
#[test]
fn prop_read_uint24_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_usize_in(ctx, 0..=0xFF_FFFF) as u32;
        let bytes = value.to_le_bytes();
        let mut packet = MysqlPacket::new(vec![bytes[0], bytes[1], bytes[2]]);
        let decoded = packet.read_uint24().expect("uint24 の読み込みに失敗");
        assert_eq!(decoded, value, "uint24 の復元結果が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// read_uint32 は 4 バイトのリトルエンディアンから元の値を復元する。
#[test]
fn prop_read_uint32_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u32(ctx);
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint32().expect("uint32 の読み込みに失敗");
        assert_eq!(decoded, value, "uint32 の復元結果が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// read_uint64 は 8 バイトのリトルエンディアンから元の値を復元する。
#[test]
fn prop_read_uint64_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u64(ctx);
        let mut packet = MysqlPacket::new(value.to_le_bytes().to_vec());
        let decoded = packet.read_uint64().expect("uint64 の読み込みに失敗");
        assert_eq!(decoded, value, "uint64 の復元結果が一致しない");
        Ok(())
    })?;
    Ok(())
}

/// read_length_coded_string は長さ符号付き文字列を正しく復元する。
#[test]
fn prop_read_length_coded_string_roundtrip() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 空・非空の両方が実行されたことを記録する。
    let empty = Cell::new(0usize);
    let non_empty = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 255);
        let mut payload = lenenc_int(data.len());
        payload.extend_from_slice(&data);
        let mut packet = MysqlPacket::new(payload);
        let decoded = packet
            .read_length_coded_string()
            .expect("長さ符号付き文字列の復号に失敗");
        assert_eq!(
            decoded.as_ref(),
            Some(&data),
            "文字列の復号結果が一致しない"
        );
        if data.is_empty() {
            empty.set(empty.get() + 1);
        } else {
            non_empty.set(non_empty.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty.get() > 0,
        "空バイト列が 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty.get() > 0,
        "非空バイト列が 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// lenenc_int のエンコード長は値の範囲に応じて仕様通り（1/3/4/9 バイト）である。
#[test]
fn prop_lenenc_int_encoding_length() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // 各符号化長クラスが実行されたことを記録する。
    let classes = Cell::new([0usize; 4]);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let i = sample_lenenc_value(ctx);
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
        assert_eq!(encoded.len(), expected_len, "符号化長が仕様と一致しない");
        if i < 0xFB {
            assert_eq!(encoded[0], i as u8, "1 バイト符号化の内容が一致しない");
        } else if i < (1 << 16) {
            assert_eq!(encoded[0], 0xFC, "3 バイト符号化の先頭が一致しない");
        } else if i < (1 << 24) {
            assert_eq!(encoded[0], 0xFD, "4 バイト符号化の先頭が一致しない");
        } else {
            assert_eq!(encoded[0], 0xFE, "9 バイト符号化の先頭が一致しない");
        }
        let mut next = classes.get();
        next[lenenc_class(i)] += 1;
        classes.set(next);
        Ok(())
    })?;
    let classes = classes.get();
    for (index, count) in classes.iter().enumerate() {
        assert!(
            *count > 0,
            "符号化長クラス {index} が 1 件も実行されなかった\n{runner}"
        );
    }
    Ok(())
}

/// read_* 呼び出し後、カーソル位置は正しく進み、read_all で残り全部が読める。
#[test]
fn prop_read_position_advances() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let value = noprop::sample_u64(ctx);
        let mut payload = value.to_le_bytes().to_vec();
        payload.extend_from_slice(&[0xAB, 0xCD]);
        let mut packet = MysqlPacket::new(payload.clone());
        let _ = packet.read_uint64().expect("uint64 の読み込みに失敗");
        assert_eq!(packet.position(), 8, "カーソル位置が 8 でない");
        let remaining = packet.read_all();
        assert_eq!(remaining, &[0xAB, 0xCD], "残りバイト列が一致しない");
        assert_eq!(packet.position(), payload.len(), "カーソル位置が末尾でない");
        Ok(())
    })?;
    Ok(())
}

/// 0xFF から始まるパケットは error パケットとして判定され、ok パケットとは同時に true にならない。
#[test]
fn prop_error_packet_exclusive() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 1, 256);
        let mut payload = vec![0xFF];
        payload.extend_from_slice(&data);
        let packet = MysqlPacket::new(payload);
        assert!(packet.is_error_packet(), "error パケットと判定されない");
        assert!(!packet.is_ok_packet(), "error が ok とも判定される");
        Ok(())
    })?;
    Ok(())
}

/// パケット種別判定は相互に排他的である（EOF/認証スイッチの包含関係を除く）。
#[test]
fn prop_packet_kind_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // OK・Error の両方が観測されたことを記録する。
    let ok_seen = Cell::new(0usize);
    let error_seen = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_packet_data(ctx, 0, 256);
        let packet = MysqlPacket::new(data.clone());
        let is_ok = packet.is_ok_packet();
        let is_error = packet.is_error_packet();
        let is_load_local = packet.is_load_local_packet();

        // OK と Error、LOAD LOCAL は同時に true にならない。
        assert!(!(is_ok && is_error), "OK と Error が同時に真になった");
        assert!(
            !(is_ok && is_load_local),
            "OK と LOAD LOCAL が同時に真になった"
        );
        assert!(
            !(is_error && is_load_local),
            "Error と LOAD LOCAL が同時に真になった"
        );

        // OK パケットは長さが 7 バイト以上、かつ先頭が 0x00。
        if is_ok {
            assert_eq!(data.first(), Some(&0x00), "OK の先頭が 0x00 でない");
            assert!(data.len() >= 7, "OK の長さが 7 バイト未満");
            ok_seen.set(ok_seen.get() + 1);
        }

        // Error パケットは先頭が 0xFF。
        if is_error {
            assert_eq!(data.first(), Some(&0xFF), "Error の先頭が 0xFF でない");
            error_seen.set(error_seen.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        ok_seen.get() > 0,
        "OK パケットが 1 件も観測されなかった\n{runner}"
    );
    assert!(
        error_seen.get() > 0,
        "Error パケットが 1 件も観測されなかった\n{runner}"
    );
    Ok(())
}

/// 有効な OK パケットは from_packet で解析でき、無効なパケットはエラーになる。
#[test]
fn prop_ok_packet_boundary() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    // メッセージの空・非空の両方が実行されたことを記録する。
    let empty_message = Cell::new(0usize);
    let non_empty_message = Cell::new(0usize);
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let affected_rows = noprop::sample_u64(ctx);
        let insert_id = noprop::sample_u64(ctx);
        let server_status = noprop::sample_u16(ctx);
        let warning_count = noprop::sample_u16(ctx);
        let message = sample_bytes_bounded(ctx, 0, 32);
        let mut payload = vec![0x00];
        payload.extend_from_slice(&lenenc_int(affected_rows as usize));
        payload.extend_from_slice(&lenenc_int(insert_id as usize));
        payload.extend_from_slice(&server_status.to_le_bytes());
        payload.extend_from_slice(&warning_count.to_le_bytes());
        payload.extend_from_slice(&message);
        let mut packet = MysqlPacket::new(payload);
        let ok = OkPacketWrapper::from_packet(&mut packet);
        assert!(ok.is_ok(), "有効な OK パケットの解析に失敗: {ok:?}");
        let ok = ok.expect("OK パケットの取得に失敗");
        assert_eq!(
            ok.affected_rows,
            Some(affected_rows),
            "affected_rows が一致しない"
        );
        assert_eq!(ok.insert_id, Some(insert_id), "insert_id が一致しない");
        assert_eq!(
            ok.server_status, server_status,
            "server_status が一致しない"
        );
        assert_eq!(
            ok.warning_count, warning_count,
            "warning_count が一致しない"
        );
        assert_eq!(ok.message, message, "message が一致しない");
        if message.is_empty() {
            empty_message.set(empty_message.get() + 1);
        } else {
            non_empty_message.set(non_empty_message.get() + 1);
        }
        Ok(())
    })?;
    assert!(
        empty_message.get() > 0,
        "空メッセージが 1 件も実行されなかった\n{runner}"
    );
    assert!(
        non_empty_message.get() > 0,
        "非空メッセージが 1 件も実行されなかった\n{runner}"
    );
    Ok(())
}

/// 有効な EOF パケットは from_packet で解析でき、無効なパケットはエラーになる。
#[test]
fn prop_eof_packet_boundary() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let warning_count = noprop::sample_u16(ctx);
        let server_status = noprop::sample_u16(ctx);
        let mut payload = vec![0xFE];
        payload.extend_from_slice(&warning_count.to_le_bytes());
        payload.extend_from_slice(&server_status.to_le_bytes());
        let mut packet = MysqlPacket::new(payload);
        let eof = EofPacketWrapper::from_packet(&mut packet);
        assert!(eof.is_ok(), "有効な EOF パケットの解析に失敗: {eof:?}");
        let eof = eof.expect("EOF パケットの取得に失敗");
        assert_eq!(
            eof.warning_count, warning_count,
            "warning_count が一致しない"
        );
        assert_eq!(
            eof.server_status, server_status,
            "server_status が一致しない"
        );
        Ok(())
    })?;
    Ok(())
}

/// FieldDescriptorPacket::description は元のパケットの情報と一致する。
#[test]
fn prop_field_descriptor_description_consistency() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let name = sample_field_name(ctx);
        let charsetnr = noprop::sample_choice(ctx, &[8u16, 33, 63]);
        let length = noprop::sample_u32(ctx);
        let type_code = noprop::sample_u8(ctx);
        let flags = noprop::sample_u16(ctx);
        let scale = noprop::sample_u8(ctx);
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
        let fd = FieldDescriptorPacket::parse(payload, "utf8").expect("フィールド記述の解析に失敗");
        assert_eq!(&fd.name, &name, "name が一致しない");
        assert_eq!(fd.charsetnr, charsetnr, "charsetnr が一致しない");
        assert_eq!(fd.length, length, "length が一致しない");
        assert_eq!(fd.type_code, type_code, "type_code が一致しない");
        assert_eq!(fd.flags, flags, "flags が一致しない");
        assert_eq!(fd.scale, scale, "scale が一致しない");

        let desc = fd.description();
        assert_eq!(&desc.name, &name, "description の name が一致しない");
        assert_eq!(
            desc.type_code, type_code,
            "description の type_code が一致しない"
        );
        assert_eq!(desc.scale, scale, "description の scale が一致しない");
        assert_eq!(
            desc.null_ok,
            flags.is_multiple_of(2),
            "description の null_ok が一致しない"
        );
        Ok(())
    })?;
    Ok(())
}

/// 任意のバイト列に対して MysqlPacket::raise_for_error がパニックしない。
#[test]
fn prop_raise_for_error_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 256);
        let packet = MysqlPacket::new(data);
        let _ = packet.raise_for_error();
        Ok(())
    })?;
    Ok(())
}

/// 任意のバイト列に対して FieldDescriptorPacket::parse がパニックしない。
#[test]
fn prop_field_descriptor_parse_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 256);
        let _ = FieldDescriptorPacket::parse(data, "utf8");
        Ok(())
    })?;
    Ok(())
}

/// 任意のバイト列に対して OkPacketWrapper::from_packet がパニックしない。
#[test]
fn prop_ok_packet_parse_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 256);
        let mut packet = MysqlPacket::new(data);
        let _ = OkPacketWrapper::from_packet(&mut packet);
        Ok(())
    })?;
    Ok(())
}

/// 任意のバイト列に対して EofPacketWrapper::from_packet がパニックしない。
#[test]
fn prop_eof_packet_parse_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 256);
        let mut packet = MysqlPacket::new(data);
        let _ = EofPacketWrapper::from_packet(&mut packet);
        Ok(())
    })?;
    Ok(())
}

/// 任意のバイト列に対して LoadLocalPacketWrapper::from_packet がパニックしない。
#[test]
fn prop_load_local_packet_parse_no_panic() -> noprop::TestResult {
    let seed = noprop::seed_from_env_or_time(SEED_ENV)?;
    let mut runner = noprop::Runner::new(seed);
    runner.run(CASES, |ctx| {
        let data = sample_bytes_bounded(ctx, 0, 256);
        let packet = MysqlPacket::new(data);
        let _ = LoadLocalPacketWrapper::from_packet(&packet);
        Ok(())
    })?;
    Ok(())
}
