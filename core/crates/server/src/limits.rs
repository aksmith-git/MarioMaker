//! Abuse limits for a public server: writes per client address in a sliding minute, and account
//! creations per address per hour. Reads are never limited. Kept in memory (a restart forgets).

use std::collections::HashMap;
use std::net::IpAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug)]
pub struct Limits {
    /// POST / DELETE requests per address per minute.
    pub writes_per_minute: u32,
    /// New accounts per address per hour.
    pub accounts_per_hour: u32,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            writes_per_minute: 60,
            accounts_per_hour: 5,
        }
    }
}

/// What a request counts against.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Kind {
    Write,
    Account,
}

/// Fixed windows per (address, kind).
pub struct Gate {
    pub limits: Limits,
    seen: Mutex<HashMap<(IpAddr, Kind), (Instant, u32)>>,
}

impl Gate {
    pub fn new(limits: Limits) -> Self {
        Self {
            limits,
            seen: Mutex::new(HashMap::new()),
        }
    }

    /// Count one request of `kind` from `ip` at `now`; false when over the limit.
    pub fn allow(&self, ip: IpAddr, kind: Kind, now: Instant) -> bool {
        let (window, max) = match kind {
            Kind::Write => (Duration::from_secs(60), self.limits.writes_per_minute),
            Kind::Account => (Duration::from_secs(3600), self.limits.accounts_per_hour),
        };
        let mut seen = self.seen.lock().unwrap();
        if seen.len() > 100_000 {
            // bound memory: drop finished windows
            seen.retain(|_, (start, _)| now.duration_since(*start) < Duration::from_secs(3600));
        }
        let e = seen.entry((ip, kind)).or_insert((now, 0));
        if now.duration_since(e.0) >= window {
            *e = (now, 0);
        }
        if e.1 >= max {
            return false;
        }
        e.1 += 1;
        true
    }
}

/// The limit a request counts against (None: not limited).
pub fn kind_of(method: &str, path: &str) -> Option<Kind> {
    match method {
        "POST" if path.trim_end_matches('/') == "/api/accounts" => Some(Kind::Account),
        "POST" | "DELETE" | "PUT" => Some(Kind::Write),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_limit_and_reset() {
        let g = Gate::new(Limits {
            writes_per_minute: 3,
            accounts_per_hour: 1,
        });
        let (a, b): (IpAddr, IpAddr) = ("1.2.3.4".parse().unwrap(), "5.6.7.8".parse().unwrap());
        let t = Instant::now();
        assert!((0..3).all(|_| g.allow(a, Kind::Write, t)));
        assert!(!g.allow(a, Kind::Write, t), "4th write in the minute");
        assert!(g.allow(b, Kind::Write, t), "other addresses unaffected");
        assert!(
            g.allow(a, Kind::Write, t + Duration::from_secs(61)),
            "next minute"
        );
        assert!(g.allow(a, Kind::Account, t));
        assert!(!g.allow(a, Kind::Account, t + Duration::from_secs(1800)));
        assert_eq!(kind_of("POST", "/api/accounts"), Some(Kind::Account));
        assert_eq!(kind_of("GET", "/api/courses"), None);
        assert_eq!(kind_of("DELETE", "/api/courses/X"), Some(Kind::Write));
    }
}
