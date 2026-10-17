//! Super Mario Maker 1 and 2 course formats.
//!
//! Clean-room implementation from community format documentation (see `docs/formats/`), kept in
//! parity with the Python reference package `smmfmt`. Contains no Nintendo data or key material:
//! the SMM2 course key table is supplied by the caller from the user's own game executable.

pub mod aes_modes;
pub mod course_id;
pub mod error;
pub mod record;
pub mod smm1;
pub mod smm2;

pub use error::{Error, Result};
