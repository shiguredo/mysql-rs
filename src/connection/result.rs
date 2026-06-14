// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 結果セット読み込み。

use crate::connection::Connection;
use crate::constants::field_type;
use crate::error::{Error, Result};
use crate::protocol::{
    ColumnDescription, EofPacketWrapper, FieldDescriptorPacket, LoadLocalPacketWrapper,
    MysqlPacket, OkPacketWrapper,
};

/// テキストとして扱うフィールド型の集合。
const TEXT_TYPES: &[u8] = &[
    field_type::BIT,
    field_type::BLOB,
    field_type::LONG_BLOB,
    field_type::MEDIUM_BLOB,
    field_type::STRING,
    field_type::TINY_BLOB,
    field_type::VAR_STRING,
    field_type::VARCHAR,
    field_type::GEOMETRY,
];

/// フィールドごとのコンバーター情報。
#[derive(Debug, Default, Clone)]
struct FieldConverter {
    encoding: Option<String>,
    converter: Option<crate::converters::Converter>,
}

/// 結果セット読み込みの内部状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum ReadState {
    /// 最初のパケットを待っている。
    #[default]
    Initial,
    /// カラム定義を読み込み中。
    Descriptions,
    /// 行データを読み込み中。
    Rows,
    /// アンバッファードクエリでカラム定義まで完了。
    UnbufferedReady,
    /// 読み込み完了。
    Done,
}

/// 1 パケットを処理した後の結果読み込み状態。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeedResult {
    /// さらにパケットが必要。
    NeedMore,
    /// 結果セットの読み込みが完了。
    Done,
    /// アンバッファードクエリのカラム定義まで読み込み完了。
    UnbufferedReady,
}

/// クエリ結果を表現する構造体。
#[derive(Debug, Default, Clone)]
pub struct MySQLResult {
    pub affected_rows: i64,
    pub insert_id: Option<u64>,
    pub server_status: Option<u16>,
    pub warning_count: u16,
    pub message: Option<Vec<u8>>,
    pub field_count: u64,
    pub description: Option<Vec<ColumnDescription>>,
    pub rows: Option<Vec<Vec<crate::converters::Value>>>,
    pub fields: Vec<FieldDescriptorPacket>,
    pub has_next: bool,
    pub unbuffered_active: bool,
    converters: Vec<FieldConverter>,
    read_state: ReadState,
}

impl MySQLResult {
    /// 新規の結果セットを作成する。
    pub fn new() -> Self {
        Self::default()
    }

    /// 結果セットの読み込みが完了しているかどうか。
    pub fn is_done(&self) -> bool {
        self.read_state == ReadState::Done
    }

    /// 最初のパケットを待っている状態かどうか。
    pub(crate) fn is_initial(&self) -> bool {
        self.read_state == ReadState::Initial
    }

    /// 1 パケットを消費して結果セットを組み立てる。
    ///
    /// 追加のパケットが必要な場合は `FeedResult::NeedMore` を返す。
    pub fn feed_packet(&mut self, packet: MysqlPacket, conn: &Connection) -> Result<FeedResult> {
        match self.read_state {
            ReadState::Initial => self.feed_initial(packet, conn),
            ReadState::Descriptions => self.feed_description(packet, conn),
            ReadState::Rows | ReadState::UnbufferedReady => self.feed_row(packet),
            ReadState::Done => Ok(FeedResult::Done),
        }
    }

    fn feed_initial(&mut self, mut packet: MysqlPacket, conn: &Connection) -> Result<FeedResult> {
        if packet.is_ok_packet() {
            let ok = OkPacketWrapper::from_packet(&mut packet)?;
            self.affected_rows = ok.affected_rows.unwrap_or(0) as i64;
            self.insert_id = ok.insert_id;
            self.server_status = Some(ok.server_status);
            self.warning_count = ok.warning_count;
            self.message = Some(ok.message);
            self.has_next = ok.has_next;
            self.read_state = ReadState::Done;
            return Ok(FeedResult::Done);
        }
        if packet.is_load_local_packet() {
            return self.read_load_local_packet(conn, packet);
        }
        self.field_count = packet.read_length_encoded_integer()?.unwrap_or(0);
        self.fields.clear();
        self.converters.clear();
        self.description = None;
        self.rows = None;
        self.read_state = ReadState::Descriptions;
        Ok(FeedResult::NeedMore)
    }

    fn feed_description(&mut self, packet: MysqlPacket, conn: &Connection) -> Result<FeedResult> {
        if self.fields.len() < self.field_count as usize {
            let field =
                FieldDescriptorPacket::parse(packet.get_all_data().to_vec(), conn.encoding())?;
            self.build_converter(&field, conn);
            self.fields.push(field);
            return Ok(FeedResult::NeedMore);
        }

        if !packet.is_eof_packet() {
            return Err(Error::InternalError {
                code: crate::constants::client_error::CR_MALFORMED_PACKET,
                message: "Protocol error, expecting EOF".to_string(),
            });
        }
        let mut pkt = packet;
        let wp = EofPacketWrapper::from_packet(&mut pkt)?;
        self.warning_count = wp.warning_count;
        self.has_next = wp.has_next;
        self.server_status = Some(wp.server_status);
        self.description = Some(self.fields.iter().map(|f| f.description()).collect());
        if self.unbuffered_active {
            self.read_state = ReadState::UnbufferedReady;
            Ok(FeedResult::UnbufferedReady)
        } else {
            self.read_state = ReadState::Rows;
            Ok(FeedResult::NeedMore)
        }
    }

    fn build_converter(&mut self, field: &FieldDescriptorPacket, conn: &Connection) {
        let encoding = if conn.use_unicode() {
            if field.type_code == field_type::JSON {
                // JSON 型は接続文字セットでデコードする。
                Some(conn.encoding().to_string())
            } else if TEXT_TYPES.contains(&field.type_code) {
                if field.charsetnr == 63 {
                    // binary の場合はデコードしない。
                    None
                } else {
                    Some(conn.encoding().to_string())
                }
            } else {
                // 数値・日時等は ASCII で十分。
                Some("ascii".to_string())
            }
        } else {
            None
        };
        let converter = crate::converters::decoder_for(field.type_code);
        self.converters.push(FieldConverter {
            encoding,
            converter,
        });
    }

    fn feed_row(&mut self, packet: MysqlPacket) -> Result<FeedResult> {
        if self.check_packet_is_eof(&packet)? {
            self.read_state = ReadState::Done;
            return Ok(FeedResult::Done);
        }
        let row = self.read_row_from_packet(packet)?;
        self.rows.get_or_insert_with(Vec::new).push(row);
        Ok(FeedResult::NeedMore)
    }

    fn check_packet_is_eof(&mut self, packet: &MysqlPacket) -> Result<bool> {
        if !packet.is_eof_packet() {
            return Ok(false);
        }
        let mut pkt = packet.clone();
        let wp = EofPacketWrapper::from_packet(&mut pkt)?;
        self.warning_count = wp.warning_count;
        self.has_next = wp.has_next;
        self.server_status = Some(wp.server_status);
        Ok(true)
    }

    fn read_row_from_packet(
        &self,
        mut packet: MysqlPacket,
    ) -> Result<Vec<crate::converters::Value>> {
        let mut row = Vec::new();
        for field_converter in &self.converters {
            let data = packet.read_length_coded_string()?;
            let value = match data {
                None => crate::converters::Value::Null,
                Some(bytes) => {
                    if let Some(enc) = &field_converter.encoding {
                        let s = encoding_rs::Encoding::for_label(enc.as_bytes())
                            .unwrap_or(encoding_rs::UTF_8)
                            .decode(&bytes)
                            .0
                            .to_string();
                        match field_converter.converter {
                            Some(conv) => conv(&s),
                            None => crate::converters::Value::String(s),
                        }
                    } else {
                        // binary 等、文字列デコードを行わない場合は bytes のまま返す。
                        crate::converters::Value::Bytes(bytes)
                    }
                }
            };
            row.push(value);
        }
        Ok(row)
    }

    /// 結果セットを読み込む。
    ///
    /// 受信キューに十分なパケットがない場合は `Error::NeedMoreData` を返す。
    pub fn read(&mut self, conn: &mut Connection) -> Result<()> {
        loop {
            let packet = conn.read_packet()?;
            match self.feed_packet(packet, conn)? {
                FeedResult::Done | FeedResult::UnbufferedReady => return Ok(()),
                FeedResult::NeedMore => continue,
            }
        }
    }

    /// アンバッファードクエリを初期化する。
    pub fn init_unbuffered_query(&mut self, conn: &mut Connection) -> Result<()> {
        let packet = conn.read_packet()?;
        if self.feed_packet(packet, conn)? == FeedResult::UnbufferedReady {
            self.affected_rows = u64::MAX as i64;
            return Ok(());
        }
        loop {
            let packet = conn.read_packet()?;
            match self.feed_packet(packet, conn)? {
                FeedResult::UnbufferedReady => {
                    self.affected_rows = u64::MAX as i64;
                    return Ok(());
                }
                FeedResult::Done => return Ok(()),
                FeedResult::NeedMore => continue,
            }
        }
    }

    /// アンバッファードクエリで次の行を読み込む。
    pub fn read_rowdata_packet_unbuffered(
        &mut self,
        packet: MysqlPacket,
    ) -> Result<Option<Vec<crate::converters::Value>>> {
        if !self.unbuffered_active {
            return Ok(None);
        }
        if self.check_packet_is_eof(&packet)? {
            self.unbuffered_active = false;
            self.read_state = ReadState::Done;
            return Ok(None);
        }
        self.affected_rows = 1;
        let row = self.read_row_from_packet(packet)?;
        self.rows = Some(vec![row.clone()]);
        Ok(Some(row))
    }

    /// アンバッファードクエリを終了する。
    pub fn finish_unbuffered_query(&mut self, conn: &mut Connection) -> Result<()> {
        while self.unbuffered_active {
            let packet = conn.read_packet()?;
            if self.check_packet_is_eof(&packet)? {
                self.unbuffered_active = false;
                self.read_state = ReadState::Done;
            }
        }
        Ok(())
    }

    fn read_load_local_packet(
        &mut self,
        conn: &Connection,
        pkt: MysqlPacket,
    ) -> Result<FeedResult> {
        if !conn.options().local_infile {
            return Err(Error::OperationalError {
                code: crate::constants::client_error::CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
                message: "Received LOAD_LOCAL packet but local_infile option is false".to_string(),
            });
        }
        let load_packet = LoadLocalPacketWrapper::from_packet(&pkt)?;
        let filename = String::from_utf8_lossy(&load_packet.filename).to_string();
        // ファイル読み込みは I/O を伴うため、呼び出し側で行う。
        Err(Error::OperationalError {
            code: crate::constants::client_error::CR_LOAD_DATA_LOCAL_INFILE_REJECTED,
            message: format!(
                "LOAD DATA LOCAL INFILE requires I/O driver support: {}",
                filename
            ),
        })
    }
}
