//! Fixed-layout binary records that round-trip byte-exactly.
//!
//! A record keeps the raw bytes it was parsed from; writing starts from those bytes and packs the
//! known fields over them, so unknown, reserved and garbage bytes survive a parse/write cycle.
//! This mirrors `smmfmt/binstruct.py`, which is the reference implementation.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Map, Value};

use crate::error::{Error, Result};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Endian {
    Little,
    Big,
}

/// A value that can be read from / written to a fixed offset.
pub trait Field: Sized + Clone {
    fn read(buf: &[u8], off: usize, e: Endian) -> Self;
    fn write(&self, buf: &mut [u8], off: usize, e: Endian, name: &str) -> Result<()>;
    fn to_json(&self) -> Value;
}

macro_rules! int_field {
    ($($t:ty),*) => {$(
        impl Field for $t {
            fn read(buf: &[u8], off: usize, e: Endian) -> Self {
                let mut b = [0u8; size_of::<$t>()];
                b.copy_from_slice(&buf[off..off + size_of::<$t>()]);
                match e { Endian::Little => <$t>::from_le_bytes(b), Endian::Big => <$t>::from_be_bytes(b) }
            }
            fn write(&self, buf: &mut [u8], off: usize, e: Endian, _name: &str) -> Result<()> {
                let b = match e { Endian::Little => self.to_le_bytes(), Endian::Big => self.to_be_bytes() };
                buf[off..off + b.len()].copy_from_slice(&b);
                Ok(())
            }
            fn to_json(&self) -> Value { Value::from(*self) }
        }
    )*};
}
int_field!(u8, i8, u16, i16, u32, i32, u64);

/// Fixed-capacity UTF-16 string of `N` code units including the terminator. Stored as raw code
/// units so unpaired surrogates survive a round trip.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Utf16<const N: usize>(pub Vec<u16>);

impl<const N: usize> Utf16<N> {
    pub fn to_string_lossy(&self) -> String {
        String::from_utf16_lossy(&self.0)
    }
    pub fn set(&mut self, s: &str) {
        self.0 = s.encode_utf16().collect();
    }
}

impl<const N: usize> Field for Utf16<N> {
    fn read(buf: &[u8], off: usize, e: Endian) -> Self {
        let mut units = Vec::new();
        for i in 0..N {
            let u = u16::read(buf, off + i * 2, e);
            if u == 0 {
                break;
            }
            units.push(u);
        }
        Utf16(units)
    }
    fn write(&self, buf: &mut [u8], off: usize, e: Endian, name: &str) -> Result<()> {
        if self.0.len() > N {
            return Err(Error::StringTooLong {
                field: name.to_string(),
                len: self.0.len(),
                capacity: N,
            });
        }
        for (i, u) in self.0.iter().enumerate() {
            u.write(buf, off + i * 2, e, name)?;
        }
        if self.0.len() < N {
            0u16.write(buf, off + self.0.len() * 2, e, name)?; // terminator; later bytes untouched
        }
        Ok(())
    }
    fn to_json(&self) -> Value {
        Value::String(self.to_string_lossy())
    }
}

/// Fixed-capacity single-byte (Latin-1) string of `N` bytes including the terminator.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Ascii<const N: usize>(pub Vec<u8>);

impl<const N: usize> Ascii<N> {
    pub fn as_str_lossy(&self) -> String {
        self.0.iter().map(|&b| b as char).collect()
    }
    pub fn set(&mut self, s: &str) {
        self.0 = s.chars().map(|c| c as u8).collect();
    }
}

impl<const N: usize> Field for Ascii<N> {
    fn read(buf: &[u8], off: usize, _e: Endian) -> Self {
        let raw = &buf[off..off + N];
        let end = raw.iter().position(|&b| b == 0).unwrap_or(N);
        Ascii(raw[..end].to_vec())
    }
    fn write(&self, buf: &mut [u8], off: usize, _e: Endian, name: &str) -> Result<()> {
        if self.0.len() > N {
            return Err(Error::StringTooLong {
                field: name.to_string(),
                len: self.0.len(),
                capacity: N,
            });
        }
        buf[off..off + self.0.len()].copy_from_slice(&self.0);
        if self.0.len() < N {
            buf[off + self.0.len()] = 0;
        }
        Ok(())
    }
    fn to_json(&self) -> Value {
        Value::String(self.as_str_lossy())
    }
}

/// A fixed-size record.
pub trait Record: Sized + Clone {
    const SIZE: usize;
    /// Parse from exactly `SIZE` bytes.
    fn parse(raw: &[u8]) -> Self;
    fn to_bytes(&self) -> Result<Vec<u8>>;
    /// Known fields as a JSON object, in declaration order (same names as the Python reference).
    fn fields_json(&self) -> Map<String, Value>;

    fn from_slice(buf: &[u8], off: usize) -> Self {
        Self::parse(&buf[off..off + Self::SIZE])
    }
    fn blank() -> Self {
        Self::parse(&vec![0u8; Self::SIZE])
    }
}

/// Declare a record type: `record! { pub struct Name, SIZE, Endian::Little, { field: Type = 0x00, ... } }`.
#[macro_export]
macro_rules! record {
    ($(#[$m:meta])* pub struct $name:ident, $size:expr, $endian:expr, { $($field:ident : $ty:ty = $off:expr),* $(,)? }) => {
        $(#[$m])*
        #[derive(Clone, Debug)]
        pub struct $name {
            raw: Vec<u8>,
            $(pub $field: $ty,)*
        }

        impl $crate::record::Record for $name {
            const SIZE: usize = $size;
            fn parse(raw: &[u8]) -> Self {
                assert_eq!(raw.len(), $size, concat!(stringify!($name), " size"));
                Self {
                    raw: raw.to_vec(),
                    $($field: <$ty as $crate::record::Field>::read(raw, $off, $endian),)*
                }
            }
            fn to_bytes(&self) -> $crate::error::Result<Vec<u8>> {
                let mut buf = self.raw.clone();
                $(<$ty as $crate::record::Field>::write(&self.$field, &mut buf, $off, $endian, stringify!($field))?;)*
                Ok(buf)
            }
            fn fields_json(&self) -> serde_json::Map<String, serde_json::Value> {
                let mut m = serde_json::Map::new();
                $(m.insert(stringify!($field).trim_start_matches("r#").to_string(), <$ty as $crate::record::Field>::to_json(&self.$field));)*
                m
            }
        }

        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                use $crate::record::Record;
                self.to_bytes().ok() == other.to_bytes().ok()
            }
        }
    };
}

/// Fixed-capacity array whose live length comes from a separate count. Slots past the live
/// length keep their raw bytes; slots freed by shrinking the list are zeroed.
#[derive(Clone, Debug)]
pub struct RecordArray<T: Record> {
    raw: Vec<u8>,
    capacity: usize,
    parsed: usize,
    pub items: Vec<T>,
}

impl<T: Record> RecordArray<T> {
    pub fn parse(raw: &[u8], capacity: usize, count: i64) -> Self {
        assert_eq!(raw.len(), T::SIZE * capacity);
        let parsed = count.clamp(0, capacity as i64) as usize;
        let items = (0..parsed)
            .map(|i| T::from_slice(raw, i * T::SIZE))
            .collect();
        Self {
            raw: raw.to_vec(),
            capacity,
            parsed,
            items,
        }
    }

    pub fn changed_length(&self) -> bool {
        self.items.len() != self.parsed
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        if self.items.len() > self.capacity {
            return Err(Error::Capacity {
                what: std::any::type_name::<T>(),
                len: self.items.len(),
                capacity: self.capacity,
            });
        }
        let mut buf = self.raw.clone();
        for (i, item) in self.items.iter().enumerate() {
            buf[i * T::SIZE..(i + 1) * T::SIZE].copy_from_slice(&item.to_bytes()?);
        }
        for i in self.items.len()..self.parsed {
            buf[i * T::SIZE..(i + 1) * T::SIZE].fill(0);
        }
        Ok(buf)
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub fn iter(&self) -> std::slice::Iter<'_, T> {
        self.items.iter()
    }
}

/// Fixed-capacity array addressed by slot; empty slots are all-zero and live entries may be sparse.
#[derive(Clone, Debug)]
pub struct SlotArray<T: Record> {
    raw: Vec<u8>,
    capacity: usize,
    original: BTreeSet<usize>,
    pub slots: BTreeMap<usize, T>,
}

impl<T: Record> SlotArray<T> {
    pub fn parse(raw: &[u8], capacity: usize) -> Self {
        assert_eq!(raw.len(), T::SIZE * capacity);
        let slots: BTreeMap<usize, T> = (0..capacity)
            .filter(|&i| raw[i * T::SIZE..(i + 1) * T::SIZE].iter().any(|&b| b != 0))
            .map(|i| (i, T::from_slice(raw, i * T::SIZE)))
            .collect();
        let original = slots.keys().copied().collect();
        Self {
            raw: raw.to_vec(),
            capacity,
            original,
            slots,
        }
    }

    pub fn changed_length(&self) -> bool {
        self.slots.keys().copied().collect::<BTreeSet<_>>() != self.original
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        let mut buf = self.raw.clone();
        for &i in self.original.iter().filter(|i| !self.slots.contains_key(i)) {
            buf[i * T::SIZE..(i + 1) * T::SIZE].fill(0);
        }
        for (&i, item) in &self.slots {
            if i >= self.capacity {
                return Err(Error::Capacity {
                    what: std::any::type_name::<T>(),
                    len: i + 1,
                    capacity: self.capacity,
                });
            }
            buf[i * T::SIZE..(i + 1) * T::SIZE].copy_from_slice(&item.to_bytes()?);
        }
        Ok(buf)
    }

    pub fn len(&self) -> usize {
        self.slots.len()
    }
    pub fn is_empty(&self) -> bool {
        self.slots.is_empty()
    }
}
