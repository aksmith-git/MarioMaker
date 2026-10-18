//! The front end's side of the open-source course server: the signed-in account (kept with
//! [`Host::store_value`]: `online.token`, `online.account`), calls started through
//! [`Host::online`] and their answers, collected once per tick by the front end and taken by the
//! screen that asked ([`Online::take`]). Never Nintendo's network.

use serde_json::{Value, json};

use crate::host::Host;

/// The signed-in account (the token is the credential).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Account {
    pub name: String,
    pub maker_id: String,
    pub token: String,
}

/// Answers kept for screens that haven't taken them yet (older ones are dropped).
const KEEP: usize = 64;

#[derive(Debug, Default)]
pub struct Online {
    pub account: Option<Account>,
    next: u32,
    answers: Vec<(u32, Result<Value, String>)>,
}

impl Online {
    /// The account stored by an earlier sign-in (an empty token means signed out).
    pub fn load(host: &mut dyn Host) -> Self {
        let token = host
            .load_value("online.token")
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty());
        let info = host
            .load_value("online.account")
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .unwrap_or(Value::Null);
        let account = token.map(|token| Account {
            name: info["name"].as_str().unwrap_or_default().to_string(),
            maker_id: info["maker_id"].as_str().unwrap_or_default().to_string(),
            token,
        });
        Self {
            account,
            next: 1,
            answers: Vec::new(),
        }
    }

    pub fn signed_in(&self) -> bool {
        self.account.is_some()
    }

    pub fn maker_id(&self) -> Option<&str> {
        self.account.as_ref().map(|a| a.maker_id.as_str())
    }

    /// Start `action` (see [`Host::online`]) as the signed-in account: its id, None when no
    /// server is configured.
    pub fn call(&mut self, host: &mut dyn Host, action: &str, args: Value) -> Option<u32> {
        let id = self.next.max(1);
        self.next = id.wrapping_add(1).max(1);
        let token = self.account.as_ref().map(|a| a.token.clone());
        host.online(id, action, &args, token.as_deref())
            .then_some(id)
    }

    /// Collect the answers that arrived (the front end calls this once per tick).
    pub fn poll(&mut self, host: &mut dyn Host) {
        while let Some(a) = host.poll_online() {
            self.answers.push(a);
        }
        if self.answers.len() > KEEP {
            let n = self.answers.len() - KEEP;
            self.answers.drain(..n);
        }
    }

    /// The answer to call `id`, once (None: not yet).
    pub fn take(&mut self, id: u32) -> Option<Result<Value, String>> {
        let i = self.answers.iter().position(|(k, _)| *k == id)?;
        Some(self.answers.remove(i).1)
    }

    /// Keep the account of a `register` answer (`{account: {name, maker_id}, token}`).
    pub fn sign_in(&mut self, host: &mut dyn Host, answer: &Value) -> bool {
        let Some(token) = answer["token"].as_str().filter(|t| !t.is_empty()) else {
            return false;
        };
        let a = &answer["account"];
        let account = Account {
            name: a["name"].as_str().unwrap_or_default().to_string(),
            maker_id: a["maker_id"].as_str().unwrap_or_default().to_string(),
            token: token.to_string(),
        };
        host.store_value("online.token", &account.token);
        host.store_value(
            "online.account",
            &json!({ "name": account.name, "maker_id": account.maker_id }).to_string(),
        );
        self.account = Some(account);
        true
    }

    pub fn sign_out(&mut self, host: &mut dyn Host) {
        self.account = None;
        host.store_value("online.token", "");
        host.store_value("online.account", "{}");
    }
}

/// A text field's edit for one typed key (`FrontInput::text`): '\u{8}' erases, '\n' / '\r'
/// finish (true), other control characters are ignored; at most `max` characters.
pub fn edit(text: &mut String, key: char, max: usize) -> bool {
    match key {
        '\n' | '\r' => return true,
        '\u{8}' | '\u{7f}' => {
            text.pop();
        }
        c if c.is_control() => {}
        c if text.chars().count() < max => text.push(c),
        _ => {}
    }
    false
}

/// A time in ms as SMM2 shows it (`m:ss.mmm`), from the layout's own formats when it has them.
pub fn time_text(l: &crate::Layout, ms: i64) -> (String, String) {
    let a = l
        .format(
            "Time_0001",
            &[
                format!("{:02}", ms / 60000),
                format!("{:02}", ms / 1000 % 60),
            ],
        )
        .unwrap_or_else(|| format!("{:02}:{:02}", ms / 60000, ms / 1000 % 60));
    let b = l
        .format("Time_0002", &[format!("{:03}", ms % 1000)])
        .unwrap_or_else(|| format!(".{:03}", ms % 1000));
    (a, b)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::play::tests::MockHost;

    #[test]
    fn account_round_trip_and_answers() {
        let mut host = MockHost::default();
        let mut o = Online::load(&mut host);
        assert!(!o.signed_in());
        assert_eq!(o.call(&mut host, "me", Value::Null), None, "no server");
        host.server = true;
        let id = o
            .call(&mut host, "register", json!({"name": "Ada"}))
            .unwrap();
        assert_eq!(host.calls[0].1, "register");
        assert_eq!(host.calls[0].3, None, "no token before signing in");
        host.answers.push((
            id,
            Ok(json!({"account": {"name": "Ada", "maker_id": "M1"}, "token": "t0k"})),
        ));
        o.poll(&mut host);
        let a = o.take(id).unwrap().unwrap();
        assert!(o.take(id).is_none(), "taken once");
        assert!(o.sign_in(&mut host, &a));
        let again = Online::load(&mut host);
        assert_eq!(again.account.as_ref().unwrap().name, "Ada");
        assert_eq!(again.maker_id(), Some("M1"));
        o.call(&mut host, "me", Value::Null);
        assert_eq!(host.calls[1].3.as_deref(), Some("t0k"));
        o.sign_out(&mut host);
        assert!(!Online::load(&mut host).signed_in());
    }

    #[test]
    fn text_edits() {
        let mut s = String::new();
        for c in "Ab\u{8}c".chars() {
            assert!(!edit(&mut s, c, 3));
        }
        assert_eq!(s, "Ac");
        edit(&mut s, 'x', 3);
        edit(&mut s, 'y', 3);
        assert_eq!(s, "Acx");
        assert!(edit(&mut s, '\n', 3));
        edit(&mut s, '\u{1b}', 3);
        assert_eq!(s, "Acx");
    }
}
