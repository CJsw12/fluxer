//! Emoji usage (frecency) for autocomplete ranking, kept as a small JSON
//! file next to the session store (`emoji_frecency.json`, emoji keys and
//! counts only). Without a path (tests, `--token-env`) it lives in memory.

use std::path::{Path, PathBuf};

use anyhow::{Context as _, Result};
use fluxer_client_core::search::emoji::EmojiFrecency;

pub const FILE_NAME: &str = "emoji_frecency.json";

#[derive(Debug, Default)]
pub struct FrecencyStore {
    pub data: EmojiFrecency,
    path: Option<PathBuf>,
}

impl FrecencyStore {
    /// Loads `path` (a missing or unreadable file starts empty) and keeps
    /// saving there.
    pub fn open(path: PathBuf, now_ms: u64) -> Self {
        let mut data = match read(&path) {
            Ok(data) => data,
            Err(err) => {
                if path.exists() {
                    tracing::warn!(%err, "emoji frecency not loaded");
                }
                EmojiFrecency::default()
            }
        };
        data.prune(now_ms);
        Self { data, path: Some(path) }
    }

    /// Counts a use and saves (a failed save is only logged).
    pub fn record(&mut self, key: &str, now_ms: u64) {
        self.data.record(key, now_ms);
        if let Some(path) = &self.path
            && let Err(err) = write(path, &self.data)
        {
            tracing::warn!(%err, "emoji frecency not saved");
        }
    }
}

fn read(path: &Path) -> Result<EmojiFrecency> {
    let bytes = std::fs::read(path).with_context(|| format!("reading {}", path.display()))?;
    Ok(serde_json::from_slice(&bytes)?)
}

/// Write to a temporary file, then rename over the old one.
fn write(path: &Path, data: &EmojiFrecency) -> Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec(data)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_and_loads() {
        let dir = std::env::temp_dir().join(format!("fluxer-frecency-{}", std::process::id()));
        let path = dir.join(FILE_NAME);
        let _ = std::fs::remove_file(&path);
        let mut store = FrecencyStore::open(path.clone(), 1_000);
        assert!(store.data.entries.is_empty(), "missing file starts empty");
        store.record("custom:9", 2_000);
        store.record("custom:9", 3_000);
        let loaded = FrecencyStore::open(path.clone(), 4_000);
        assert_eq!(loaded.data.entries.get("custom:9").map(|e| e.count), Some(2));
        std::fs::write(&path, b"not json").expect("overwrite");
        assert!(FrecencyStore::open(path, 5_000).data.entries.is_empty(), "bad file starts empty");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
