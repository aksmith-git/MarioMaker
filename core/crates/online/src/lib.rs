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
    pub difficulty: u8,
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
            difficulty: n("difficulty") as u8,
        })
    }
}

#[derive(Clone, Debug)]
pub struct Client {
    /// e.g. "http://localhost:8640"
    pub base: String,
}

pub type Result<T> = std::result::Result<T, String>;

impl Client {
    pub fn new(base: impl Into<String>) -> Self {
        Self {
            base: base.into().trim_end_matches('/').to_string(),
        }
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
        let mut r = ureq::post(&format!("{}/api/courses", self.base))
            .header("X-Maker", maker)
            .header("Content-Type", "application/octet-stream")
            .send(data)
            .map_err(|e| e.to_string())?;
        let s = r.body_mut().read_to_string().map_err(|e| e.to_string())?;
        let v: Value = serde_json::from_str(&s).map_err(|e| e.to_string())?;
        Listing::from_json(&v).ok_or_else(|| format!("upload refused: {s}"))
    }

    /// Report a play ("play"), a clear ("clear") or a like ("like").
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
        let _ = std::fs::remove_dir_all(&dir);
    }
}
