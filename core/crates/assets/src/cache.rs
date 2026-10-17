//! Read an asset cache (see crate docs for the layout).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use serde_json::Value;

use crate::mtex::{Mtex, MtexError};
use crate::sprites::{Anim, Quad, Sprites};

#[derive(Debug, thiserror::Error)]
pub enum CacheError {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("{path}: {source}")]
    Json {
        path: PathBuf,
        source: serde_json::Error,
    },
    #[error("{path}: {source}")]
    Mtex { path: PathBuf, source: MtexError },
}

/// A texture of the cache: name and size up front, pixels on first use (see [`Cache::texture`]).
#[derive(Debug)]
pub struct TextureEntry {
    pub model: String,
    pub name: String,
    pub width: u32,
    pub height: u32,
    path: PathBuf,
    data: OnceLock<Option<Mtex>>,
}

/// A loaded cache. Textures are kept in a stable order (sorted by model, then name) so indices
/// can be shared with the renderer.
#[derive(Debug, Default)]
pub struct Cache {
    pub root: PathBuf,
    /// Read texture pixels from `root` when first asked for (`open`); else they were all loaded.
    lazy: bool,
    pub manifest: Value,
    pub textures: Vec<TextureEntry>,
    by_name: HashMap<(String, String), usize>,
    sprites: Sprites,
}

/// Reads one file of a cache, by its full path (`root.join(relative path)`).
pub type ReadFn<'a> = &'a dyn Fn(&Path) -> std::io::Result<Vec<u8>>;

fn read_with(read: ReadFn, path: &Path) -> Result<Vec<u8>, CacheError> {
    read(path).map_err(|source| CacheError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read(path: &Path) -> Result<Vec<u8>, CacheError> {
    read_with(&|p| std::fs::read(p), path)
}

fn parse_json(path: &Path, bytes: &[u8]) -> Result<Value, CacheError> {
    serde_json::from_slice(bytes).map_err(|source| CacheError::Json {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json(path: &Path) -> Result<Value, CacheError> {
    parse_json(path, &read(path)?)
}

impl Cache {
    /// Open a cache directory. Texture pixels are read when first used ([`Cache::texture`]):
    /// a cache of every style holds thousands of textures and a course draws a few hundred.
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CacheError> {
        Self::build(root, &|p| std::fs::read(p), true)
    }

    /// Open a cache through `read` instead of the file system (e.g. files the user picked in a
    /// browser). `root` is only joined with relative paths and passed back to `read`.
    pub fn load(root: impl AsRef<Path>, read: ReadFn) -> Result<Self, CacheError> {
        Self::build(root, read, false)
    }

    fn build(root: impl AsRef<Path>, read: ReadFn, lazy: bool) -> Result<Self, CacheError> {
        let root = root.as_ref().to_path_buf();
        let read_json = |path: &Path| parse_json(path, &read_with(read, path)?);
        let manifest = read_json(&root.join("manifest.json"))?;
        let index = read_json(&root.join("textures/index.json"))?;
        let mut textures = Vec::new();
        if let Value::Object(models) = &index {
            let mut names: Vec<(&String, &Value)> = models.iter().collect();
            names.sort_by(|a, b| a.0.cmp(b.0));
            for (model, texs) in names {
                let mut tex_names: Vec<&String> = texs
                    .as_object()
                    .map(|m| m.keys().collect())
                    .unwrap_or_default();
                tex_names.sort();
                for name in tex_names {
                    let path = root.join(format!("textures/{model}/{name}.mtex"));
                    let info = &texs[name.as_str()];
                    let dim = |k: &str| info[k].as_u64().unwrap_or(0) as u32;
                    let data = OnceLock::new();
                    let (mut width, mut height) = (dim("w"), dim("h"));
                    if !lazy || width == 0 {
                        let texture = Mtex::parse(&read_with(read, &path)?).map_err(|source| {
                            CacheError::Mtex {
                                path: path.clone(),
                                source,
                            }
                        })?;
                        (width, height) = (texture.width, texture.height);
                        if !lazy {
                            let _ = data.set(Some(texture));
                        }
                    }
                    textures.push(TextureEntry {
                        model: model.clone(),
                        name: name.clone(),
                        width,
                        height,
                        path,
                        data,
                    });
                }
            }
        }
        let by_name = textures
            .iter()
            .enumerate()
            .map(|(i, t)| ((t.model.clone(), t.name.clone()), i))
            .collect();
        // sprite placement is optional (caches from importer version 1 have none)
        let sprites_path = root.join("sprites.json");
        let sprites = match read(&sprites_path) {
            Ok(bytes) => Sprites::from_json(&parse_json(&sprites_path, &bytes)?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Sprites::default(),
            Err(source) => {
                return Err(CacheError::Io {
                    path: sprites_path,
                    source,
                });
            }
        };
        Ok(Self {
            root,
            lazy,
            manifest,
            textures,
            by_name,
            sprites,
        })
    }

    /// Where `texture` of `model` is drawn relative to the model origin (from the BFRES quad).
    pub fn sprite_quad(&self, model: &str, texture: &str) -> Option<Quad> {
        self.sprites
            .quads
            .get(&(model.to_string(), texture.to_string()))
            .copied()
    }

    /// Texture-pattern animation of `model`, by BFRES name (`walk_ftp`) or short name (`walk`).
    pub fn anim(&self, model: &str, name: &str) -> Option<&Anim> {
        let m = model.to_string();
        self.sprites
            .anims
            .get(&(m.clone(), name.to_string()))
            .or_else(|| self.sprites.anims.get(&(m, format!("{name}_ftp"))))
    }

    /// The pixels of texture `i`, read on first use for a lazily opened cache (None if the file
    /// is missing or unreadable).
    pub fn texture(&self, i: usize) -> Option<&Mtex> {
        let e = self.textures.get(i)?;
        e.data
            .get_or_init(|| {
                if !self.lazy {
                    return None;
                }
                std::fs::read(&e.path)
                    .ok()
                    .and_then(|b| Mtex::parse(&b).ok())
            })
            .as_ref()
    }

    /// Index of a texture by model and texture name.
    pub fn texture_index(&self, model: &str, name: &str) -> Option<usize> {
        self.by_name
            .get(&(model.to_string(), name.to_string()))
            .copied()
    }

    pub fn table(&self, name: &str) -> Result<Value, CacheError> {
        read_json(&self.root.join(format!("tables/{name}.json")))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A cache with no textures and one sprite entry; a cache without sprites.json still opens.
    #[test]
    fn opens_with_and_without_sprites() {
        let dir = std::env::temp_dir().join(format!("mmk-cache-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("textures")).unwrap();
        std::fs::write(dir.join("manifest.json"), "{}").unwrap();
        std::fs::write(dir.join("textures/index.json"), "{}").unwrap();
        let c = Cache::open(&dir).unwrap();
        assert_eq!(c.sprite_quad("M", "walk.0"), None);
        std::fs::write(
            dir.join("sprites.json"),
            r#"{"version":1,"models":{"M":{"quads":{"walk.0":{"rect":[-8,-16,8,16],"uv":[0,0,1,1]}},
               "anims":{"walk_ftp":{"frames":20,"loop":true,"keys":[[0,"walk.0"],[10,"walk.1"]]}}}}}"#,
        )
        .unwrap();
        let c = Cache::open(&dir).unwrap();
        let q = c.sprite_quad("M", "walk.0").unwrap();
        assert_eq!((q.y0, q.y1), (-16.0, 16.0));
        assert_eq!(c.anim("M", "walk").unwrap().texture_at(10), Some("walk.1"));
        assert!(c.anim("M", "walk_ftp").is_some());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
