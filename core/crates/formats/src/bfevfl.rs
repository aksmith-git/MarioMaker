//! BFEVFL ("binary event flow", version 3, little-endian): the event flowcharts that script SMM2's
//! Story Mode (`Pack/QuestEventFlow.pack`) and Yamamura's Dojo lessons (`EventFlow/lesson_*`).
//! Read from inspection of the owner's files; the layout matches the publicly documented
//! `evfl` container used by other Switch games.
//!
//! A flowchart is a graph of events. Each event is one of: an **action** (an actor does something,
//! then go to `next`), a **switch** (ask an actor a query, branch on the integer answer), a
//! **fork** (run several branches, then continue at the join), a **join**, or a **sub-flow** call
//! (run another entry point, then continue). Actors are named (`QuestSystemActor`, an optional
//! secondary name such as a map object) and list the action and query names their events use;
//! events refer to them by index. Entry points name the events where a flow starts.
//!
//! ```text
//! file:       0x00 "BFEVFL\0\0"  0x08 u8 version[4] (0,3,0,0)  0x0C u16 BOM 0xFEFF
//!             0x10 u32 → file name characters  0x18 u32 relocation table  0x1C u32 file size
//!             0x20 u16 flowcharts  0x22 u16 timelines  0x28 u64 → flowchart offsets
//!             0x30 u64 → flowchart dictionary  0x38 / 0x40 timelines (unused here)
//! flowchart:  0x00 "EVFL"  0x10 u16 actors, actions, queries, events, entry points
//!             0x20 u64 name  0x28 actors  0x30 events  0x38 entry point dictionary
//!             0x40 entry points
//! actor:      0x38 bytes: name, secondary name, argument name, → action names, → query names,
//!             → parameter container (u64 each), u16 actions, u16 queries, u16 entry point
//! event:      0x28 bytes: u64 name, u8 type, then by type —
//!             action  0x0A u16 next, 0x0C u16 actor, 0x0E u16 action, 0x10 → params
//!             switch  0x0A u16 cases, 0x0C u16 actor, 0x0E u16 query, 0x10 → params,
//!                     0x18 → cases (u32 value, u16 event, pad to 8)
//!             fork    0x0A u16 forks, 0x0C u16 join, 0x10 → u16 event × forks
//!             join    0x0A u16 next
//!             subflow 0x0A u16 next, 0x10 → params, 0x18 flowchart name, 0x20 entry point name
//! entry:      0x20 bytes: → sub-flow event indices, → variable dictionary, → variable definitions,
//!             u16 sub-flow events, u16 variables, u16 main event
//! variable:   0x10 bytes: value (inline scalar or → array), u16 count, u16 type
//! container:  u8 type, u16 count at 2, → dictionary at 8 (containers), values from 0x10
//! dictionary: "DIC " u32 count, then (count + 1) 0x10-byte nodes; node + 8 = u64 name
//! string:     u16 length, bytes, NUL
//! ```
//!
//! Offsets are file offsets (the relocation table only matters to a loader that turns them into
//! pointers in place). Event index 0xFFFF means "none" (end of the flow).

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "BFEVFL",
        detail: detail.into(),
    }
}

/// "No event" in next / join / main-event fields.
pub const NONE: u16 = 0xFFFF;

/// A parameter value (event parameters, actor parameters, entry-point variables).
#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    /// A reference to a variable of the running entry point (a sub-flow argument).
    Argument(String),
    Container(Params),
    Int(i32),
    Bool(bool),
    Float(f32),
    String(String),
    WString(String),
    IntArray(Vec<i32>),
    BoolArray(Vec<bool>),
    FloatArray(Vec<f32>),
    StringArray(Vec<String>),
    WStringArray(Vec<String>),
    /// An actor named by (name, secondary name).
    Actor(String, String),
}

impl Value {
    pub fn as_int(&self) -> Option<i32> {
        match self {
            Value::Int(v) => Some(*v),
            Value::Bool(b) => Some(*b as i32),
            Value::Float(f) => Some(*f as i32),
            _ => None,
        }
    }
    pub fn as_float(&self) -> Option<f32> {
        match self {
            Value::Float(f) => Some(*f),
            Value::Int(v) => Some(*v as f32),
            _ => None,
        }
    }
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            Value::Int(v) => Some(*v != 0),
            _ => None,
        }
    }
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) | Value::WString(s) => Some(s),
            _ => None,
        }
    }
}

/// Named values in file order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Params(pub Vec<(String, Value)>);

impl Params {
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.0.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.0.iter().map(|(k, v)| (k.as_str(), v))
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Actor {
    pub name: String,
    /// Secondary name: which instance (a map object, a camera, …); empty for the plain actor.
    pub secondary: String,
    pub argument: String,
    pub actions: Vec<String>,
    pub queries: Vec<String>,
    pub params: Params,
    pub entry_point: u16,
}

#[derive(Clone, Debug, PartialEq)]
pub enum EventKind {
    Action {
        actor: u16,
        action: u16,
        params: Params,
        next: u16,
    },
    Switch {
        actor: u16,
        query: u16,
        params: Params,
        /// (answer, event) pairs; an answer with no case ends the flow.
        cases: Vec<(u32, u16)>,
    },
    Fork {
        forks: Vec<u16>,
        join: u16,
    },
    Join {
        next: u16,
    },
    SubFlow {
        /// Flowchart to call into; empty = this flowchart.
        flowchart: String,
        entry_point: String,
        params: Params,
        next: u16,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Event {
    pub name: String,
    pub kind: EventKind,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EntryPoint {
    pub name: String,
    pub main_event: u16,
    pub sub_flow_events: Vec<u16>,
    /// Variables with their default values (sub-flow arguments).
    pub variables: Params,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Flowchart {
    pub name: String,
    pub actors: Vec<Actor>,
    pub events: Vec<Event>,
    pub entry_points: Vec<EntryPoint>,
}

impl Flowchart {
    pub fn entry_point(&self, name: &str) -> Option<&EntryPoint> {
        self.entry_points.iter().find(|e| e.name == name)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct EventFlowFile {
    pub name: String,
    pub flowcharts: Vec<Flowchart>,
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
    fn bytes(&self, o: usize, n: usize) -> Result<&[u8]> {
        o.checked_add(n)
            .and_then(|end| self.0.get(o..end))
            .ok_or_else(|| bad(format!("read of {n} bytes at {o:#x} past the end")))
    }
    fn u8(&self, o: usize) -> Result<u8> {
        Ok(self.bytes(o, 1)?[0])
    }
    fn u16(&self, o: usize) -> Result<u16> {
        Ok(u16::from_le_bytes(self.bytes(o, 2)?.try_into().unwrap()))
    }
    fn u32(&self, o: usize) -> Result<u32> {
        Ok(u32::from_le_bytes(self.bytes(o, 4)?.try_into().unwrap()))
    }
    fn i32(&self, o: usize) -> Result<i32> {
        Ok(self.u32(o)? as i32)
    }
    fn f32(&self, o: usize) -> Result<f32> {
        Ok(f32::from_bits(self.u32(o)?))
    }
    fn ptr(&self, o: usize) -> Result<usize> {
        let v = u64::from_le_bytes(self.bytes(o, 8)?.try_into().unwrap());
        usize::try_from(v).map_err(|_| bad(format!("offset {v:#x} at {o:#x}")))
    }

    /// A length-prefixed string at `p` (0 = empty).
    fn string(&self, p: usize) -> Result<String> {
        if p == 0 {
            return Ok(String::new());
        }
        let n = self.u16(p)? as usize;
        Ok(String::from_utf8_lossy(self.bytes(p + 2, n)?).into_owned())
    }
    fn string_at(&self, o: usize) -> Result<String> {
        self.string(self.ptr(o)?)
    }

    fn dict(&self, p: usize) -> Result<Vec<String>> {
        if p == 0 {
            return Ok(Vec::new());
        }
        if self.bytes(p, 4)? != b"DIC " {
            return Err(bad(format!("dictionary at {p:#x}")));
        }
        let n = self.u32(p + 4)? as usize;
        (0..n)
            .map(|i| self.string_at(p + 8 + 0x10 * (i + 1) + 8))
            .collect()
    }

    fn strings(&self, p: usize, n: usize) -> Result<Vec<String>> {
        (0..n).map(|i| self.string_at(p + 8 * i)).collect()
    }

    /// A scalar or array of `kind` stored at `v` (`count` items).
    fn value(&self, kind: u8, v: usize, count: usize) -> Result<Value> {
        let arr = |size: usize| (0..count).map(move |i| v + size * i);
        Ok(match kind {
            0 => Value::Argument(self.string_at(v)?),
            2 => Value::Int(self.i32(v)?),
            3 => Value::Bool(self.u32(v)? != 0),
            4 => Value::Float(self.f32(v)?),
            5 => Value::String(self.string_at(v)?),
            6 => Value::WString(self.string_at(v)?),
            7 => Value::IntArray(arr(4).map(|o| self.i32(o)).collect::<Result<_>>()?),
            8 => Value::BoolArray(
                arr(4)
                    .map(|o| self.u32(o).map(|x| x != 0))
                    .collect::<Result<_>>()?,
            ),
            9 => Value::FloatArray(arr(4).map(|o| self.f32(o)).collect::<Result<_>>()?),
            10 => Value::StringArray(arr(8).map(|o| self.string_at(o)).collect::<Result<_>>()?),
            11 => Value::WStringArray(arr(8).map(|o| self.string_at(o)).collect::<Result<_>>()?),
            12 => Value::Actor(self.string_at(v)?, self.string_at(v + 8)?),
            t => return Err(bad(format!("value type {t} at {v:#x}"))),
        })
    }

    fn container_value(&self, p: usize, depth: usize) -> Result<Value> {
        if depth > 16 {
            return Err(bad("containers nested too deep"));
        }
        let kind = self.u8(p)?;
        let n = self.u16(p + 2)? as usize;
        if kind == 1 {
            let names = self.dict(self.ptr(p + 8)?)?;
            let mut out = Vec::with_capacity(n);
            for i in 0..n {
                let child = self.ptr(p + 0x10 + 8 * i)?;
                let name = names.get(i).cloned().unwrap_or_else(|| format!("#{i}"));
                out.push((name, self.container_value(child, depth + 1)?));
            }
            return Ok(Value::Container(Params(out)));
        }
        self.value(kind, p + 0x10, n)
    }

    fn params(&self, p: usize) -> Result<Params> {
        if p == 0 {
            return Ok(Params::default());
        }
        match self.container_value(p, 0)? {
            Value::Container(c) => Ok(c),
            other => Ok(Params(vec![(String::new(), other)])),
        }
    }

    fn flowchart(&self, p: usize) -> Result<Flowchart> {
        if self.bytes(p, 4)? != b"EVFL" {
            return Err(bad(format!("flowchart at {p:#x}")));
        }
        let n_actors = self.u16(p + 0x10)? as usize;
        let n_events = self.u16(p + 0x16)? as usize;
        let n_entries = self.u16(p + 0x18)? as usize;
        let name = self.string_at(p + 0x20)?;
        let (ap, ep, edp, eep) = (
            self.ptr(p + 0x28)?,
            self.ptr(p + 0x30)?,
            self.ptr(p + 0x38)?,
            self.ptr(p + 0x40)?,
        );
        let mut actors = Vec::with_capacity(n_actors);
        for i in 0..n_actors {
            let a = ap + 0x38 * i;
            let n_act = self.u16(a + 0x30)? as usize;
            let n_q = self.u16(a + 0x32)? as usize;
            actors.push(Actor {
                name: self.string_at(a)?,
                secondary: self.string_at(a + 8)?,
                argument: self.string_at(a + 0x10)?,
                actions: self.strings(self.ptr(a + 0x18)?, n_act)?,
                queries: self.strings(self.ptr(a + 0x20)?, n_q)?,
                params: self.params(self.ptr(a + 0x28)?)?,
                entry_point: self.u16(a + 0x34)?,
            });
        }
        let mut events = Vec::with_capacity(n_events);
        for i in 0..n_events {
            let e = ep + 0x28 * i;
            let name = self.string_at(e)?;
            let kind = match self.u8(e + 8)? {
                0 => EventKind::Action {
                    next: self.u16(e + 0xA)?,
                    actor: self.u16(e + 0xC)?,
                    action: self.u16(e + 0xE)?,
                    params: self.params(self.ptr(e + 0x10)?)?,
                },
                1 => {
                    let n = self.u16(e + 0xA)? as usize;
                    let cp = self.ptr(e + 0x18)?;
                    let cases = (0..n)
                        .map(|k| Ok((self.u32(cp + 8 * k)?, self.u16(cp + 8 * k + 4)?)))
                        .collect::<Result<_>>()?;
                    EventKind::Switch {
                        actor: self.u16(e + 0xC)?,
                        query: self.u16(e + 0xE)?,
                        params: self.params(self.ptr(e + 0x10)?)?,
                        cases,
                    }
                }
                2 => {
                    let n = self.u16(e + 0xA)? as usize;
                    let fp = self.ptr(e + 0x10)?;
                    EventKind::Fork {
                        join: self.u16(e + 0xC)?,
                        forks: (0..n)
                            .map(|k| self.u16(fp + 2 * k))
                            .collect::<Result<_>>()?,
                    }
                }
                3 => EventKind::Join {
                    next: self.u16(e + 0xA)?,
                },
                4 => EventKind::SubFlow {
                    next: self.u16(e + 0xA)?,
                    params: self.params(self.ptr(e + 0x10)?)?,
                    flowchart: self.string_at(e + 0x18)?,
                    entry_point: self.string_at(e + 0x20)?,
                },
                t => return Err(bad(format!("event {i} of {name:?}: type {t}"))),
            };
            events.push(Event { name, kind });
        }
        let entry_names = self.dict(edp)?;
        let mut entry_points = Vec::with_capacity(n_entries);
        for i in 0..n_entries {
            let q = eep + 0x20 * i;
            let n_sub = self.u16(q + 0x18)? as usize;
            let n_var = self.u16(q + 0x1A)? as usize;
            let sp = self.ptr(q)?;
            let sub_flow_events = (0..n_sub)
                .map(|k| self.u16(sp + 2 * k))
                .collect::<Result<_>>()?;
            let mut variables = Vec::with_capacity(n_var);
            if n_var > 0 {
                let vp = self.ptr(q + 0x10)?;
                let names = self.dict(self.ptr(q + 8)?)?;
                for k in 0..n_var {
                    let v = vp + 0x10 * k;
                    let count = self.u16(v + 8)? as usize;
                    let kind = self.u16(v + 0xA)? as u8;
                    // arrays hold a pointer; scalars are inline
                    let at = if matches!(kind, 7..=11) {
                        self.ptr(v)?
                    } else {
                        v
                    };
                    let name = names.get(k).cloned().unwrap_or_else(|| format!("#{k}"));
                    variables.push((name, self.value(kind, at, count)?));
                }
            }
            entry_points.push(EntryPoint {
                name: entry_names.get(i).cloned().unwrap_or_else(|| i.to_string()),
                main_event: self.u16(q + 0x1C)?,
                sub_flow_events,
                variables: Params(variables),
            });
        }
        let fc = Flowchart {
            name,
            actors,
            events,
            entry_points,
        };
        fc.validate()?;
        Ok(fc)
    }
}

impl Flowchart {
    /// Every event, actor, action and query index points inside the flowchart.
    fn validate(&self) -> Result<()> {
        let ne = self.events.len();
        let ev_ok = |k: u16| k == NONE || (k as usize) < ne;
        for (i, e) in self.events.iter().enumerate() {
            let fail = |what: &str| Err(bad(format!("{}: event {i} {what}", self.name)));
            match &e.kind {
                EventKind::Action {
                    actor,
                    action,
                    next,
                    ..
                } => {
                    let Some(a) = self.actors.get(*actor as usize) else {
                        return fail("actor out of range");
                    };
                    if *action as usize >= a.actions.len() || !ev_ok(*next) {
                        return fail("action or next out of range");
                    }
                }
                EventKind::Switch {
                    actor,
                    query,
                    cases,
                    ..
                } => {
                    let Some(a) = self.actors.get(*actor as usize) else {
                        return fail("actor out of range");
                    };
                    if *query as usize >= a.queries.len() || !cases.iter().all(|c| ev_ok(c.1)) {
                        return fail("query or case out of range");
                    }
                }
                EventKind::Fork { forks, join } => {
                    if !forks.iter().all(|&k| ev_ok(k)) || !ev_ok(*join) {
                        return fail("fork out of range");
                    }
                }
                EventKind::Join { next } | EventKind::SubFlow { next, .. } => {
                    if !ev_ok(*next) {
                        return fail("next out of range");
                    }
                }
            }
        }
        if let Some(e) = self.entry_points.iter().find(|e| !ev_ok(e.main_event)) {
            return Err(bad(format!("{}: entry point {}", self.name, e.name)));
        }
        Ok(())
    }
}

/// Parse a BFEVFL file.
pub fn parse(data: &[u8]) -> Result<EventFlowFile> {
    let r = Reader(data);
    if r.bytes(0, 8)? != b"BFEVFL\0\0" {
        return Err(Error::Magic { what: "BFEVFL" });
    }
    if r.u16(0xC)? != 0xFEFF {
        return Err(bad("only little-endian files are supported"));
    }
    // the file name field points at the characters, after the length
    let name = match r.u32(0x10)? as usize {
        0 | 1 => String::new(),
        p => r.string(p - 2)?,
    };
    let n = r.u16(0x20)? as usize;
    let fp = r.ptr(0x28)?;
    let flowcharts = (0..n)
        .map(|i| r.flowchart(r.ptr(fp + 8 * i)?))
        .collect::<Result<_>>()?;
    Ok(EventFlowFile { name, flowcharts })
}

/// Write `file` in the layout [`parse`] reads. Meant for tests and tools: dictionaries carry no
/// search trie and there is no relocation table, so the game itself would not load the result.
pub fn build(file: &EventFlowFile) -> Vec<u8> {
    let mut w = Writer::default();
    w.buf.extend_from_slice(b"BFEVFL\0\0");
    w.buf.extend_from_slice(&[0, 3, 0, 0]);
    w.buf.extend_from_slice(&0xFEFFu16.to_le_bytes());
    w.buf.extend_from_slice(&[3, 0]);
    w.buf.resize(0x48, 0);
    let fname = w.string(&file.name);
    w.put_u32(0x10, fname as u32 + 2);
    w.put_u16(0x20, file.flowcharts.len() as u16);
    let table = w.alloc(8 * file.flowcharts.len());
    w.put_ptr(0x28, table);
    let names: Vec<&str> = file.flowcharts.iter().map(|f| f.name.as_str()).collect();
    let dict = w.dict(&names);
    w.put_ptr(0x30, dict);
    for (i, fc) in file.flowcharts.iter().enumerate() {
        let p = w.flowchart(fc);
        w.put_ptr(table + 8 * i, p);
    }
    let size = w.buf.len() as u32;
    w.put_u32(0x1C, size);
    w.buf
}

#[derive(Default)]
struct Writer {
    buf: Vec<u8>,
}

impl Writer {
    fn alloc(&mut self, n: usize) -> usize {
        while !self.buf.len().is_multiple_of(8) {
            self.buf.push(0);
        }
        let at = self.buf.len();
        self.buf.resize(at + n, 0);
        at
    }
    fn put(&mut self, at: usize, b: &[u8]) {
        self.buf[at..at + b.len()].copy_from_slice(b);
    }
    fn put_u16(&mut self, at: usize, v: u16) {
        self.put(at, &v.to_le_bytes());
    }
    fn put_u32(&mut self, at: usize, v: u32) {
        self.put(at, &v.to_le_bytes());
    }
    fn put_ptr(&mut self, at: usize, v: usize) {
        self.put(at, &(v as u64).to_le_bytes());
    }
    fn string(&mut self, s: &str) -> usize {
        let at = self.alloc(2 + s.len() + 1);
        self.put_u16(at, s.len() as u16);
        self.put(at + 2, s.as_bytes());
        at
    }
    fn dict(&mut self, names: &[&str]) -> usize {
        let at = self.alloc(8 + 0x10 * (names.len() + 1));
        self.put(at, b"DIC ");
        self.put_u32(at + 4, names.len() as u32);
        for (i, n) in names.iter().enumerate() {
            let s = self.string(n);
            self.put_ptr(at + 8 + 0x10 * (i + 1) + 8, s);
        }
        at
    }
    fn strings(&mut self, list: &[String]) -> usize {
        let at = self.alloc(8 * list.len());
        for (i, s) in list.iter().enumerate() {
            let p = self.string(s);
            self.put_ptr(at + 8 * i, p);
        }
        at
    }
    fn u16s(&mut self, list: &[u16]) -> usize {
        let at = self.alloc(2 * list.len());
        for (i, v) in list.iter().enumerate() {
            self.put_u16(at + 2 * i, *v);
        }
        at
    }

    /// Write `v`'s payload at `at` (scalars inline, arrays as items); returns (type, count).
    fn value_into(&mut self, at: usize, v: &Value) -> (u8, usize) {
        match v {
            Value::Argument(s) | Value::String(s) | Value::WString(s) => {
                let p = self.string(s);
                self.put_ptr(at, p);
                let t = match v {
                    Value::Argument(_) => 0,
                    Value::String(_) => 5,
                    _ => 6,
                };
                (t, 1)
            }
            Value::Int(x) => {
                self.put_u32(at, *x as u32);
                (2, 1)
            }
            Value::Bool(b) => {
                self.put_u32(at, *b as u32);
                (3, 1)
            }
            Value::Float(f) => {
                self.put_u32(at, f.to_bits());
                (4, 1)
            }
            Value::Actor(a, b) => {
                let (pa, pb) = (self.string(a), self.string(b));
                self.put_ptr(at, pa);
                self.put_ptr(at + 8, pb);
                (12, 1)
            }
            Value::Container(_) => unreachable!("containers are written by container()"),
            _ => unreachable!("arrays are written by container()"),
        }
    }

    fn array_words(v: &Value) -> Option<(u8, Vec<u32>)> {
        Some(match v {
            Value::IntArray(a) => (7, a.iter().map(|x| *x as u32).collect()),
            Value::BoolArray(a) => (8, a.iter().map(|x| *x as u32).collect()),
            Value::FloatArray(a) => (9, a.iter().map(|x| x.to_bits()).collect()),
            _ => return None,
        })
    }

    fn container(&mut self, v: &Value) -> usize {
        match v {
            Value::Container(params) => {
                let at = self.alloc(0x10 + 8 * params.0.len());
                self.buf[at] = 1;
                self.put_u16(at + 2, params.0.len() as u16);
                let names: Vec<&str> = params.0.iter().map(|(k, _)| k.as_str()).collect();
                let d = self.dict(&names);
                self.put_ptr(at + 8, d);
                for (i, (_, child)) in params.0.iter().enumerate() {
                    let c = self.container(child);
                    self.put_ptr(at + 0x10 + 8 * i, c);
                }
                at
            }
            Value::StringArray(a) | Value::WStringArray(a) => {
                let at = self.alloc(0x10 + 8 * a.len());
                self.buf[at] = if matches!(v, Value::StringArray(_)) {
                    10
                } else {
                    11
                };
                self.put_u16(at + 2, a.len() as u16);
                for (i, s) in a.iter().enumerate() {
                    let p = self.string(s);
                    self.put_ptr(at + 0x10 + 8 * i, p);
                }
                at
            }
            _ => {
                if let Some((t, words)) = Self::array_words(v) {
                    let at = self.alloc(0x10 + 4 * words.len());
                    self.buf[at] = t;
                    self.put_u16(at + 2, words.len() as u16);
                    for (i, w) in words.iter().enumerate() {
                        self.put_u32(at + 0x10 + 4 * i, *w);
                    }
                    return at;
                }
                let at = self.alloc(0x20);
                let (t, n) = self.value_into(at + 0x10, v);
                self.buf[at] = t;
                self.put_u16(at + 2, n as u16);
                at
            }
        }
    }

    fn params(&mut self, p: &Params) -> usize {
        if p.0.is_empty() {
            0
        } else {
            self.container(&Value::Container(p.clone()))
        }
    }

    fn flowchart(&mut self, fc: &Flowchart) -> usize {
        let p = self.alloc(0x48);
        self.put(p, b"EVFL");
        self.put_u16(p + 0x10, fc.actors.len() as u16);
        let n_act: usize = fc.actors.iter().map(|a| a.actions.len()).sum();
        let n_q: usize = fc.actors.iter().map(|a| a.queries.len()).sum();
        self.put_u16(p + 0x12, n_act as u16);
        self.put_u16(p + 0x14, n_q as u16);
        self.put_u16(p + 0x16, fc.events.len() as u16);
        self.put_u16(p + 0x18, fc.entry_points.len() as u16);
        let name = self.string(&fc.name);
        self.put_ptr(p + 0x20, name);
        let ap = self.alloc(0x38 * fc.actors.len());
        self.put_ptr(p + 0x28, ap);
        for (i, a) in fc.actors.iter().enumerate() {
            let b = ap + 0x38 * i;
            let (n, s, g) = (
                self.string(&a.name),
                self.string(&a.secondary),
                self.string(&a.argument),
            );
            self.put_ptr(b, n);
            self.put_ptr(b + 8, s);
            self.put_ptr(b + 0x10, g);
            let acts = self.strings(&a.actions);
            let qs = self.strings(&a.queries);
            let prm = self.params(&a.params);
            self.put_ptr(b + 0x18, acts);
            self.put_ptr(b + 0x20, qs);
            self.put_ptr(b + 0x28, prm);
            self.put_u16(b + 0x30, a.actions.len() as u16);
            self.put_u16(b + 0x32, a.queries.len() as u16);
            self.put_u16(b + 0x34, a.entry_point);
        }
        let ep = self.alloc(0x28 * fc.events.len());
        self.put_ptr(p + 0x30, ep);
        for (i, e) in fc.events.iter().enumerate() {
            let b = ep + 0x28 * i;
            let n = self.string(&e.name);
            self.put_ptr(b, n);
            match &e.kind {
                EventKind::Action {
                    actor,
                    action,
                    params,
                    next,
                } => {
                    self.buf[b + 8] = 0;
                    self.put_u16(b + 0xA, *next);
                    self.put_u16(b + 0xC, *actor);
                    self.put_u16(b + 0xE, *action);
                    let prm = self.params(params);
                    self.put_ptr(b + 0x10, prm);
                }
                EventKind::Switch {
                    actor,
                    query,
                    params,
                    cases,
                } => {
                    self.buf[b + 8] = 1;
                    self.put_u16(b + 0xA, cases.len() as u16);
                    self.put_u16(b + 0xC, *actor);
                    self.put_u16(b + 0xE, *query);
                    let prm = self.params(params);
                    self.put_ptr(b + 0x10, prm);
                    let cp = self.alloc(8 * cases.len());
                    for (k, (v, ev)) in cases.iter().enumerate() {
                        self.put_u32(cp + 8 * k, *v);
                        self.put_u16(cp + 8 * k + 4, *ev);
                    }
                    self.put_ptr(b + 0x18, cp);
                }
                EventKind::Fork { forks, join } => {
                    self.buf[b + 8] = 2;
                    self.put_u16(b + 0xA, forks.len() as u16);
                    self.put_u16(b + 0xC, *join);
                    let fp = self.u16s(forks);
                    self.put_ptr(b + 0x10, fp);
                }
                EventKind::Join { next } => {
                    self.buf[b + 8] = 3;
                    self.put_u16(b + 0xA, *next);
                }
                EventKind::SubFlow {
                    flowchart,
                    entry_point,
                    params,
                    next,
                } => {
                    self.buf[b + 8] = 4;
                    self.put_u16(b + 0xA, *next);
                    let prm = self.params(params);
                    self.put_ptr(b + 0x10, prm);
                    let (f, e) = (self.string(flowchart), self.string(entry_point));
                    self.put_ptr(b + 0x18, f);
                    self.put_ptr(b + 0x20, e);
                }
            }
        }
        let names: Vec<&str> = fc.entry_points.iter().map(|e| e.name.as_str()).collect();
        let d = self.dict(&names);
        self.put_ptr(p + 0x38, d);
        let eep = self.alloc(0x20 * fc.entry_points.len());
        self.put_ptr(p + 0x40, eep);
        for (i, e) in fc.entry_points.iter().enumerate() {
            let q = eep + 0x20 * i;
            let sp = self.u16s(&e.sub_flow_events);
            self.put_ptr(q, sp);
            if !e.variables.0.is_empty() {
                let vp = self.alloc(0x10 * e.variables.0.len());
                for (k, (_, v)) in e.variables.0.iter().enumerate() {
                    let at = vp + 0x10 * k;
                    let (t, n) = if let Some((t, words)) = Self::array_words(v) {
                        let arr = self.alloc(4 * words.len());
                        for (j, w) in words.iter().enumerate() {
                            self.put_u32(arr + 4 * j, *w);
                        }
                        self.put_ptr(at, arr);
                        (t, words.len())
                    } else {
                        self.value_into(at, v)
                    };
                    self.put_u16(at + 8, n as u16);
                    self.put_u16(at + 0xA, t as u16);
                }
                let names: Vec<&str> = e.variables.0.iter().map(|(k, _)| k.as_str()).collect();
                let d = self.dict(&names);
                self.put_ptr(q + 0x10, vp);
                self.put_ptr(q + 8, d);
            }
            self.put_u16(q + 0x18, e.sub_flow_events.len() as u16);
            self.put_u16(q + 0x1A, e.variables.0.len() as u16);
            self.put_u16(q + 0x1C, e.main_event);
        }
        p
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> EventFlowFile {
        let p = |v: Vec<(&str, Value)>| {
            Params(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
        };
        EventFlowFile {
            name: "test_flow".into(),
            flowcharts: vec![Flowchart {
                name: "test_flow".into(),
                actors: vec![
                    Actor {
                        name: "System".into(),
                        actions: vec!["Wait".into(), "Say".into()],
                        queries: vec!["Pick".into()],
                        ..Default::default()
                    },
                    Actor {
                        name: "Camera".into(),
                        secondary: "Near".into(),
                        actions: vec!["Move".into()],
                        params: p(vec![("speed", Value::Float(1.5))]),
                        ..Default::default()
                    },
                ],
                events: vec![
                    Event {
                        name: "Event0".into(),
                        kind: EventKind::Switch {
                            actor: 0,
                            query: 0,
                            params: p(vec![("n", Value::Int(3))]),
                            cases: vec![(0, 1), (1, 2)],
                        },
                    },
                    Event {
                        name: "Event1".into(),
                        kind: EventKind::Action {
                            actor: 0,
                            action: 1,
                            params: p(vec![
                                ("text", Value::String("label".into())),
                                ("frames", Value::Argument("frames".into())),
                                ("list", Value::IntArray(vec![1, -2])),
                                ("names", Value::StringArray(vec!["a".into(), "b".into()])),
                                ("who", Value::Actor("Camera".into(), "Near".into())),
                                ("on", Value::Bool(true)),
                            ]),
                            next: 3,
                        },
                    },
                    Event {
                        name: "Event2".into(),
                        kind: EventKind::SubFlow {
                            flowchart: String::new(),
                            entry_point: "Sub".into(),
                            params: p(vec![("frames", Value::Int(30))]),
                            next: NONE,
                        },
                    },
                    Event {
                        name: "Event3".into(),
                        kind: EventKind::Fork {
                            forks: vec![4, 5],
                            join: 6,
                        },
                    },
                    Event {
                        name: "Event4".into(),
                        kind: EventKind::Action {
                            actor: 1,
                            action: 0,
                            params: Params::default(),
                            next: 6,
                        },
                    },
                    Event {
                        name: "Event5".into(),
                        kind: EventKind::Action {
                            actor: 0,
                            action: 0,
                            params: p(vec![("frames", Value::Argument("frames".into()))]),
                            next: 6,
                        },
                    },
                    Event {
                        name: "Event6".into(),
                        kind: EventKind::Join { next: NONE },
                    },
                ],
                entry_points: vec![
                    EntryPoint {
                        name: "Main".into(),
                        main_event: 0,
                        ..Default::default()
                    },
                    EntryPoint {
                        name: "Sub".into(),
                        main_event: 3,
                        sub_flow_events: vec![2],
                        variables: p(vec![("frames", Value::Int(10))]),
                    },
                ],
            }],
        }
    }

    #[test]
    fn round_trips_through_the_writer() {
        let f = sample();
        let bytes = build(&f);
        assert_eq!(&bytes[..6], b"BFEVFL");
        let back = parse(&bytes).unwrap();
        assert_eq!(back, f);
        let fc = &back.flowcharts[0];
        assert_eq!(fc.entry_point("Sub").unwrap().main_event, 3);
        assert_eq!(fc.actors[1].params.get("speed"), Some(&Value::Float(1.5)));
    }

    /// The owner's flows (skipped without a RomFS): every Story and lesson flow parses, and the
    /// largest one has the shape the research decoder reported.
    #[test]
    fn owner_flows_parse() {
        use crate::archive::{Sarc, unwrap};
        let Some(romfs) = crate::bfres::tests::owner_romfs() else {
            eprintln!("skipped: no RomFS");
            return;
        };
        let pack = std::fs::read(romfs.join("Pack/QuestEventFlow.pack")).unwrap();
        let sarc = Sarc::parse(unwrap(pack).unwrap()).unwrap();
        let mut quest = 0;
        for (name, data) in sarc.files().filter(|(n, _)| n.ends_with(".bfevfl")) {
            let f = parse(data).unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(f.flowcharts.len(), 1, "{name}");
            if f.name == "quest_SceneEnter" {
                let fc = &f.flowcharts[0];
                assert_eq!(
                    (fc.actors.len(), fc.events.len(), fc.entry_points.len()),
                    (165, 877, 18)
                );
                assert!(fc.entry_point("course_clear").is_some());
            }
            quest += 1;
        }
        assert!(quest >= 20, "{quest} quest flows");
        let mut lessons = 0;
        for e in std::fs::read_dir(romfs.join("EventFlow")).unwrap() {
            let p = e.unwrap().path();
            let f = parse(&std::fs::read(&p).unwrap())
                .unwrap_or_else(|e| panic!("{}: {e}", p.display()));
            lessons += f.name.starts_with("lesson_") as usize;
        }
        assert_eq!(lessons, 51, "50 lessons + the shared sub-flows");
    }

    #[test]
    fn rejects_bad_input() {
        assert!(parse(b"nope").is_err());
        let mut bytes = build(&sample());
        assert!(parse(&bytes[..0x40]).is_err());
        // point an action at a missing event
        let mut broken = sample();
        if let EventKind::Action { next, .. } = &mut broken.flowcharts[0].events[1].kind {
            *next = 40;
        }
        assert!(parse(&build(&broken)).is_err());
        bytes[0] = b'X';
        assert!(matches!(parse(&bytes), Err(Error::Magic { .. })));
    }
}
