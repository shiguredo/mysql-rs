// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL クライアント・サーバープロトコルの低レベル実装。

use crate::charset::mblength;
use crate::constants::field_type;
use crate::constants::server_status;
use crate::error::{Error, raise_mysql_exception};

/// NULL 値を表す長さ符号付き整数のヘッダー。
pub const NULL_COLUMN: u8 = 251;
/// 1 バイトで表現できる最大値の閾値。
pub const UNSIGNED_CHAR_COLUMN: u8 = 251;
/// 2 バイト長符号付き整数のヘッダー。
pub const UNSIGNED_SHORT_COLUMN: u8 = 252;
/// 3 バイト長符号付き整数のヘッダー。
pub const UNSIGNED_INT24_COLUMN: u8 = 253;
/// 8 バイト長符号付き整数のヘッダー。
pub const UNSIGNED_INT64_COLUMN: u8 = 254;

/// MySQL パケットを表現する構造体。
#[derive(Debug, Clone)]
pub struct MysqlPacket {
    position: usize,
    data: Vec<u8>,
}

impl MysqlPacket {
    /// ペイロードからパケットを作成する。
    pub fn new(data: Vec<u8>) -> Self {
        Self { position: 0, data }
    }

    /// 内部データ全体を取得する。
    pub fn get_all_data(&self) -> &[u8] {
        &self.data
    }

    /// 現在のカーソル位置を取得する。
    pub fn position(&self) -> usize {
        self.position
    }

    /// 指定バイト数を読み込み、カーソルを進める。
    pub fn read(&mut self, size: usize) -> crate::error::Result<&[u8]> {
        let end = self
            .position
            .checked_add(size)
            .ok_or_else(|| Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!(
                    "Read size overflow: Position={}, Size={}",
                    self.position, size
                ),
            })?;
        if end > self.data.len() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!(
                    "Result length not requested length: Expected={}, Actual={}, Position={}, Data Length={}, Data={:?}",
                    size,
                    self.data.len().saturating_sub(self.position),
                    self.position,
                    self.data.len(),
                    self.data
                ),
            });
        }
        let result = &self.data[self.position..end];
        self.position = end;
        Ok(result)
    }

    /// 残りのデータをすべて読み込む。
    pub fn read_all(&mut self) -> &[u8] {
        let result = &self.data[self.position..];
        self.position = self.data.len();
        result
    }

    /// カーソルを指定バイト数進める。
    pub fn advance(&mut self, length: usize) -> crate::error::Result<()> {
        let new_position =
            self.position
                .checked_add(length)
                .ok_or_else(|| Error::InternalError {
                    code: crate::constants::client_error::CR_MALFORMED_PACKET,
                    message: format!(
                        "Advance amount overflow: Position={}, Length={}",
                        self.position, length
                    ),
                })?;
        if new_position > self.data.len() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!(
                    "Invalid advance amount ({}) for cursor. Position={}",
                    length, new_position
                ),
            });
        }
        self.position = new_position;
        Ok(())
    }

    /// カーソルを指定位置に戻す。
    pub fn rewind(&mut self, position: usize) -> crate::error::Result<()> {
        if position > self.data.len() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!("Invalid position to rewind cursor to: {}", position),
            });
        }
        self.position = position;
        Ok(())
    }

    /// 1 バイト符号なし整数を読み込む。
    pub fn read_uint8(&mut self) -> crate::error::Result<u8> {
        let bytes = self.read(1)?;
        Ok(bytes[0])
    }

    /// 2 バイト符号なし整数（リトルエンディアン）を読み込む。
    pub fn read_uint16(&mut self) -> crate::error::Result<u16> {
        let bytes = self.read(2)?;
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    /// 3 バイト符号なし整数（リトルエンディアン）を読み込む。
    pub fn read_uint24(&mut self) -> crate::error::Result<u32> {
        let bytes = self.read(3)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], 0]))
    }

    /// 4 バイト符号なし整数（リトルエンディアン）を読み込む。
    pub fn read_uint32(&mut self) -> crate::error::Result<u32> {
        let bytes = self.read(4)?;
        Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }

    /// 8 バイト符号なし整数（リトルエンディアン）を読み込む。
    pub fn read_uint64(&mut self) -> crate::error::Result<u64> {
        let bytes = self.read(8)?;
        Ok(u64::from_le_bytes([
            bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
        ]))
    }

    /// NUL 終端文字列を読み込む。
    ///
    /// NUL が見つからない場合は `None` を返し、カーソル位置は進めない。
    pub fn read_string(&mut self) -> crate::error::Result<Option<Vec<u8>>> {
        let end_pos = self.data[self.position..]
            .iter()
            .position(|&b| b == 0)
            .map(|p| self.position + p);
        match end_pos {
            Some(end) => {
                let result = self.data[self.position..end].to_vec();
                self.position = end + 1;
                Ok(Some(result))
            }
            None => Ok(None),
        }
    }

    /// 長さ符号付き整数を読み込む。
    pub fn read_length_encoded_integer(&mut self) -> crate::error::Result<Option<u64>> {
        let c = self.read_uint8()?;
        if c == NULL_COLUMN {
            return Ok(None);
        }
        if c < UNSIGNED_CHAR_COLUMN {
            return Ok(Some(u64::from(c)));
        }
        match c {
            UNSIGNED_SHORT_COLUMN => Ok(Some(u64::from(self.read_uint16()?))),
            UNSIGNED_INT24_COLUMN => Ok(Some(u64::from(self.read_uint24()?))),
            UNSIGNED_INT64_COLUMN => Ok(Some(self.read_uint64()?)),
            _ => Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!("Invalid length encoded integer header: {}", c),
            }),
        }
    }

    /// 長さ符号付き文字列を読み込む。
    pub fn read_length_coded_string(&mut self) -> crate::error::Result<Option<Vec<u8>>> {
        let length = self.read_length_encoded_integer()?;
        match length {
            None => Ok(None),
            Some(len) => Ok(Some(self.read(len as usize)?.to_vec())),
        }
    }

    /// OK パケットかどうかを判定する。
    pub fn is_ok_packet(&self) -> bool {
        self.data.first() == Some(&0) && self.data.len() >= 7
    }

    /// EOF パケットかどうかを判定する。
    pub fn is_eof_packet(&self) -> bool {
        // 0xFE は length-encoded integer の 8 バイト整数ヘッダーでもある。
        // そのため EOF パケットは 9 バイト未満であることを併せて確認する。
        self.data.first() == Some(&0xFE) && self.data.len() < 9
    }

    /// 認証スイッチ要求パケットかどうかを判定する。
    pub fn is_auth_switch_request(&self) -> bool {
        self.data.first() == Some(&0xFE) && self.data.len() >= 8
    }

    /// 追加認証データパケットかどうかを判定する。
    pub fn is_extra_auth_data(&self) -> bool {
        self.data.first() == Some(&1)
    }

    /// 結果セットパケットかどうかを判定する。
    pub fn is_resultset_packet(&self) -> bool {
        matches!(self.data.first(), Some(&n) if (1..=250).contains(&n))
    }

    /// LOAD LOCAL パケットかどうかを判定する。
    pub fn is_load_local_packet(&self) -> bool {
        self.data.first() == Some(&0xFB)
    }

    /// エラーパケットかどうかを判定する。
    pub fn is_error_packet(&self) -> bool {
        self.data.first() == Some(&0xFF)
    }

    /// エラーパケットであればエラーを発生させる。
    pub fn check_error(&self) -> crate::error::Result<()> {
        if self.is_error_packet() {
            Err(raise_mysql_exception(&self.data))
        } else {
            Ok(())
        }
    }

    /// エラーパケットとしてエラーを発生させる。
    pub fn raise_for_error(&self) -> Error {
        raise_mysql_exception(&self.data)
    }
}

/// カラム記述情報。
#[derive(Debug, Clone)]
pub struct ColumnDescription {
    pub name: String,
    pub type_code: u8,
    pub internal_size: u32,
    pub precision: u32,
    pub scale: u8,
    pub null_ok: bool,
}

/// フィールド記述子パケット。
#[derive(Debug, Clone)]
pub struct FieldDescriptorPacket {
    pub catalog: Option<Vec<u8>>,
    pub db: Option<Vec<u8>>,
    pub table_name: String,
    pub org_table: String,
    pub name: String,
    pub org_name: String,
    pub charsetnr: u16,
    pub length: u32,
    pub type_code: u8,
    pub flags: u16,
    pub scale: u8,
}

impl FieldDescriptorPacket {
    /// ペイロードからフィールド記述子を解析する。
    pub fn parse(data: Vec<u8>, encoding: &str) -> crate::error::Result<Self> {
        let mut packet = MysqlPacket::new(data);
        let catalog = packet.read_length_coded_string()?;
        let db = packet.read_length_coded_string()?;
        let table_name = decode_or_empty(packet.read_length_coded_string()?, encoding)?;
        let org_table = decode_or_empty(packet.read_length_coded_string()?, encoding)?;
        let name = decode_or_empty(packet.read_length_coded_string()?, encoding)?;
        let org_name = decode_or_empty(packet.read_length_coded_string()?, encoding)?;

        // 1 バイトのフィラーは 0x0c であることを確認する。
        let filler = packet.read_uint8()?;
        if filler != 0x0c {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: format!(
                    "Invalid field descriptor filler: expected 0x0c, got 0x{:02x}",
                    filler
                ),
            });
        }
        let charsetnr = packet.read_uint16()?;
        let length = packet.read_uint32()?;
        let type_code = packet.read_uint8()?;
        let flags = packet.read_uint16()?;
        let scale = packet.read_uint8()?;

        Ok(Self {
            catalog,
            db,
            table_name,
            org_table,
            name,
            org_name,
            charsetnr,
            length,
            type_code,
            flags,
            scale,
        })
    }

    /// カラム記述情報を返す。
    pub fn description(&self) -> ColumnDescription {
        let column_length = self.get_column_length();
        ColumnDescription {
            name: self.name.clone(),
            type_code: self.type_code,
            internal_size: column_length,
            precision: column_length,
            scale: self.scale,
            null_ok: self.flags.is_multiple_of(2),
        }
    }

    /// カラム長を取得する。
    pub fn get_column_length(&self) -> u32 {
        if self.type_code == field_type::VAR_STRING {
            let mblen = mblength(self.charsetnr);
            return self.length / mblen as u32;
        }
        self.length
    }
}

fn decode_or_empty(data: Option<Vec<u8>>, encoding: &str) -> crate::error::Result<String> {
    match data {
        Some(bytes) => Ok(encoding_rs::Encoding::for_label(encoding.as_bytes())
            .unwrap_or(encoding_rs::UTF_8)
            .decode(&bytes)
            .0
            .to_string()),
        None => Ok(String::new()),
    }
}

/// OK パケットの内容をラップする構造体。
#[derive(Debug, Clone)]
pub struct OkPacketWrapper {
    pub affected_rows: Option<u64>,
    pub insert_id: Option<u64>,
    pub server_status: u16,
    pub warning_count: u16,
    pub message: Vec<u8>,
    pub has_next: bool,
}

impl OkPacketWrapper {
    /// OK パケットから内容を抽出する。
    pub fn from_packet(packet: &mut MysqlPacket) -> crate::error::Result<Self> {
        if !packet.is_ok_packet() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: "Cannot create OkPacketWrapper from invalid packet type".to_string(),
            });
        }
        packet.advance(1)?;
        let affected_rows = packet.read_length_encoded_integer()?;
        let insert_id = packet.read_length_encoded_integer()?;
        let server_status = packet.read_uint16()?;
        let warning_count = packet.read_uint16()?;
        let message = packet.read_all().to_vec();
        let has_next = (server_status & server_status::SERVER_MORE_RESULTS_EXISTS) != 0;

        Ok(Self {
            affected_rows,
            insert_id,
            server_status,
            warning_count,
            message,
            has_next,
        })
    }
}

/// EOF パケットの内容をラップする構造体。
#[derive(Debug, Clone)]
pub struct EofPacketWrapper {
    pub warning_count: u16,
    pub server_status: u16,
    pub has_next: bool,
}

impl EofPacketWrapper {
    /// EOF パケットから内容を抽出する。
    pub fn from_packet(packet: &mut MysqlPacket) -> crate::error::Result<Self> {
        if !packet.is_eof_packet() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: "Cannot create EofPacketWrapper from invalid packet type".to_string(),
            });
        }
        packet.advance(1)?;
        let warning_count = packet.read_uint16()?;
        let server_status = packet.read_uint16()?;
        let has_next = (server_status & server_status::SERVER_MORE_RESULTS_EXISTS) != 0;

        Ok(Self {
            warning_count,
            server_status,
            has_next,
        })
    }
}

/// LOAD LOCAL パケットの内容をラップする構造体。
#[derive(Debug, Clone)]
pub struct LoadLocalPacketWrapper {
    pub filename: Vec<u8>,
}

impl LoadLocalPacketWrapper {
    /// LOAD LOCAL パケットからファイル名を抽出する。
    pub fn from_packet(packet: &MysqlPacket) -> crate::error::Result<Self> {
        if !packet.is_load_local_packet() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: "Cannot create LoadLocalPacketWrapper from invalid packet type"
                    .to_string(),
            });
        }
        let filename = packet.get_all_data()[1..].to_vec();
        Ok(Self { filename })
    }
}

/// パケットの内容をデバッグ出力する。
///
/// PyMySQL の `protocol.dump_packet` に相当し、
/// 16 バイトごとの 16 進ダンプと ASCII 表現を tracing の debug レベルで出力する。
pub fn dump_packet(data: &[u8]) {
    tracing::debug!("--- MySQL packet dump ({} bytes) ---", data.len());
    for (i, chunk) in data.chunks(16).enumerate() {
        let hex: Vec<String> = chunk.iter().map(|b| format!("{:02x}", b)).collect();
        let ascii: String = chunk
            .iter()
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        tracing::debug!("{:04x}  {:<48}  |{}|", i * 16, hex.join(" "), ascii);
    }
}
