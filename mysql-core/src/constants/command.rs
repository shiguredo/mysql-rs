// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL コマンド種別。

#![allow(dead_code)]

pub const COM_SLEEP: u8 = 0x00;
pub const COM_QUIT: u8 = 0x01;
pub const COM_INIT_DB: u8 = 0x02;
pub const COM_QUERY: u8 = 0x03;
pub const COM_FIELD_LIST: u8 = 0x04;
pub const COM_CREATE_DB: u8 = 0x05;
pub const COM_DROP_DB: u8 = 0x06;
pub const COM_REFRESH: u8 = 0x07;
pub const COM_SHUTDOWN: u8 = 0x08;
pub const COM_STATISTICS: u8 = 0x09;
pub const COM_PROCESS_INFO: u8 = 0x0A;
pub const COM_CONNECT: u8 = 0x0B;
pub const COM_PROCESS_KILL: u8 = 0x0C;
pub const COM_DEBUG: u8 = 0x0D;
pub const COM_PING: u8 = 0x0E;
pub const COM_TIME: u8 = 0x0F;
pub const COM_DELAYED_INSERT: u8 = 0x10;
pub const COM_CHANGE_USER: u8 = 0x11;
pub const COM_BINLOG_DUMP: u8 = 0x12;
pub const COM_TABLE_DUMP: u8 = 0x13;
pub const COM_CONNECT_OUT: u8 = 0x14;
pub const COM_REGISTER_SLAVE: u8 = 0x15;
pub const COM_STMT_PREPARE: u8 = 0x16;
pub const COM_STMT_EXECUTE: u8 = 0x17;
pub const COM_STMT_SEND_LONG_DATA: u8 = 0x18;
pub const COM_STMT_CLOSE: u8 = 0x19;
pub const COM_STMT_RESET: u8 = 0x1A;
pub const COM_SET_OPTION: u8 = 0x1B;
pub const COM_STMT_FETCH: u8 = 0x1C;
pub const COM_DAEMON: u8 = 0x1D;
pub const COM_BINLOG_DUMP_GTID: u8 = 0x1E;
pub const COM_END: u8 = 0x1F;
