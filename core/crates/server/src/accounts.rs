//! Accounts without Nintendo services: a name and a secret token per player, kept in the
//! server's directory (`accounts.json`). The token is the only credential (sent as
//! `Authorization: Bearer <token>`); the server stores only its SHA-256 so a leaked
//! `accounts.json` can't be replayed. Each account gets an SMM2-style maker ID (XXX-XXX-XXX).

use std::collections::HashMap;

use serde_json::{Value, json};

/// Longest display name (SMM2 allows 10 characters for a Mii name).
pub const MAX_NAME: usize = 10;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Account {
    pub id: u32,
    pub name: String,
    pub maker_id: String,
    /// SHA-256 of the token (`digest`).
    pub token_digest: String,
    pub created: u64,
    /// Moderation: banned accounts can't upload, vote or comment.
    pub banned: bool,
}

impl Account {
    pub fn public_json(&self) -> Value {
        json!({ "id": self.id, "name": self.name, "maker_id": self.maker_id,
                "created": self.created, "banned": self.banned })
    }

    fn to_json(&self) -> Value {
        let mut v = self.public_json();
        v["token_digest"] = json!(self.token_digest);
        v
    }

    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            id: v["id"].as_u64()? as u32,
            name: v["name"].as_str()?.to_string(),
            maker_id: v["maker_id"].as_str().unwrap_or_default().to_string(),
            token_digest: v["token_digest"].as_str()?.to_string(),
            created: v["created"].as_u64().unwrap_or(0),
            banned: v["banned"].as_bool().unwrap_or(false),
        })
    }
}

/// SHA-256 of the token, hex.
pub fn digest(token: &str) -> String {
    ring::digest::digest(&ring::digest::SHA256, token.as_bytes())
        .as_ref()
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect()
}

/// 32 random bytes as hex (the OS random source).
pub fn new_token() -> Result<String, String> {
    let mut b = [0u8; 32];
    getrandom::fill(&mut b).map_err(|e| e.to_string())?;
    Ok(b.iter().map(|x| format!("{x:02x}")).collect())
}

#[derive(Default)]
pub struct Accounts {
    pub list: Vec<Account>,
    by_digest: HashMap<String, usize>,
}

impl Accounts {
    pub fn from_json(v: &Value) -> Self {
        let list: Vec<Account> = v["accounts"]
            .as_array()
            .map(|a| a.iter().filter_map(Account::from_json).collect())
            .unwrap_or_default();
        let by_digest = list
            .iter()
            .enumerate()
            .map(|(i, a)| (a.token_digest.clone(), i))
            .collect();
        Self { list, by_digest }
    }

    pub fn to_json(&self) -> Value {
        json!({ "accounts": self.list.iter().map(Account::to_json).collect::<Vec<_>>() })
    }

    /// The account a bearer token belongs to.
    pub fn by_token(&self, token: &str) -> Option<&Account> {
        self.by_digest
            .get(&digest(token))
            .and_then(|&i| self.list.get(i))
    }

    pub fn by_id(&self, id: u32) -> Option<&Account> {
        self.list.iter().find(|a| a.id == id)
    }

    pub fn by_maker_id(&self, maker_id: &str) -> Option<&Account> {
        self.list.iter().find(|a| a.maker_id == maker_id)
    }

    /// New account; returns it and its token (shown once).
    pub fn register(&mut self, name: &str, now: u64) -> Result<(Account, String), String> {
        let name: String = name.trim().chars().take(MAX_NAME).collect();
        if name.is_empty() || name.chars().any(char::is_control) {
            return Err("bad name".into());
        }
        let token = new_token()?;
        let id = self.list.iter().map(|a| a.id).max().unwrap_or(0) + 1;
        // maker IDs use the course code alphabet, from a distinct id range
        let maker_id = mmk_formats::course_id::smm2_data_id_to_code(0x80_0000 + id, true);
        let a = Account {
            id,
            name,
            maker_id,
            token_digest: digest(&token),
            created: now,
            banned: false,
        };
        self.by_digest
            .insert(a.token_digest.clone(), self.list.len());
        self.list.push(a.clone());
        Ok((a, token))
    }

    pub fn set_banned(&mut self, id: u32, banned: bool) -> bool {
        match self.list.iter_mut().find(|a| a.id == id) {
            Some(a) => {
                a.banned = banned;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn register_and_look_up_by_token() {
        let mut acc = Accounts::default();
        let (a, token) = acc.register("  Luigi  ", 1).unwrap();
        assert_eq!(a.name, "Luigi");
        assert_eq!(token.len(), 64);
        assert_eq!(acc.by_token(&token).map(|x| x.id), Some(a.id));
        assert!(acc.by_token("nope").is_none());
        assert!(acc.register("", 1).is_err());
        // round trip keeps tokens working, never stores them
        let back = Accounts::from_json(&acc.to_json());
        assert_eq!(back.by_token(&token).map(|x| x.id), Some(a.id));
        assert!(!acc.to_json().to_string().contains(&token));
        assert_ne!(a.maker_id, "");
    }
}
