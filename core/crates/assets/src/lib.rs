//! Engine runtime asset cache format.
//!
//! Layout of a cache (`<root>/<build>/`), written by `mmk-import` from the user's own dump or by
//! the placeholder-pack builder from original art:
//!
//! ```text
//! manifest.json                importer version, build, stages, contents
//! keys/course_table.bin        private: SMM2 course key table (dump imports only)
//! tables/<name>.json           game data tables (EnemyDB, EditDB, SceneDB, ...)
//! textures/index.json          { model: { texture: {w, h, format, variant} } }
//! textures/<model>/<tex>.mtex  one texture (see `Mtex`)
//! ```

pub mod mtex;

pub use mtex::{Mtex, MtexFormat, MtexVariant};
