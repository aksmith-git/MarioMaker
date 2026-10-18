//! Client of the open-source course server (`mmk-server`, Phase 10): Course World browsing,
//! downloads, uploads and statistics, and a [`CourseSource`] so 100 Mario runs can draw their
//! courses from a server. Never talks to Nintendo's network.

use mmk_modes::Rng;
use mmk_modes::pool::{CourseRef, CourseSource};
use serde_json::Value;

/// A course listing as the server describes it.
#[derive(Clone, Debug, PartialEq)]
pub struct Listing {
    pub code: String,
    pub name: String,
    pub maker: String,
    pub style: String,
    pub theme: u8,
    pub plays: u64,
    pub clears: u64,
    pub likes: u64,
    pub boos: u64,
    pub difficulty: u8,
    /// The uploader's maker ID (empty for anonymous uploads), the record (ms, 0 none) and its
    /// holder, the first clearer.
    pub maker_id: String,
    pub record_ms: u64,
    pub record_holder: String,
    pub first_clear: String,
}

impl Listing {
    fn from_json(v: &Value) -> Option<Self> {
        let s = |k: &str| v[k].as_str().unwrap_or_default().to_string();
        let n = |k: &str| v[k].as_u64().unwrap_or(0);
        Some(Self {
            code: v["code"].as_str()?.to_string(),
            name: s("name"),
            maker: s("maker"),
            style: s("style"),
            theme: n("theme") as u8,
            plays: n("plays"),
            clears: n("clears"),
            likes: n("likes"),
            boos: n("boos"),
            difficulty: n("difficulty") as u8,
            maker_id: s("maker_id"),
            record_ms: n("record_ms"),
            record_holder: s("record_holder"),
            first_clear: s("first_clear"),
        })
    }
}

/// A server account (the token is the credential; keep it private).
#[derive(Clone, Debug, PartialEq)]
pub struct Account {
    pub id: u32,
    pub name: String,
    pub maker_id: String,
}

impl Account {
    fn from_json(v: &Value) -> Option<Self> {
        Some(Self {
            id: v["id"].as_u64()? as u32,
            name: v["name"].as_str()?.to_string(),
            maker_id: v["maker_id"].as_str().unwrap_or_default().to_string(),
        })
    }
}

/// A course comment.
#[derive(Clone, Debug, PartialEq)]
pub struct Comment {
    pub maker_id: String,
    pub name: String,
    pub text: String,
    pub posted: u64,
}

#[derive(Clone, Debug)]
pub struct Client {
    /// e.g. "http://localhost:8640"
    pub base: String,
    /// Account token for signed-in calls (uploads under the account, votes, comments).
    pub token: Option<String>,
}

pub type Result<T> = std::result::Result<T, String>;

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
            token: None,
        }
    }

    pub fn with_token(mut self, token: Option<String>) -> Self {
        self.token = token;
        self
    }

    fn auth<B>(&self, req: ureq::RequestBuilder<B>) -> ureq::RequestBuilder<B> {
        match &self.token {
            Some(t) => req.header("Authorization", &format!("Bearer {t}")),
            None => req,
        }
    }

    /// POST a JSON body (signed in when a token is set); the JSON answer.
    fn post_json(&self, path: &str, body: &Value) -> Result<Value> {
        let mut r = self
            .auth(ureq::post(&format!("{}{path}", self.base)))
            .header("Content-Type", "application/json")
            .send(serde_json::to_vec(body).unwrap().as_slice())
            .map_err(|e| e.to_string())?;
        let s = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }

    /// Create an account named `name`: the account and its token (sign in with `with_token`).
    pub fn register(&self, name: &str) -> Result<(Account, String)> {
        let v = self.post_json("/api/accounts", &serde_json::json!({ "name": name }))?;
        let a = Account::from_json(&v["account"]).ok_or("registration refused")?;
        let t = v["token"].as_str().ok_or("no token")?.to_string();
        Ok((a, t))
    }

    /// The signed-in account.
    pub fn me(&self) -> Result<Account> {
        let mut r = self
            .auth(ureq::get(&format!("{}/api/me", self.base)))
            .call()
            .map_err(|e| e.to_string())?;
        let s = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        Account::from_json(&v).ok_or_else(|| "not signed in".into())
    }

    /// Like (true) or boo (false) a course as the signed-in account.
    pub fn vote(&self, code: &str, like: bool) -> Result<Listing> {
        let what = if like { "like" } else { "boo" };
        let v = self.post_json(&format!("/api/courses/{code}/{what}"), &Value::Null)?;
        Listing::from_json(&v).ok_or_else(|| format!("vote refused: {v}"))
    }

    /// Report a clear in `time_ms` (records count when signed in).
    pub fn clear(&self, code: &str, time_ms: u64) -> Result<Listing> {
        let v = self.post_json(
            &format!("/api/courses/{code}/clear"),
            &serde_json::json!({ "time_ms": time_ms }),
        )?;
        Listing::from_json(&v).ok_or_else(|| format!("clear refused: {v}"))
    }

    pub fn comments(&self, code: &str) -> Result<Vec<Comment>> {
        let v = self.get_json(&format!("/api/courses/{code}/comments"))?;
        Ok(v["comments"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|c| {
                        Some(Comment {
                            maker_id: c["maker_id"].as_str()?.to_string(),
                            name: c["name"].as_str().unwrap_or_default().to_string(),
                            text: c["text"].as_str()?.to_string(),
                            posted: c["posted"].as_u64().unwrap_or(0),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default())
    }

    pub fn comment(&self, code: &str, text: &str) -> Result<()> {
        let v = self.post_json(
            &format!("/api/courses/{code}/comments"),
            &serde_json::json!({ "text": text }),
        )?;
        if v.get("error").is_some() {
            return Err(v["error"].as_str().unwrap_or("refused").to_string());
        }
        Ok(())
    }

    /// A maker's profile and courses.
    pub fn maker(&self, maker_id: &str) -> Result<(Account, Vec<Listing>)> {
        let v = self.get_json(&format!("/api/makers/{maker_id}"))?;
        let a = Account::from_json(&v["maker"]).ok_or("no such maker")?;
        Ok((a, Self::list(&v)))
    }

    /// Remove a course (yours, or any with the admin token).
    pub fn delete(&self, code: &str) -> Result<()> {
        self.auth(ureq::delete(&format!("{}/api/courses/{code}", self.base)))
            .call()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    fn get_json(&self, path: &str) -> Result<Value> {
        let mut r = ureq::get(&format!("{}{path}", self.base))
            .call()
            .map_err(|e| e.to_string())?;
        let s = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        serde_json::from_str(&s).map_err(|e| e.to_string())
    }

    fn list(v: &Value) -> Vec<Listing> {
        v["courses"]
            .as_array()
            .map(|a| a.iter().filter_map(Listing::from_json).collect())
            .unwrap_or_default()
    }

    /// Search courses: `q` text, optional style, sort "new" / "popular" / "hard".
    pub fn search(
        &self,
        q: &str,
        style: Option<&str>,
        sort: &str,
        offset: usize,
        limit: usize,
    ) -> Result<Vec<Listing>> {
        let mut path = format!(
            "/api/courses?q={}&sort={sort}&offset={offset}&limit={limit}",
            encode(q)
        );
        if let Some(s) = style {
            path += &format!("&style={s}");
        }
        Ok(Self::list(&self.get_json(&path)?))
    }

    pub fn info(&self, code: &str) -> Result<Listing> {
        Listing::from_json(&self.get_json(&format!("/api/courses/{code}"))?)
            .ok_or_else(|| "bad listing".into())
    }

    /// The course data (raw).
    pub fn download(&self, code: &str) -> Result<Vec<u8>> {
        let mut r = ureq::get(&format!("{}/api/courses/{code}/data", self.base))
            .call()
            .map_err(|e| e.to_string())?;
        r.body_mut()
            .with_config()
            .limit(0x80000)
            .read_to_vec()
            .map_err(|e| e.to_string())
    }

    /// Upload raw course data as `maker`; returns the new listing (with its code).
    pub fn upload(&self, data: &[u8], maker: &str) -> Result<Listing> {
        let mut r = self
            .auth(ureq::post(&format!("{}/api/courses", self.base)))
            .header("X-Maker", maker)
            .header("Content-Type", "application/octet-stream")
            .send(data)
            .map_err(|e| e.to_string())?;
        let s = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        Listing::from_json(&v).ok_or_else(|| format!("upload refused: {s}"))
    }

    /// Report a play ("play") or an anonymous clear ("clear"); likes need an account (`vote`).
    pub fn report(&self, code: &str, what: &str) -> Result<()> {
        ureq::post(&format!("{}/api/courses/{code}/{what}", self.base))
            .send_empty()
            .map(|_| ())
            .map_err(|e| e.to_string())
    }

    /// Random courses with a difficulty value in `band` (100 Mario).
    pub fn pool(&self, band: (u8, u8), count: usize) -> Result<Vec<Listing>> {
        Ok(Self::list(&self.get_json(&format!(
            "/api/pool?min={}&max={}&count={count}",
            band.0, band.1
        ))?))
    }
}

fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' => (b as char).to_string(),
            b' ' => "+".into(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

/// 100 Mario courses from a server. `CourseRef::id` is the course code; the app downloads it
/// before playing. Network errors give an empty search (the run reports "not enough courses").
pub struct RemoteSource {
    pub client: Client,
    /// Only courses in styles the engine plays.
    pub styles: Vec<String>,
}

impl CourseSource for RemoteSource {
    fn search(
        &mut self,
        band: (u8, u8),
        max: usize,
        _castle: bool,
        _rng: &mut Rng,
    ) -> Vec<CourseRef> {
        self.client
            .pool(band, max)
            .unwrap_or_default()
            .into_iter()
            .filter(|l| self.styles.is_empty() || self.styles.contains(&l.style))
            .map(|l| CourseRef {
                id: l.code,
                name: l.name,
                maker: l.maker,
                difficulty: l.difficulty,
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn talks_to_a_live_server() {
        let dir = std::env::temp_dir().join(format!("mmk-online-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = mmk_server::Store::open(&dir).unwrap();
        let port = 20000 + (std::process::id() % 20000) as u16;
        let addr = format!("127.0.0.1:{port}");
        let a2 = addr.clone();
        std::thread::spawn(move || mmk_server::serve(store, &a2));
        std::thread::sleep(std::time::Duration::from_millis(200));
        let c = Client::new(format!("http://{addr}"));
        let mut course = mmk_formats::smm2::Course::blank("M1");
        course.header.name.set("Online test");
        let data = course.to_bytes().unwrap();
        let up = c.upload(&data, "tester").unwrap();
        assert_eq!(up.name, "Online test");
        assert_eq!(c.download(&up.code).unwrap(), data);
        c.report(&up.code, "play").unwrap();
        c.report(&up.code, "clear").unwrap();
        assert_eq!(c.info(&up.code).unwrap().difficulty, 0);
        assert_eq!(c.search("online", None, "new", 0, 10).unwrap().len(), 1);
        let mut src = RemoteSource {
            client: c.clone(),
            styles: vec!["M1".into()],
        };
        let got = src.search((0, 34), 50, false, &mut Rng::new(1));
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].id, up.code);
        // accounts
        let (acc, token) = c.register("Tess").unwrap();
        let me = c.clone().with_token(Some(token));
        assert_eq!(me.me().unwrap().name, "Tess");
        let mine = me.upload(&data, "ignored").unwrap();
        assert_eq!(
            (mine.maker.as_str(), mine.maker_id.as_str()),
            ("Tess", acc.maker_id.as_str())
        );
        assert!(c.vote(&mine.code, true).is_err(), "votes need an account");
        assert_eq!(me.vote(&mine.code, true).unwrap().likes, 1);
        assert_eq!(me.clear(&mine.code, 42_000).unwrap().record_ms, 42_000);
        me.comment(&mine.code, "hi").unwrap();
        assert_eq!(c.comments(&mine.code).unwrap()[0].text, "hi");
        assert_eq!(c.maker(&acc.maker_id).unwrap().1.len(), 1);
        me.delete(&mine.code).unwrap();
        assert!(c.info(&mine.code).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
