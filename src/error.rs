// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 関連のエラー型。

use crate::constants::server_error;
use std::fmt;

/// 結果型のエイリアス。
pub type Result<T> = std::result::Result<T, Error>;

/// MySQL 関連の全エラーを表す列挙型。
#[derive(Debug)]
pub enum Error {
    /// 警告。
    Warning(String),

    /// 汎用エラー。
    Error(String),

    /// インターフェイス関連エラー。
    InterfaceError { code: u16, message: String },

    /// データベース関連エラー。
    DatabaseError { code: u16, message: String },

    /// データ関連エラー。
    DataError { code: u16, message: String },

    /// 運用エラー。
    OperationalError { code: u16, message: String },

    /// 整合性エラー。
    IntegrityError { code: u16, message: String },

    /// 内部エラー。
    InternalError { code: u16, message: String },

    /// プログラミングエラー。
    ProgrammingError { code: u16, message: String },

    /// 未サポートエラー。
    NotSupportedError { code: u16, message: String },

    /// さらにデータが必要。
    NeedMoreData,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Warning(msg) => write!(f, "{}", msg),
            Error::Error(msg) => write!(f, "{}", msg),
            Error::InterfaceError { code, message } => write!(f, "{}: {}", code, message),
            Error::DatabaseError { code, message } => write!(f, "{}: {}", code, message),
            Error::DataError { code, message } => write!(f, "{}: {}", code, message),
            Error::OperationalError { code, message } => write!(f, "{}: {}", code, message),
            Error::IntegrityError { code, message } => write!(f, "{}: {}", code, message),
            Error::InternalError { code, message } => write!(f, "{}: {}", code, message),
            Error::ProgrammingError { code, message } => write!(f, "{}: {}", code, message),
            Error::NotSupportedError { code, message } => write!(f, "{}: {}", code, message),
            Error::NeedMoreData => write!(f, "Need more data"),
        }
    }
}

impl std::error::Error for Error {}

/// MySQL サーバーからのエラーパケットを解析して適切なエラーを生成する。
pub fn raise_mysql_exception(data: &[u8]) -> Error {
    if data.len() < 3 {
        return Error::InternalError {
            code: crate::constants::client_error::CR_MALFORMED_PACKET,
            message: "Malformed error packet: too short".to_string(),
        };
    }
    let errno = u16::from_le_bytes([data[1], data[2]]);
    let (_sqlstate, errval) = if data.get(3) == Some(&0x23) {
        // SQLSTATE 付きエラー。
        let state = String::from_utf8_lossy(data.get(4..9).unwrap_or(&[])).to_string();
        let msg = String::from_utf8_lossy(data.get(9..).unwrap_or(&[])).to_string();
        (Some(state), msg)
    } else {
        (
            None,
            String::from_utf8_lossy(data.get(3..).unwrap_or(&[])).to_string(),
        )
    };

    let error_class = error_map(errno);
    match error_class {
        ErrorClass::Programming => Error::ProgrammingError {
            code: errno,
            message: errval,
        },
        ErrorClass::Data => Error::DataError {
            code: errno,
            message: errval,
        },
        ErrorClass::Integrity => Error::IntegrityError {
            code: errno,
            message: errval,
        },
        ErrorClass::NotSupported => Error::NotSupportedError {
            code: errno,
            message: errval,
        },
        ErrorClass::Operational => Error::OperationalError {
            code: errno,
            message: errval,
        },
        ErrorClass::Internal => Error::InternalError {
            code: errno,
            message: errval,
        },
    }
}

#[derive(Clone, Copy)]
enum ErrorClass {
    Programming,
    Data,
    Integrity,
    NotSupported,
    Operational,
    Internal,
}

fn error_map(errno: u16) -> ErrorClass {
    match errno {
        server_error::DB_CREATE_EXISTS
        | server_error::SYNTAX_ERROR
        | server_error::PARSE_ERROR
        | server_error::NO_SUCH_TABLE
        | server_error::WRONG_DB_NAME
        | server_error::WRONG_TABLE_NAME
        | server_error::FIELD_SPECIFIED_TWICE
        | server_error::INVALID_GROUP_FUNC_USE
        | server_error::UNSUPPORTED_EXTENSION
        | server_error::TABLE_MUST_HAVE_COLUMNS
        | server_error::CANT_DO_THIS_DURING_AN_TRANSACTION
        | server_error::WRONG_COLUMN_NAME => ErrorClass::Programming,

        server_error::WARN_DATA_TRUNCATED
        | server_error::WARN_NULL_TO_NOTNULL
        | server_error::WARN_DATA_OUT_OF_RANGE
        | server_error::NO_DEFAULT
        | server_error::PRIMARY_CANT_HAVE_NULL
        | server_error::DATA_TOO_LONG
        | server_error::DATETIME_FUNCTION_OVERFLOW
        | server_error::TRUNCATED_WRONG_VALUE_FOR_FIELD
        | server_error::ILLEGAL_VALUE_FOR_TYPE => ErrorClass::Data,

        server_error::DUP_ENTRY
        | server_error::NO_REFERENCED_ROW
        | server_error::NO_REFERENCED_ROW_2
        | server_error::ROW_IS_REFERENCED
        | server_error::ROW_IS_REFERENCED_2
        | server_error::CANNOT_ADD_FOREIGN
        | server_error::BAD_NULL_ERROR => ErrorClass::Integrity,

        server_error::WARNING_NOT_COMPLETE_ROLLBACK
        | server_error::NOT_SUPPORTED_YET
        | server_error::FEATURE_DISABLED
        | server_error::UNKNOWN_STORAGE_ENGINE => ErrorClass::NotSupported,

        server_error::DBACCESS_DENIED_ERROR
        | server_error::ACCESS_DENIED_ERROR
        | server_error::CON_COUNT_ERROR
        | server_error::TABLEACCESS_DENIED_ERROR
        | server_error::COLUMNACCESS_DENIED_ERROR
        | server_error::CONSTRAINT_FAILED
        | server_error::LOCK_DEADLOCK => ErrorClass::Operational,

        _ => {
            if errno < 1000 {
                ErrorClass::Internal
            } else {
                ErrorClass::Operational
            }
        }
    }
}
