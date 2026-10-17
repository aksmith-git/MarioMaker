use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum Error {
    #[error("{what} must be {expected:#x} bytes, got {got:#x}")]
    Size {
        what: &'static str,
        expected: usize,
        got: usize,
    },
    #[error("bad magic in {what}")]
    Magic { what: &'static str },
    #[error("{field}: {len} characters exceeds capacity {capacity}")]
    StringTooLong {
        field: String,
        len: usize,
        capacity: usize,
    },
    #[error("{what}: {len} items exceeds capacity {capacity}")]
    Capacity {
        what: &'static str,
        len: usize,
        capacity: usize,
    },
    #[error("CRC32 mismatch (wrong key table or corrupt file)")]
    Crc,
    #[error("CMAC mismatch (wrong key table or corrupt file)")]
    Cmac,
    #[error("invalid course code: {0}")]
    InvalidCode(String),
    #[error("{what}: {detail}")]
    Format { what: &'static str, detail: String },
}

pub type Result<T> = std::result::Result<T, Error>;
