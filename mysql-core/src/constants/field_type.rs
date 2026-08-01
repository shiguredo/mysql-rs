// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL フィールド型。

#![allow(dead_code)]

pub const DECIMAL: u8 = 0;
pub const TINY: u8 = 1;
pub const SHORT: u8 = 2;
pub const LONG: u8 = 3;
pub const FLOAT: u8 = 4;
pub const DOUBLE: u8 = 5;
pub const NULL: u8 = 6;
pub const TIMESTAMP: u8 = 7;
pub const LONGLONG: u8 = 8;
pub const INT24: u8 = 9;
pub const DATE: u8 = 10;
pub const TIME: u8 = 11;
pub const DATETIME: u8 = 12;
pub const YEAR: u8 = 13;
pub const NEWDATE: u8 = 14;
pub const VARCHAR: u8 = 15;
pub const BIT: u8 = 16;
pub const JSON: u8 = 245;
pub const NEWDECIMAL: u8 = 246;
pub const ENUM: u8 = 247;
pub const SET: u8 = 248;
pub const TINY_BLOB: u8 = 249;
pub const MEDIUM_BLOB: u8 = 250;
pub const LONG_BLOB: u8 = 251;
pub const BLOB: u8 = 252;
pub const VAR_STRING: u8 = 253;
pub const STRING: u8 = 254;
pub const GEOMETRY: u8 = 255;

pub const CHAR: u8 = TINY;
pub const INTERVAL: u8 = ENUM;
