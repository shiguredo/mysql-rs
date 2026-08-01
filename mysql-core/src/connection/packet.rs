// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL パケットストリーム。

use crate::constants::client_error;
use crate::error::{Error, Result};
use crate::protocol::MysqlPacket;
use std::collections::VecDeque;

/// 最大パケット長。
pub const MAX_PACKET_LEN: usize = 2_usize.pow(24) - 1;

/// 圧縮パケットヘッダー長。
const COMPRESSED_HEADER_LEN: usize = 7;

/// 3 バイトのリトルエンディアン整数を読み取る。
fn read_int3(bytes: &[u8]) -> usize {
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]) as usize
}

/// 3 バイトのリトルエンディアン整数を書き込む。
fn write_int3(value: usize) -> [u8; 3] {
    let bytes = (value as u32).to_le_bytes();
    [bytes[0], bytes[1], bytes[2]]
}

/// パケット入出力を管理する構造体。
pub struct PacketStream {
    /// 送信すべきパケットのキュー。
    pub send_queue: VecDeque<Vec<u8>>,
    /// 受信中の生バイト列（圧縮時は圧縮パケット単位のバイト列）。
    recv_buffer: Vec<u8>,
    /// 解凍済みの標準パケットバイト列（不完全なパケットを含む）。
    decompressed_buffer: Vec<u8>,
    /// 受信済みパケットのキュー。
    pub recv_queue: VecDeque<MysqlPacket>,
    /// 次に期待する標準パケットのシーケンス番号。
    next_seq_id: u8,
    /// 次に期待する圧縮パケットのシーケンス番号。
    next_compressed_seq_id: u8,
    /// 許容する最大パケットサイズ。
    max_allowed_packet: usize,
    /// 圧縮が有効かどうか。
    compressed: bool,
}

impl PacketStream {
    /// 新規のパケットストリームを作成する。
    pub fn new(max_allowed_packet: usize) -> Self {
        Self {
            send_queue: VecDeque::new(),
            recv_buffer: Vec::new(),
            decompressed_buffer: Vec::new(),
            recv_queue: VecDeque::new(),
            next_seq_id: 0,
            next_compressed_seq_id: 0,
            max_allowed_packet,
            compressed: false,
        }
    }

    /// 圧縮が有効かどうか。
    pub fn is_compressed(&self) -> bool {
        self.compressed
    }

    /// 圧縮を有効にする。
    pub fn enable_compression(&mut self) {
        self.compressed = true;
        self.next_compressed_seq_id = 0;
    }

    /// 次のシーケンス番号を取得する。
    pub fn next_seq_id(&self) -> u8 {
        self.next_seq_id
    }

    /// シーケンス番号を設定する。
    pub fn set_next_seq_id(&mut self, seq: u8) {
        self.next_seq_id = seq;
    }

    /// 標準パケットと圧縮パケットのシーケンス番号を両方とも 0 にリセットする。
    pub fn reset_seq_id(&mut self) {
        self.next_seq_id = 0;
        self.next_compressed_seq_id = 0;
    }

    /// 強制的にキューをクリアする。
    pub fn force_close(&mut self) {
        self.send_queue.clear();
        self.recv_queue.clear();
        self.recv_buffer.clear();
        self.decompressed_buffer.clear();
    }

    /// ペイロードを標準 MySQL パケットに組み立てる。
    fn build_plain_packets(&mut self, payload: &[u8]) -> Vec<u8> {
        let mut data = Vec::with_capacity(payload.len() + 4);
        let mut offset = 0;

        loop {
            let remaining = payload.len() - offset;
            let chunk_len = remaining.min(MAX_PACKET_LEN);
            let chunk = if chunk_len == 0 {
                &payload[offset..offset]
            } else {
                &payload[offset..offset + chunk_len]
            };

            data.extend_from_slice(&write_int3(chunk.len()));
            data.push(self.next_seq_id);
            data.extend_from_slice(chunk);
            self.next_seq_id = self.next_seq_id.wrapping_add(1);

            if chunk_len < MAX_PACKET_LEN {
                break;
            }
            offset += chunk_len;
        }

        data
    }

    /// パケットを送信キューに追加する。
    pub fn write_packet(&mut self, payload: &[u8]) -> Result<()> {
        if self.compressed {
            self.write_compressed_packet(payload)
        } else {
            let data = self.build_plain_packets(payload);
            self.send_queue.push_back(data);
            Ok(())
        }
    }

    /// 圧縮有効時にペイロードを圧縮パケットとして送信キューに追加する。
    fn write_compressed_packet(&mut self, payload: &[u8]) -> Result<()> {
        let plain = self.build_plain_packets(payload);
        let mut offset = 0;

        while offset < plain.len() {
            let end = (offset + MAX_PACKET_LEN).min(plain.len());
            let chunk = &plain[offset..end];
            let uncompressed_len = chunk.len();

            let compressed =
                noflate::zlib::compress(chunk).map_err(|e| Error::OperationalError {
                    code: client_error::CR_UNKNOWN_ERROR,
                    message: format!("Failed to compress MySQL packet: {e}"),
                })?;

            let (body, uncompressed_field) = if compressed.len() >= uncompressed_len {
                (chunk.to_vec(), 0)
            } else {
                (compressed, uncompressed_len)
            };

            if body.len() > MAX_PACKET_LEN {
                return Err(Error::OperationalError {
                    code: client_error::CR_NET_PACKET_TOO_LARGE,
                    message: format!(
                        "Got compressed packet larger than 'max_allowed_packet' bytes ({} > {})",
                        body.len(),
                        MAX_PACKET_LEN
                    ),
                });
            }

            let mut packet = Vec::with_capacity(COMPRESSED_HEADER_LEN + body.len());
            packet.extend_from_slice(&write_int3(body.len()));
            packet.push(self.next_compressed_seq_id);
            packet.extend_from_slice(&write_int3(uncompressed_field));
            packet.extend_from_slice(&body);
            self.send_queue.push_back(packet);

            self.next_compressed_seq_id = self.next_compressed_seq_id.wrapping_add(1);
            offset = end;
        }

        Ok(())
    }

    /// 受信した生バイト列を消費してパケットを組み立て、recv_queue に追加する。
    ///
    /// 戻り値は追加された論理パケット数。
    pub fn feed_bytes(&mut self, data: &[u8]) -> Result<usize> {
        self.recv_buffer.extend_from_slice(data);

        if self.compressed {
            self.feed_compressed_packets()?;
        }

        let mut buffer = std::mem::take(&mut self.decompressed_buffer);
        let packets_added = self.parse_plain_packets(&mut buffer)?;
        self.decompressed_buffer = buffer;

        if !self.compressed {
            let mut buffer = std::mem::take(&mut self.recv_buffer);
            let packets_added_raw = self.parse_plain_packets(&mut buffer)?;
            self.recv_buffer = buffer;
            return Ok(packets_added + packets_added_raw);
        }

        Ok(packets_added)
    }

    /// 圧縮パケットを recv_buffer から取り出して解凍し、decompressed_buffer に格納する。
    fn feed_compressed_packets(&mut self) -> Result<()> {
        let max_packet = self.max_allowed_packet;

        loop {
            if self.recv_buffer.len() < COMPRESSED_HEADER_LEN {
                break;
            }

            let header = &self.recv_buffer[..COMPRESSED_HEADER_LEN];
            let compressed_len = read_int3(&header[..3]);
            let compressed_seq = header[3];
            let uncompressed_len = read_int3(&header[4..]);

            if compressed_len > MAX_PACKET_LEN {
                self.force_close();
                return Err(Error::OperationalError {
                    code: client_error::CR_NET_PACKET_TOO_LARGE,
                    message: format!(
                        "Got compressed packet larger than 'max_allowed_packet' bytes ({} > {})",
                        compressed_len, MAX_PACKET_LEN
                    ),
                });
            }

            let total_len = COMPRESSED_HEADER_LEN + compressed_len;
            if self.recv_buffer.len() < total_len {
                break;
            }

            if compressed_seq != self.next_compressed_seq_id {
                self.force_close();
                if compressed_seq == 0 {
                    return Err(Error::OperationalError {
                        code: client_error::CR_SERVER_LOST,
                        message: "Lost connection to MySQL server during query".to_string(),
                    });
                }
                return Err(Error::InternalError {
                    code: client_error::CR_COMMANDS_OUT_OF_SYNC,
                    message: format!(
                        "Compressed packet sequence number wrong - got {} expected {}",
                        compressed_seq, self.next_compressed_seq_id
                    ),
                });
            }
            self.next_compressed_seq_id = self.next_compressed_seq_id.wrapping_add(1);

            let payload = &self.recv_buffer[COMPRESSED_HEADER_LEN..total_len];
            if uncompressed_len == 0 {
                self.decompressed_buffer.extend_from_slice(payload);
            } else {
                let decompressed =
                    noflate::zlib::decompress(payload).map_err(|e| Error::OperationalError {
                        code: client_error::CR_UNKNOWN_ERROR,
                        message: format!("Failed to decompress MySQL packet: {e}"),
                    })?;
                if decompressed.len() != uncompressed_len {
                    self.force_close();
                    return Err(Error::OperationalError {
                        code: client_error::CR_UNKNOWN_ERROR,
                        message: format!(
                            "Decompressed length mismatch: got {} expected {}",
                            decompressed.len(),
                            uncompressed_len
                        ),
                    });
                }
                self.decompressed_buffer.extend_from_slice(&decompressed);
            }

            if self.decompressed_buffer.len() > max_packet {
                self.force_close();
                return Err(Error::OperationalError {
                    code: client_error::CR_NET_PACKET_TOO_LARGE,
                    message: format!(
                        "Got packet larger than 'max_allowed_packet' bytes ({} > {})",
                        self.decompressed_buffer.len(),
                        max_packet
                    ),
                });
            }

            self.recv_buffer.drain(..total_len);
        }

        Ok(())
    }

    /// 標準 MySQL パケットをバッファから取り出して recv_queue に追加する。
    fn parse_plain_packets(&mut self, buffer: &mut Vec<u8>) -> Result<usize> {
        let mut packets_added = 0;
        let max_packet = self.max_allowed_packet;

        loop {
            if buffer.len() < 4 {
                break;
            }

            let header = &buffer[..4];
            let payload_len = read_int3(&header[..3]);
            if payload_len > max_packet {
                self.force_close();
                return Err(Error::OperationalError {
                    code: client_error::CR_NET_PACKET_TOO_LARGE,
                    message: format!(
                        "Got packet larger than 'max_allowed_packet' bytes ({} > {})",
                        payload_len, max_packet
                    ),
                });
            }
            let total_len = 4 + payload_len;
            if buffer.len() < total_len {
                break;
            }

            // 小パケットを結合して一つの論理パケットにする。
            let mut buff = Vec::new();
            loop {
                if buffer.len() < 4 {
                    break;
                }
                let header = &buffer[..4];
                let chunk_payload_len = read_int3(&header[..3]);
                if chunk_payload_len > max_packet {
                    self.force_close();
                    return Err(Error::OperationalError {
                        code: client_error::CR_NET_PACKET_TOO_LARGE,
                        message: format!(
                            "Got packet larger than 'max_allowed_packet' bytes ({} > {})",
                            chunk_payload_len, max_packet
                        ),
                    });
                }
                let chunk_total_len = 4 + chunk_payload_len;
                if buffer.len() < chunk_total_len {
                    break;
                }
                let packet_number = header[3];

                if packet_number != self.next_seq_id {
                    self.force_close();
                    if packet_number == 0 {
                        return Err(Error::OperationalError {
                            code: client_error::CR_SERVER_LOST,
                            message: "Lost connection to MySQL server during query".to_string(),
                        });
                    }
                    return Err(Error::InternalError {
                        code: client_error::CR_COMMANDS_OUT_OF_SYNC,
                        message: format!(
                            "Packet sequence number wrong - got {} expected {}",
                            packet_number, self.next_seq_id
                        ),
                    });
                }
                self.next_seq_id = self.next_seq_id.wrapping_add(1);

                if buff.len().saturating_add(chunk_payload_len) > max_packet {
                    self.force_close();
                    return Err(Error::OperationalError {
                        code: client_error::CR_NET_PACKET_TOO_LARGE,
                        message: format!(
                            "Got packet larger than 'max_allowed_packet' bytes ({} > {})",
                            buff.len().saturating_add(chunk_payload_len),
                            max_packet
                        ),
                    });
                }
                buff.extend_from_slice(&buffer[4..chunk_total_len]);
                buffer.drain(..chunk_total_len);

                if chunk_payload_len < MAX_PACKET_LEN {
                    break;
                }
            }

            let packet = MysqlPacket::new(buff);
            if packet.is_error_packet() {
                return Err(packet.raise_for_error());
            }
            self.recv_queue.push_back(packet);
            packets_added += 1;
        }

        Ok(packets_added)
    }

    /// 受信済みパケットキューから一つ取り出す。
    pub fn read_packet(&mut self) -> Result<MysqlPacket> {
        self.recv_queue.pop_front().ok_or(Error::NeedMoreData)
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    pub(crate) fn build_packet(seq: u8, payload: &[u8]) -> Vec<u8> {
        let mut packet = Vec::new();
        packet.extend_from_slice(&write_int3(payload.len()));
        packet.push(seq);
        packet.extend_from_slice(payload);
        packet
    }

    #[test]
    fn test_feed_bytes_max_allowed_packet() {
        let mut stream = PacketStream::new(1024);
        let payload = vec![0u8; 1025];
        let packet = build_packet(0, &payload);
        let result = stream.feed_bytes(&packet);
        assert!(result.is_err());
    }

    #[test]
    fn test_write_and_read_compressed_packet() {
        let mut stream = PacketStream::new(MAX_PACKET_LEN);
        stream.enable_compression();

        let payload = b"SELECT 1";
        stream.write_packet(payload).unwrap();

        let sent = stream.send_queue.pop_front().unwrap();
        // 圧縮ヘッダー + 圧縮データを自分自身に送り返してみる。
        // 送信と受信で同じストリームを使うため、受信前にシーケンス番号をリセットする。
        stream.next_seq_id = 0;
        stream.next_compressed_seq_id = 0;
        stream.feed_bytes(&sent).unwrap();
        let packet = stream.read_packet().unwrap();
        assert_eq!(packet.get_all_data(), payload);
    }

    #[test]
    fn test_uncompressed_small_packet_when_compression_enabled() {
        let mut stream = PacketStream::new(MAX_PACKET_LEN);
        stream.enable_compression();

        // 圧縮効果がないほど短いペイロードはそのまま送信される。
        let payload = b"\x01";
        stream.write_packet(payload).unwrap();

        let sent = stream.send_queue.pop_front().unwrap();
        stream.next_seq_id = 0;
        stream.next_compressed_seq_id = 0;
        stream.feed_bytes(&sent).unwrap();
        let packet = stream.read_packet().unwrap();
        assert_eq!(packet.get_all_data(), payload);
    }
}
