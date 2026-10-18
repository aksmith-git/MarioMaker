//! Super Mario Maker 1 and 2 course formats.
//!
//! Clean-room implementation from community format documentation (see `docs/formats/`), kept in
//! parity with the Python reference package `smmfmt`. Contains no Nintendo data or key material:
//! the SMM2 course key table is supplied by the caller from the user's own game executable.

pub mod aamp;
pub mod aes_modes;
pub mod archive;
pub mod audio;
pub mod bc7;
pub mod bcn;
pub mod bfcpx;
pub mod bfevfl;
pub mod bffnt;
pub mod bflim;
pub mod bfres;
pub mod bfres_wiiu;
pub mod bntx;
pub mod byml;
pub mod course_id;
pub mod error;
pub mod fixtures;
pub mod gx2;
pub mod lyt;
pub mod msbt;
pub mod nso;
pub mod record;
pub mod rpx;
pub mod smm1;
pub mod smm2;
pub mod vfxb;
pub mod xlink;
pub mod xlink_triggers;

pub use error::{Error, Result};
