//! Read an asset cache (see crate docs for the layout).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::mtex::{Mtex, MtexError};

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

#[derive(Clone, Debug)]
pub struct TextureEntry {
    pub model: String,
    pub name: String,
    pub texture: Mtex,
}

/// A loaded cache. Textures are kept in a stable order (sorted by model, then name) so indices
/// can be shared with the renderer.
#[derive(Debug, Default)]
pub struct Cache {
    pub root: PathBuf,
    pub manifest: Value,
    pub textures: Vec<TextureEntry>,
    by_name: HashMap<(String, String), usize>,
}

fn read(path: &Path) -> Result<Vec<u8>, CacheError> {
    std::fs::read(path).map_err(|source| CacheError::Io {
        path: path.to_path_buf(),
        source,
    })
}

fn read_json(path: &Path) -> Result<Value, CacheError> {
    serde_json::from_slice(&read(path)?).map_err(|source| CacheError::Json {
        path: path.to_path_buf(),
        source,
    })
}

impl Cache {
    pub fn open(root: impl AsRef<Path>) -> Result<Self, CacheError> {
        let root = root.as_ref().to_path_buf();
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
                    let texture = Mtex::parse(&read(&path)?)
                        .map_err(|source| CacheError::Mtex { path, source })?;
                    textures.push(TextureEntry {
                        model: model.clone(),
                        name: name.clone(),
                        texture,
                    });
                }
            }
        }
        let by_name = textures
            .iter()
            .enumerate()
            .map(|(i, t)| ((t.model.clone(), t.name.clone()), i))
            .collect();
        Ok(Self {
            root,
            manifest,
            textures,
            by_name,
        })
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
