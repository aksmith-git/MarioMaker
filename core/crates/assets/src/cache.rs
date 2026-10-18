//! Read an asset cache (see crate docs for the layout).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, OnceLock};

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
    /// Deferred caches: the pixels were asked for (see [`Cache::take_requests`]).
    requested: AtomicBool,
}

/// Where a cache's texture pixels come from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Pixels {
    /// All read up front ([`Cache::load`]).
    #[default]
    Loaded,
    /// Read from `root` on the file system when first used ([`Cache::open`]).
    Disk,
    /// Handed over later by the host ([`Cache::load_deferred`]): a browser reads its storage
    /// asynchronously, so a first use only records the request.
    Deferred,
}

/// A loaded cache. Textures are kept in a stable order (sorted by model, then name) so indices
/// can be shared with the renderer.
#[derive(Debug, Default)]
pub struct Cache {
    pub root: PathBuf,
    pixels: Pixels,
    /// Deferred caches: textures asked for and not yet handed to the host.
    wanted: Mutex<Vec<usize>>,
    pub manifest: Value,
    pub textures: Vec<TextureEntry>,
    by_name: HashMap<(String, String), usize>,
    sprites: Sprites,
    /// Manifests of the caches added with [`Cache::merge`], in order.
    pub merged: Vec<Value>,
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
        Self::build(root, &|p| std::fs::read(p), Pixels::Disk)
    }

    /// Open a cache through `read` instead of the file system (tests, small caches). `root` is
    /// only joined with relative paths and passed back to `read`. Every texture is read now.
    pub fn load(root: impl AsRef<Path>, read: ReadFn) -> Result<Self, CacheError> {
        Self::build(root, read, Pixels::Loaded)
    }

    /// Like [`Cache::load`], but no texture pixels are read: [`Cache::texture`] returns None and
    /// records the request until the host has read the file (asynchronously, e.g. from a
    /// browser's storage) and handed it over with [`Cache::provide`]. The manifest, texture
    /// index and sprites are read through `read` now.
    pub fn load_deferred(root: impl AsRef<Path>, read: ReadFn) -> Result<Self, CacheError> {
        Self::build(root, read, Pixels::Deferred)
    }

    fn build(root: impl AsRef<Path>, read: ReadFn, pixels: Pixels) -> Result<Self, CacheError> {
        let lazy = pixels != Pixels::Loaded;
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
                    // (a deferred cache can't read now: a texture without a size stays 0 × 0)
                    if !lazy || (width == 0 && pixels == Pixels::Disk) {
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
                        requested: AtomicBool::new(false),
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
            pixels,
            wanted: Mutex::new(Vec::new()),
            manifest,
            textures,
            by_name,
            sprites,
            merged: Vec::new(),
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
    /// is missing or unreadable). A deferred cache returns None until the host has provided
    /// them (see [`Cache::texture_pending`]).
    pub fn texture(&self, i: usize) -> Option<&Mtex> {
        let e = self.textures.get(i)?;
        if self.pixels == Pixels::Deferred {
            if let Some(t) = e.data.get() {
                return t.as_ref();
            }
            if !e.requested.swap(true, Ordering::Relaxed)
                && let Ok(mut w) = self.wanted.lock()
            {
                w.push(i);
            }
            return None;
        }
        e.data
            .get_or_init(|| {
                if self.pixels != Pixels::Disk {
                    return None;
                }
                std::fs::read(&e.path)
                    .ok()
                    .and_then(|b| Mtex::parse(&b).ok())
            })
            .as_ref()
    }

    /// Deferred caches: texture `i`'s pixels haven't been provided yet (asked for or not).
    pub fn texture_pending(&self, i: usize) -> bool {
        self.pixels == Pixels::Deferred
            && self.textures.get(i).is_some_and(|e| e.data.get().is_none())
    }

    /// Deferred caches: the textures asked for since the last call, with their file paths
    /// relative to the cache root (`textures/<model>/<name>.mtex`).
    pub fn take_requests(&self) -> Vec<(usize, String)> {
        let wanted = match self.wanted.lock() {
            Ok(mut w) => std::mem::take(&mut *w),
            Err(_) => return Vec::new(),
        };
        wanted
            .into_iter()
            .map(|i| {
                let e = &self.textures[i];
                (i, format!("textures/{}/{}.mtex", e.model, e.name))
            })
            .collect()
    }

    /// Deferred caches: hand over the file of texture `i` (None: it couldn't be read, so the
    /// texture stays missing). False if it was provided before or doesn't parse.
    pub fn provide(&self, i: usize, bytes: Option<&[u8]>) -> bool {
        let Some(e) = self.textures.get(i) else {
            return false;
        };
        let t = bytes.and_then(|b| Mtex::parse(b).ok());
        let parsed = t.is_some();
        e.data.set(t).is_ok() && parsed
    }

    /// Add another cache's textures and sprite placement after this cache's own (e.g. the SMM1
    /// costume art next to the SMM2 cache). Existing (model, texture) names win; the indices of
    /// this cache's textures don't change, the added ones follow them. Do this right after
    /// opening, before texture indices are handed out. The other cache's manifest is kept in
    /// [`Cache::merged`]. Returns the number of textures added.
    pub fn merge(&mut self, other: Cache) -> usize {
        let mut added = 0;
        for t in other.textures {
            let key = (t.model.clone(), t.name.clone());
            if self.by_name.contains_key(&key) {
                continue;
            }
            if self.pixels == Pixels::Loaded && t.data.get().is_none() {
                // this cache never reads lazily: read the added texture now
                let tex = std::fs::read(&t.path)
                    .ok()
                    .and_then(|b| Mtex::parse(&b).ok());
                let _ = t.data.set(tex);
            }
            self.by_name.insert(key, self.textures.len());
            self.textures.push(t);
            added += 1;
        }
        for (k, q) in other.sprites.quads {
            self.sprites.quads.entry(k).or_insert(q);
        }
        for (k, a) in other.sprites.anims {
            self.sprites.anims.entry(k).or_insert(a);
        }
        self.merged.push(other.manifest);
        added
    }

    /// The SMM1 costumes with art, as (costume id, display name), from a merged SMM1 sprite
    /// cache (manifest kind `smm1-sprites`); empty without one.
    pub fn costumes(&self) -> Vec<(u16, String)> {
        self.merged
            .iter()
            .chain(std::iter::once(&self.manifest))
            .filter(|m| m["kind"] == "smm1-sprites")
            .flat_map(|m| m["costumes"].as_array().cloned().unwrap_or_default())
            .filter(|c| c["art"].as_bool().unwrap_or(true))
            .filter_map(|c| {
                let name = c["name"].as_str().or(c["res"].as_str()).unwrap_or_default();
                Some((u16::try_from(c["id"].as_u64()?).ok()?, name.to_string()))
            })
            .collect()
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

    /// Merging appends the other cache's textures and quads after this cache's own.
    #[test]
    fn merge_appends_textures_and_quads() {
        let dir = std::env::temp_dir().join(format!("mmk-cache-merge-{}", std::process::id()));
        for (sub, model) in [("a", "M"), ("b", "C")] {
            let d = dir.join(sub);
            std::fs::create_dir_all(d.join(format!("textures/{model}"))).unwrap();
            std::fs::write(d.join("manifest.json"), format!(r#"{{"kind":"{sub}"}}"#)).unwrap();
            std::fs::write(
                d.join("textures/index.json"),
                format!(r#"{{"{model}":{{"wait.0":{{"w":1,"h":1}}}}}}"#),
            )
            .unwrap();
            let t = Mtex {
                format: crate::MtexFormat::Rgba8,
                variant: crate::MtexVariant::Unorm,
                width: 1,
                height: 1,
                channel_select: [2, 3, 4, 5],
                data: vec![1, 2, 3, 4],
            };
            std::fs::write(
                d.join(format!("textures/{model}/wait.0.mtex")),
                t.to_bytes(),
            )
            .unwrap();
            std::fs::write(
                d.join("sprites.json"),
                format!(
                    r#"{{"version":1,"models":{{"{model}":{{"quads":{{"wait.0":{{"rect":[-12,0,12,24],"uv":[0,0,1,1]}}}}}}}}}}"#
                ),
            )
            .unwrap();
        }
        let mut a = Cache::open(dir.join("a")).unwrap();
        let b = Cache::open(dir.join("b")).unwrap();
        assert_eq!(a.merge(b), 1);
        assert_eq!(a.texture_index("M", "wait.0"), Some(0));
        assert_eq!(a.texture_index("C", "wait.0"), Some(1));
        assert_eq!(a.texture(1).unwrap().data, vec![1, 2, 3, 4]);
        assert_eq!(a.sprite_quad("C", "wait.0").unwrap().x0, -12.0);
        assert_eq!(a.merged[0]["kind"], "b");
        let again = Cache::open(dir.join("b")).unwrap();
        assert_eq!(a.merge(again), 0, "names already present are kept");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// A deferred cache records the first request of each texture and takes the pixels later.
    #[test]
    fn deferred_textures_are_requested_once_then_provided() {
        let files: HashMap<&str, Vec<u8>> = [
            ("/c/manifest.json", b"{}".to_vec()),
            (
                "/c/textures/index.json",
                br#"{"M":{"a":{"w":4,"h":4},"b":{"w":4,"h":4}}}"#.to_vec(),
            ),
        ]
        .into();
        let read = |p: &Path| {
            files
                .get(p.to_str().unwrap())
                .cloned()
                .ok_or_else(|| std::io::Error::from(std::io::ErrorKind::NotFound))
        };
        let c = Cache::load_deferred("/c", &read).unwrap();
        assert_eq!((c.textures[0].width, c.textures.len()), (4, 2));
        assert!(c.texture_pending(0));
        assert!(c.texture(0).is_none() && c.texture(0).is_none() && c.texture(1).is_none());
        assert_eq!(
            c.take_requests(),
            [
                (0, "textures/M/a.mtex".into()),
                (1, "textures/M/b.mtex".into())
            ]
        );
        assert!(c.take_requests().is_empty());
        let t = Mtex {
            format: crate::MtexFormat::Rgba8,
            variant: crate::MtexVariant::Srgb,
            width: 4,
            height: 4,
            channel_select: [2, 3, 4, 5],
            data: vec![7; 64],
        };
        assert!(c.provide(0, Some(&t.to_bytes())));
        assert!(!c.provide(0, Some(&t.to_bytes())), "only once");
        assert!(!c.provide(1, None));
        assert_eq!(c.texture(0).unwrap().data, t.data);
        assert!(c.texture(1).is_none() && !c.texture_pending(1));
        assert!(c.take_requests().is_empty());
    }
}
