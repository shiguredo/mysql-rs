// Copyright 2026, Shiguredo Inc.
// SPDX-License-Identifier: Apache-2.0

//! MySQL 認証状態機械。

use crate::auth::{self, AuthPlugin};
use crate::connection::Connection;
use crate::constants::client;
use crate::constants::client_error;
use crate::error::{Error, Result};
use crate::protocol::MysqlPacket;
use std::collections::HashMap;

/// 認証処理の段階。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthPhase {
    /// 初期段階。auth switch / extra auth data の応答待ち。
    Initial,
    /// caching_sha2_password の fast auth 成功後、OK パケット待ち。
    CachingSha2FastSuccess,
    /// caching_sha2_password の公開鍵応答待ち。
    CachingSha2PublicKey,
    /// sha256_password の応答待ち。
    Sha256Password,
}

impl Connection {
    /// 認証要求の第一段階。
    ///
    /// SSL リクエスト（必要な場合）および認証パケットを send_queue に追加する。
    pub fn request_authentication_start(&mut self) -> Result<crate::connection::AuthState> {
        if self.options().user.is_empty() {
            return Err(Error::ProgrammingError {
                code: client_error::CR_MALFORMED_PACKET,
                message: "Did not specify a username".to_string(),
            });
        }

        let charset =
            crate::charset::charset_by_name(self.character_set_name()).ok_or_else(|| {
                Error::OperationalError {
                    code: client_error::CR_CANT_READ_CHARSET,
                    message: format!("Unknown charset: {}", self.character_set_name()),
                }
            })?;
        let charset_id = charset.id;

        let mut client_flags = self.client_flag;
        let do_ssl = match self.options().ssl_mode {
            crate::connection::SslMode::Disabled => false,
            crate::connection::SslMode::Preferred => (self.server_capabilities & client::SSL) != 0,
            crate::connection::SslMode::Required => {
                if (self.server_capabilities & client::SSL) == 0 {
                    return Err(Error::OperationalError {
                        code: client_error::CR_SSL_CONNECTION_ERROR,
                        message: "SSL is required but the server doesn't support it".to_string(),
                    });
                }
                true
            }
        };

        if do_ssl {
            client_flags |= client::SSL;
        }

        let mut data = Vec::new();
        data.extend_from_slice(&client_flags.to_le_bytes());
        data.extend_from_slice(&(crate::connection::packet::MAX_PACKET_LEN as u32).to_le_bytes());
        data.push(charset_id as u8);
        data.extend_from_slice(&[0u8; 23]);

        if do_ssl {
            self.write_packet(&data)?;
            self.needs_tls_upgrade = true;
            return Ok(crate::connection::AuthState::Send);
        }

        self.write_auth_packet(client_flags)?;
        self.auth_phase = AuthPhase::Initial;
        Ok(crate::connection::AuthState::NeedRead)
    }

    /// TLS アップグレード後に認証情報を送信する。
    ///
    /// SSL リクエストを送信済みで `secure` が true の状態で呼び出す。
    /// 認証パケットを send_queue に追加し、サーバー応答を待つ状態を返す。
    pub fn request_authentication_send_credentials(
        &mut self,
    ) -> Result<crate::connection::AuthState> {
        let mut client_flags = self.client_flag;
        client_flags |= client::SSL;
        self.write_auth_packet(client_flags)?;
        self.auth_phase = AuthPhase::Initial;
        Ok(crate::connection::AuthState::NeedRead)
    }

    /// 認証パケットを送信キューに追加する。
    fn write_auth_packet(&mut self, client_flags: u32) -> Result<()> {
        let charset =
            crate::charset::charset_by_name(self.character_set_name()).ok_or_else(|| {
                Error::OperationalError {
                    code: client_error::CR_CANT_READ_CHARSET,
                    message: format!("Unknown charset: {}", self.character_set_name()),
                }
            })?;
        let charset_id = charset.id;

        let mut data = Vec::new();
        data.extend_from_slice(&client_flags.to_le_bytes());
        data.extend_from_slice(&(crate::connection::packet::MAX_PACKET_LEN as u32).to_le_bytes());
        data.push(charset_id as u8);
        data.extend_from_slice(&[0u8; 23]);

        data.extend_from_slice(self.options().user.as_bytes());
        data.push(0);

        let authresp = auth::make_auth_response(
            self.auth_plugin_name(),
            &self.options().password,
            self.salt(),
            self.is_secure(),
        )?;

        if (self.server_capabilities & client::PLUGIN_AUTH_LENENC_CLIENT_DATA) != 0 {
            data.extend_from_slice(&crate::connection::lenenc_int(authresp.len()));
            data.extend_from_slice(&authresp);
        } else if (self.server_capabilities & client::SECURE_CONNECTION) != 0 {
            let len = authresp.len();
            if len > 255 {
                return Err(Error::OperationalError {
                    code: client_error::CR_AUTH_PLUGIN_ERR,
                    message: format!("auth response too long for SECURE_CONNECTION: {}", len),
                });
            }
            data.push(len as u8);
            data.extend_from_slice(&authresp);
        } else {
            data.extend_from_slice(&authresp);
            data.push(0);
        }

        if let Some(db) = &self.options().database
            && (self.server_capabilities & client::CONNECT_WITH_DB) != 0
        {
            data.extend_from_slice(db.as_bytes());
            data.push(0);
        }

        if (self.server_capabilities & client::PLUGIN_AUTH) != 0 {
            data.extend_from_slice(self.auth_plugin_name().as_bytes());
            data.push(0);
        }

        if (self.server_capabilities & client::CONNECT_ATTRS) != 0 {
            let mut connect_attrs = Vec::new();
            let mut attrs: HashMap<&str, String> = HashMap::new();
            attrs.insert("_client_name", "mysql".to_string());
            attrs.insert("_client_version", env!("CARGO_PKG_VERSION").to_string());
            attrs.insert("_pid", std::process::id().to_string());
            if let Some(program_name) = &self.options().program_name {
                attrs.insert("program_name", program_name.clone());
            }
            for (k, v) in attrs {
                connect_attrs.extend_from_slice(&crate::connection::lenenc_int(k.len()));
                connect_attrs.extend_from_slice(k.as_bytes());
                connect_attrs.extend_from_slice(&crate::connection::lenenc_int(v.len()));
                connect_attrs.extend_from_slice(v.as_bytes());
            }
            data.extend_from_slice(&crate::connection::lenenc_int(connect_attrs.len()));
            data.extend_from_slice(&connect_attrs);
        }

        self.write_packet(&data)
    }

    /// 認証応答を処理し、次の状態を返す。
    ///
    /// 追加の送信が必要な場合は send_queue にデータを追加して AuthState::Send を返す。
    pub fn request_authentication_continue(&mut self) -> Result<crate::connection::AuthState> {
        let auth_packet = self.read_packet()?;
        match self.auth_phase {
            AuthPhase::Initial => self.process_auth_initial(auth_packet),
            AuthPhase::CachingSha2FastSuccess => {
                if !auth_packet.is_ok_packet() {
                    return Err(Error::OperationalError {
                        code: client_error::CR_AUTH_PLUGIN_ERR,
                        message: "caching sha2: expected OK packet after fast auth".to_string(),
                    });
                }
                let mut pkt = auth_packet;
                let ok = crate::protocol::OkPacketWrapper::from_packet(&mut pkt)?;
                self.server_status = ok.server_status;
                self.auth_phase = AuthPhase::Initial;
                Ok(crate::connection::AuthState::Success)
            }
            AuthPhase::CachingSha2PublicKey => self.process_caching_sha2_public_key(auth_packet),
            AuthPhase::Sha256Password => self.process_sha256_continue(auth_packet),
        }
    }

    fn process_auth_initial(
        &mut self,
        mut auth_packet: MysqlPacket,
    ) -> Result<crate::connection::AuthState> {
        if auth_packet.is_auth_switch_request() {
            auth_packet.read_uint8()?;
            let plugin_name = auth_packet.read_string()?.unwrap_or_default();
            *self.auth_plugin_name_mut() = AuthPlugin::from_bytes(&plugin_name);
            return self.process_auth_switch(auth_packet);
        }

        if auth_packet.is_extra_auth_data() {
            match self.auth_plugin_name() {
                AuthPlugin::CachingSha2Password => {
                    return self.process_caching_sha2_fast(auth_packet);
                }
                AuthPlugin::Sha256Password => {
                    return self.process_sha256_continue(auth_packet);
                }
                _ => {
                    return Err(Error::OperationalError {
                        code: client_error::CR_AUTH_PLUGIN_ERR,
                        message: format!(
                            "Received extra packet for auth method {:?}",
                            self.auth_plugin_name()
                        ),
                    });
                }
            }
        }

        auth_packet.check_error()?;
        Ok(crate::connection::AuthState::Success)
    }

    fn process_auth_switch(
        &mut self,
        mut auth_packet: MysqlPacket,
    ) -> Result<crate::connection::AuthState> {
        // auth_packet は既に 0xFE とプラグイン名を読み進めている。
        // 残りが認証プラグインから提供される salt である。
        let mut salt = auth_packet.read_all().to_vec();
        if salt.ends_with(&[0]) {
            salt.pop();
        }

        match self.auth_plugin_name() {
            AuthPlugin::MysqlNativePassword => {
                let data = auth::scramble_native_password(&self.options().password, &salt);
                self.write_packet(&data)?;
                self.auth_phase = AuthPhase::Initial;
                Ok(crate::connection::AuthState::Send)
            }
            AuthPlugin::CachingSha2Password => {
                let data = auth::scramble_caching_sha2(&self.options().password, &salt);
                self.write_packet(&data)?;
                self.auth_phase = AuthPhase::Initial;
                Ok(crate::connection::AuthState::Send)
            }
            AuthPlugin::Sha256Password => {
                *self.salt_mut() = salt;
                if self.is_secure() {
                    let mut data = self.options().password.clone();
                    data.push(0);
                    self.write_packet(&data)?;
                    self.auth_phase = AuthPhase::Initial;
                    Ok(crate::connection::AuthState::Send)
                } else if self.options().password.is_empty() {
                    self.write_packet(&[0])?;
                    self.auth_phase = AuthPhase::Initial;
                    Ok(crate::connection::AuthState::Send)
                } else {
                    self.write_packet(&[1])?;
                    self.auth_phase = AuthPhase::Sha256Password;
                    Ok(crate::connection::AuthState::Send)
                }
            }
            AuthPlugin::MysqlClearPassword => {
                let mut data = self.options().password.clone();
                data.push(0);
                self.write_packet(&data)?;
                self.auth_phase = AuthPhase::Initial;
                Ok(crate::connection::AuthState::Send)
            }
            _ => Err(Error::OperationalError {
                code: client_error::CR_AUTH_PLUGIN_CANNOT_LOAD,
                message: format!(
                    "Authentication plugin {:?} not configured",
                    self.auth_plugin_name()
                ),
            }),
        }
    }

    fn process_caching_sha2_fast(
        &mut self,
        mut auth_packet: MysqlPacket,
    ) -> Result<crate::connection::AuthState> {
        if auth_packet.is_error_packet() {
            return Err(auth_packet.raise_for_error());
        }
        if !auth_packet.is_extra_auth_data() {
            return Err(Error::OperationalError {
                code: client_error::CR_AUTH_PLUGIN_ERR,
                message: format!(
                    "caching sha2: Unknown packet for fast auth: {:?}",
                    auth_packet.get_all_data().first()
                ),
            });
        }

        auth_packet.advance(1)?;
        let n = auth_packet.read_uint8()?;

        if n == 3 {
            auth_packet.check_error()?;
            self.auth_phase = AuthPhase::CachingSha2FastSuccess;
            return Ok(crate::connection::AuthState::NeedRead);
        }

        if n != 4 {
            return Err(Error::OperationalError {
                code: client_error::CR_AUTH_PLUGIN_ERR,
                message: format!("caching sha2: Unknown result for fast auth: {}", n),
            });
        }

        if self.is_secure() {
            let mut data = self.options().password.clone();
            data.push(0);
            self.write_packet(&data)?;
            self.auth_phase = AuthPhase::Initial;
            return Ok(crate::connection::AuthState::Send);
        }

        self.write_packet(&[2])?;
        self.auth_phase = AuthPhase::CachingSha2PublicKey;
        Ok(crate::connection::AuthState::Send)
    }

    fn process_caching_sha2_public_key(
        &mut self,
        pkt: MysqlPacket,
    ) -> Result<crate::connection::AuthState> {
        if pkt.is_error_packet() {
            return Err(pkt.raise_for_error());
        }
        if !pkt.is_extra_auth_data() {
            return Err(Error::OperationalError {
                code: client_error::CR_AUTH_PLUGIN_ERR,
                message: format!(
                    "caching sha2: Unknown packet for public key: {:?}",
                    pkt.get_all_data().first()
                ),
            });
        }
        self.server_public_key = Some(pkt.get_all_data()[1..].to_vec());
        // 直前で設定したため unwrap は安全。
        let key = self.server_public_key.as_ref().unwrap();
        let data = auth::sha2_rsa_encrypt(&self.options().password, self.salt(), key)?;
        self.write_packet(&data)?;
        self.auth_phase = AuthPhase::Initial;
        Ok(crate::connection::AuthState::Send)
    }

    fn process_sha256_continue(
        &mut self,
        auth_packet: MysqlPacket,
    ) -> Result<crate::connection::AuthState> {
        if self.auth_phase == AuthPhase::Initial {
            let mut salt = auth_packet.get_all_data()[1..].to_vec();
            if salt.ends_with(&[0]) {
                salt.pop();
            }
            *self.salt_mut() = salt;

            let password = self.options().password.clone();
            if self.is_secure() {
                let mut data = password.clone();
                data.push(0);
                self.write_packet(&data)?;
                self.auth_phase = AuthPhase::Initial;
                return Ok(crate::connection::AuthState::Send);
            }

            if !password.is_empty() {
                self.write_packet(&[1])?;
                self.auth_phase = AuthPhase::Sha256Password;
                return Ok(crate::connection::AuthState::Send);
            }

            self.write_packet(&[0])?;
            self.auth_phase = AuthPhase::Initial;
            return Ok(crate::connection::AuthState::Send);
        }

        // サーバーからの公開鍵応答。
        if auth_packet.is_error_packet() {
            return Err(auth_packet.raise_for_error());
        }
        if auth_packet.is_extra_auth_data() {
            self.server_public_key = Some(auth_packet.get_all_data()[1..].to_vec());
        }

        let password = self.options().password.clone();
        let data = if password.is_empty() {
            Vec::new()
        } else {
            let key = self
                .server_public_key
                .as_ref()
                .ok_or_else(|| Error::OperationalError {
                    code: client_error::CR_AUTH_PLUGIN_ERR,
                    message: "Couldn't receive server's public key".to_string(),
                })?;
            auth::sha2_rsa_encrypt(&password, self.salt(), key)?
        };

        self.write_packet(&data)?;
        self.auth_phase = AuthPhase::Initial;
        Ok(crate::connection::AuthState::Send)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_caching_sha2_fast_success_consumes_ok_packet() {
        let mut conn = Connection::connect(crate::connection::ConnectOptions::default()).unwrap();
        conn.auth_phase = AuthPhase::CachingSha2FastSuccess;

        // OK パケット: header + affected_rows(0) + insert_id(0) + server_status + warning_count
        let ok_payload = vec![0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
        let packet = crate::connection::packet::tests::build_packet(0, &ok_payload);
        conn.feed_bytes(&packet).unwrap();

        let result = conn.request_authentication_continue();
        assert!(matches!(result, Ok(crate::connection::AuthState::Success)));
        assert_eq!(conn.auth_phase, AuthPhase::Initial);
    }
}
