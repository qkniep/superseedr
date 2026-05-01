// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use super::types::InfoHash;
use sha1::{Digest, Sha1};
use std::net::IpAddr;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct TokenConfig {
    pub rotation_period: Duration,
    pub acceptance_window: Duration,
}

impl Default for TokenConfig {
    fn default() -> Self {
        Self {
            rotation_period: Duration::from_secs(300),
            acceptance_window: Duration::from_secs(600),
        }
    }
}

#[derive(Debug, Clone)]
struct RollingSecret {
    secret: [u8; 32],
    started_at: Instant,
}

#[derive(Debug, Clone)]
pub struct TokenService {
    config: TokenConfig,
    current: RollingSecret,
    previous: Option<RollingSecret>,
}

impl TokenService {
    pub fn new(config: TokenConfig, now: Instant) -> Self {
        Self {
            config,
            current: RollingSecret {
                secret: rand::random::<[u8; 32]>(),
                started_at: now,
            },
            previous: None,
        }
    }

    pub fn config(&self) -> &TokenConfig {
        &self.config
    }

    pub fn mint_for(&mut self, addr: IpAddr, info_hash: InfoHash, now: Instant) -> Vec<u8> {
        self.rotate_if_due(now);
        derive_token(&self.current.secret, addr, info_hash)
    }

    pub fn validate_for(
        &mut self,
        addr: IpAddr,
        info_hash: InfoHash,
        token: &[u8],
        now: Instant,
    ) -> bool {
        self.rotate_if_due(now);
        if derive_token(&self.current.secret, addr, info_hash).as_slice() == token {
            return true;
        }

        self.previous
            .as_ref()
            .filter(|previous| {
                now.duration_since(previous.started_at) <= self.config.acceptance_window
            })
            .is_some_and(|previous| {
                derive_token(&previous.secret, addr, info_hash).as_slice() == token
            })
    }

    fn rotate_if_due(&mut self, now: Instant) {
        if now.duration_since(self.current.started_at) < self.config.rotation_period {
            self.drop_expired_previous(now);
            return;
        }

        let old_current = self.current.clone();
        self.previous = Some(old_current);
        self.current = RollingSecret {
            secret: rand::random::<[u8; 32]>(),
            started_at: now,
        };
        self.drop_expired_previous(now);
    }

    fn drop_expired_previous(&mut self, now: Instant) {
        if self.previous.as_ref().is_some_and(|previous| {
            now.duration_since(previous.started_at) > self.config.acceptance_window
        }) {
            self.previous = None;
        }
    }
}

fn derive_token(secret: &[u8; 32], addr: IpAddr, info_hash: InfoHash) -> Vec<u8> {
    let mut hasher = Sha1::new();
    hasher.update(secret);
    match addr {
        IpAddr::V4(addr) => hasher.update(addr.octets()),
        IpAddr::V6(addr) => hasher.update(addr.octets()),
    }
    hasher.update(info_hash.as_ref());
    hasher.finalize().to_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    fn info_hash(byte: u8) -> InfoHash {
        InfoHash::from([byte; InfoHash::LEN])
    }

    #[test]
    fn tokens_are_scoped_to_info_hash() {
        let now = Instant::now();
        let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let mut service = TokenService::new(TokenConfig::default(), now);
        let token = service.mint_for(addr, info_hash(1), now);

        assert!(service.validate_for(addr, info_hash(1), &token, now));
        assert!(!service.validate_for(addr, info_hash(2), &token, now));
    }

    #[test]
    fn previous_secret_acceptance_keeps_info_hash_scope() {
        let now = Instant::now();
        let config = TokenConfig {
            rotation_period: Duration::from_secs(1),
            acceptance_window: Duration::from_secs(10),
        };
        let addr = IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1));
        let mut service = TokenService::new(config, now);
        let token = service.mint_for(addr, info_hash(1), now);
        let later = now + Duration::from_secs(2);

        assert!(service.validate_for(addr, info_hash(1), &token, later));
        assert!(!service.validate_for(addr, info_hash(2), &token, later));
    }
}
