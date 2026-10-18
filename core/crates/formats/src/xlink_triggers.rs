//! XLNK action triggers: which event keys a user plays while one of its actions (an animation or
//! a state, per action slot) is active. Kept apart from [`crate::xlink`] (shared by the sound and
//! effect importers); this reads the same header itself.
//!
//! Layout (version 0x1C), from inspection of the owner's files:
//!
//! ```text
//! user header (12 × i32): [1] local properties, [2] calls, [5] action slots, [6] actions,
//!       [7] action triggers, [11] trigger table offset (from the user)
//! trigger table: slots (u32 name pos, i16 first action, i16 last action),
//!       actions (u32 name pos, u32 first trigger, u32 last trigger),
//!       action triggers (24 bytes: u32 guid, u32 call offset (32-byte calls), i32 start frame,
//!       i32 end frame (0x7FFFFFFF: while the action lasts), u16 flags, u16, i32 overwrite
//!       param pos (−1: none))
//! ```

use crate::error::{Error, Result};

fn bad(detail: impl Into<String>) -> Error {
    Error::Format {
        what: "XLNK triggers",
        detail: detail.into(),
    }
}

fn u32_at(d: &[u8], o: usize) -> Result<u32> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| bad(format!("truncated at {o:#x}")))
}

fn i32_at(d: &[u8], o: usize) -> Result<i32> {
    u32_at(d, o).map(|v| v as i32)
}

fn cstr(d: &[u8], o: usize) -> Result<String> {
    let rest = d.get(o..).ok_or_else(|| bad("string past end"))?;
    let end = rest
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| bad("unterminated string"))?;
    Ok(String::from_utf8_lossy(&rest[..end]).into_owned())
}

/// One action trigger: while `action` (in `slot`) is active, play the call `key` from frame
/// `start` (to `end`, or for as long as the action lasts when `end` is None).
#[derive(Clone, Debug, PartialEq)]
pub struct ActionTrigger {
    pub slot: String,
    pub action: String,
    pub key: String,
    pub start: i32,
    pub end: Option<i32>,
    pub flags: u16,
}

/// The action triggers of user `name` (None: no such user).
pub fn action_triggers(d: &[u8], name: &str) -> Result<Option<Vec<ActionTrigger>>> {
    if d.len() < 0x48 || &d[0..4] != b"XLNK" {
        return Err(Error::Magic { what: "XLNK" });
    }
    let n_users = i32_at(d, 0x3C)?.max(0) as usize;
    let name_pos = u32_at(d, 0x44)? as usize;
    let crc = crc32fast::hash(name.as_bytes());
    let mut user = None;
    for i in 0..n_users {
        if u32_at(d, 0x48 + 4 * i)? == crc {
            user = Some(u32_at(d, 0x48 + 4 * (n_users + i))? as usize);
            break;
        }
    }
    let Some(u) = user else { return Ok(None) };
    let def = 0x48 + 8 * n_users;
    let user_params = u32_at(d, def + 4)? as usize;
    let h = |k: usize| i32_at(d, u + 4 * k).map(|v| v.max(0) as usize);
    let (n_local, n_call) = (h(1)?, h(2)?);
    let (n_slot, n_action, n_trig) = (h(5)?, h(6)?, h(7)?);
    let mut calls = 4 * n_local + 4 * user_params + 2 * n_call + u + 48;
    calls = (calls + 3) & !3;
    let key_of = |offset: usize| -> Result<String> {
        if offset / 32 >= n_call {
            return Err(bad("trigger call outside the call table"));
        }
        cstr(d, name_pos + u32_at(d, calls + offset)? as usize)
    };
    let t = u + h(11)?;
    let mut slots = Vec::with_capacity(n_slot);
    for s in 0..n_slot {
        let o = t + 8 * s;
        let first = i16::from_le_bytes([d[o + 4], d[o + 5]]);
        let last = i16::from_le_bytes([d[o + 6], d[o + 7]]);
        slots.push((cstr(d, name_pos + u32_at(d, o)? as usize)?, first, last));
    }
    let a0 = t + 8 * n_slot;
    let t0 = a0 + 12 * n_action;
    let mut out = Vec::new();
    for a in 0..n_action {
        let o = a0 + 12 * a;
        let action = cstr(d, name_pos + u32_at(d, o)? as usize)?;
        let (first, last) = (u32_at(d, o + 4)? as usize, u32_at(d, o + 8)? as usize);
        let slot = slots
            .iter()
            .find(|(_, f, l)| (*f as i64..=*l as i64).contains(&(a as i64)))
            .map(|s| s.0.clone())
            .unwrap_or_default();
        for k in first..=last.min(n_trig.saturating_sub(1)) {
            let o = t0 + 24 * k;
            let end = i32_at(d, o + 12)?;
            out.push(ActionTrigger {
                slot: slot.clone(),
                action: action.clone(),
                key: key_of(u32_at(d, o + 4)? as usize)?,
                start: i32_at(d, o + 8)?,
                end: (end != i32::MAX).then_some(end),
                flags: u16::from_le_bytes([d[o + 16], d[o + 17]]),
            });
        }
    }
    Ok(Some(out))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_other_files() {
        assert!(action_triggers(b"NOPE", "x").is_err());
    }

    /// One user "U": one call "Dust", one slot with one action "run" triggering it.
    #[test]
    fn reads_action_triggers() {
        let mut d = vec![0u8; 0x48];
        d[0..4].copy_from_slice(b"XLNK");
        d[8] = 0x1C;
        let put = |d: &mut Vec<u8>, v: u32| d.extend_from_slice(&v.to_le_bytes());
        put(&mut d, crc32fast::hash(b"U"));
        let user_pos = d.len();
        put(&mut d, 0);
        // param define table: no user params
        for v in [20u32, 0, 0, 0, 0] {
            put(&mut d, v);
        }
        let u = d.len();
        let mut hdr = [0u32; 12];
        hdr[2] = 1;
        hdr[5] = 1;
        hdr[6] = 1;
        hdr[7] = 1;
        hdr[11] = 48 + 4 + 32; // after the sort table (2 bytes, padded) and one call
        for v in hdr {
            put(&mut d, v);
        }
        d.extend_from_slice(&[0, 0, 0, 0]); // sort table, padded
        let mut call = [0u8; 32];
        call[0..4].copy_from_slice(&0u32.to_le_bytes()); // key "Dust" at name 0
        d.extend_from_slice(&call);
        // slot "S" (name 5), actions 0..0; action "run" (name 7), triggers 0..0
        put(&mut d, 5);
        d.extend_from_slice(&0i16.to_le_bytes());
        d.extend_from_slice(&0i16.to_le_bytes());
        put(&mut d, 7);
        put(&mut d, 0);
        put(&mut d, 0);
        for v in [1u32, 0, 2, i32::MAX as u32, 4, u32::MAX] {
            put(&mut d, v);
        }
        let names = d.len();
        d.extend_from_slice(b"Dust\0S\0run\0");
        d[user_pos..user_pos + 4].copy_from_slice(&(u as u32).to_le_bytes());
        d[0x3C..0x40].copy_from_slice(&1u32.to_le_bytes());
        d[0x44..0x48].copy_from_slice(&(names as u32).to_le_bytes());
        let t = action_triggers(&d, "U").unwrap().unwrap();
        assert_eq!(
            t,
            vec![ActionTrigger {
                slot: "S".into(),
                action: "run".into(),
                key: "Dust".into(),
                start: 2,
                end: None,
                flags: 4,
            }]
        );
        assert!(action_triggers(&d, "V").unwrap().is_none());
    }
}
