// SPDX-License-Identifier: AGPL-3.0-or-later

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};
use notify::{Config, EventKind, RecommendedWatcher, RecursiveMode, Watcher};

use crate::contracts::{ThemeDirectoryCssFile, ThemeFolderChange, ThemeFolderStatus};

const MAX_CSS_FILE_SIZE: usize = 1_000_000; // 1 MB 제한
const MAX_CSS_FILES: usize = 200; // 폴더당 최대 CSS 파일 수

#[derive(Debug, Clone)]
struct CachedFileState {
    mtime: u64,
    size: u64,
    content: String,
}

pub struct ThemeWatcher {
    watched_folders: Arc<RwLock<HashSet<PathBuf>>>,
    folder_snapshots: Arc<Mutex<HashMap<PathBuf, HashMap<PathBuf, CachedFileState>>>>,
    _watcher: Option<RecommendedWatcher>,
}

impl ThemeWatcher {
    pub fn new() -> (Self, std::sync::mpsc::Receiver<ThemeFolderChange>) {
        let watched_folders = Arc::new(RwLock::new(HashSet::<PathBuf>::new()));
        let folder_snapshots = Arc::new(Mutex::new(HashMap::new()));
        let (change_tx, change_rx) = std::sync::mpsc::channel();

        let (notify_tx, notify_rx) = std::sync::mpsc::channel();
        let watcher_res = RecommendedWatcher::new(
            move |res| {
                let _ = notify_tx.send(res);
            },
            Config::default(),
        );

        let watcher = watcher_res.ok();

        // 150ms 디바운스 이벤트 처리 스레드
        let folders_clone = Arc::clone(&watched_folders);
        let snapshots_clone = Arc::clone(&folder_snapshots);
        std::thread::spawn(move || {
            let mut pending_folders: HashSet<PathBuf> = HashSet::new();
            let mut last_event_time = Instant::now();

            loop {
                // 이벤트 수신 (최대 50ms 대기)
                match notify_rx.recv_timeout(Duration::from_millis(50)) {
                    Ok(Ok(event)) => {
                        let is_css_event = event.paths.iter().any(|p| {
                            p.extension()
                                .and_then(|ext| ext.to_str())
                                .map(|ext| ext.eq_ignore_ascii_case("css"))
                                .unwrap_or(false)
                        });

                        if is_css_event || matches!(event.kind, EventKind::Remove(_)) {
                            let folders = folders_clone.read().unwrap();
                            for path in &event.paths {
                                for folder in folders.iter() {
                                    if path.starts_with(folder) {
                                        pending_folders.insert(folder.clone());
                                    }
                                }
                            }
                            last_event_time = Instant::now();
                        }
                    }
                    Ok(Err(_)) => {}
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        // 150ms 디바운스 경과 확인
                        if !pending_folders.is_empty() && last_event_time.elapsed() >= Duration::from_millis(150) {
                            let batch: Vec<PathBuf> = pending_folders.drain().collect();
                            for folder in batch {
                                if let Some(change) = Self::reconcile_folder(&folder, &snapshots_clone) {
                                    let _ = change_tx.send(change);
                                }
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });

        (
            Self {
                watched_folders,
                folder_snapshots,
                _watcher: watcher,
            },
            change_rx,
        )
    }

    pub fn watch_folder(&mut self, folder: PathBuf) -> Option<ThemeFolderChange> {
        let is_dir = folder.is_dir();
        if !is_dir {
            return Some(ThemeFolderChange {
                folder: folder.to_string_lossy().to_string(),
                status: ThemeFolderStatus::Missing,
                truncated: false,
                snapshot: true,
                added: Vec::new(),
                changed: Vec::new(),
                removed: Vec::new(),
                renamed: Vec::new(),
            });
        }

        if let Some(w) = self._watcher.as_mut() {
            let _ = w.watch(&folder, RecursiveMode::Recursive);
        }

        self.watched_folders.write().unwrap().insert(folder.clone());

        // 초기 스냅샷 생성
        Self::reconcile_folder(&folder, &self.folder_snapshots)
    }

    pub fn unwatch_folder(&mut self, folder: &Path) {
        if let Some(w) = self._watcher.as_mut() {
            let _ = w.unwatch(folder);
        }
        self.watched_folders.write().unwrap().remove(folder);
        self.folder_snapshots.lock().unwrap().remove(folder);
    }

    pub fn scan_css_files(dir: &Path) -> Vec<ThemeDirectoryCssFile> {
        let mut results = Vec::new();
        if !dir.is_dir() {
            return results;
        }

        Self::visit_dir(dir, &mut results);
        results
    }

    fn visit_dir(current: &Path, results: &mut Vec<ThemeDirectoryCssFile>) {
        if results.len() >= MAX_CSS_FILES {
            return;
        }

        let Ok(read_dir) = fs::read_dir(current) else {
            return;
        };

        for entry in read_dir.flatten() {
            if results.len() >= MAX_CSS_FILES {
                break;
            }

            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().to_string();

            if file_name.starts_with('.') || file_name == "node_modules" {
                continue;
            }

            if path.is_dir() {
                Self::visit_dir(&path, results);
            } else if path.is_file()
                && path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("css")).unwrap_or(false)
                && let Ok(meta) = fs::metadata(&path)
                && meta.len() as usize <= MAX_CSS_FILE_SIZE
                && let Ok(css) = fs::read_to_string(&path)
            {
                results.push(ThemeDirectoryCssFile {
                    file_name,
                    path: path.to_string_lossy().to_string(),
                    css,
                });
            }
        }
    }

    fn reconcile_folder(
        folder: &Path,
        snapshots: &Mutex<HashMap<PathBuf, HashMap<PathBuf, CachedFileState>>>,
    ) -> Option<ThemeFolderChange> {
        if !folder.exists() {
            snapshots.lock().unwrap().remove(folder);
            return Some(ThemeFolderChange {
                folder: folder.to_string_lossy().to_string(),
                status: ThemeFolderStatus::Missing,
                truncated: false,
                snapshot: false,
                added: Vec::new(),
                changed: Vec::new(),
                removed: Vec::new(),
                renamed: Vec::new(),
            });
        }

        let css_files = Self::scan_css_files(folder);
        let mut current_map: HashMap<PathBuf, CachedFileState> = HashMap::new();

        for file in &css_files {
            let p = PathBuf::from(&file.path);
            let meta = fs::metadata(&p).ok();
            let mtime = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let size = meta.map(|m| m.len()).unwrap_or(0);

            current_map.insert(
                p,
                CachedFileState {
                    mtime,
                    size,
                    content: file.css.clone(),
                },
            );
        }

        let mut snapshots_guard = snapshots.lock().unwrap();
        let prev_map = snapshots_guard.entry(folder.to_path_buf()).or_default();

        let mut added = Vec::new();
        let mut changed = Vec::new();
        let mut removed = Vec::new();

        for (path, curr) in &current_map {
            let file_name = path
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_default();

            match prev_map.get(path) {
                None => {
                    added.push(ThemeDirectoryCssFile {
                        file_name,
                        path: path.to_string_lossy().to_string(),
                        css: curr.content.clone(),
                    });
                }
                Some(prev) => {
                    if prev.size != curr.size || prev.mtime != curr.mtime || prev.content != curr.content {
                        changed.push(ThemeDirectoryCssFile {
                            file_name,
                            path: path.to_string_lossy().to_string(),
                            css: curr.content.clone(),
                        });
                    }
                }
            }
        }

        for path in prev_map.keys() {
            if !current_map.contains_key(path) {
                removed.push(path.to_string_lossy().to_string());
            }
        }

        *prev_map = current_map;

        Some(ThemeFolderChange {
            folder: folder.to_string_lossy().to_string(),
            status: ThemeFolderStatus::Live,
            truncated: css_files.len() >= MAX_CSS_FILES,
            snapshot: false,
            added,
            changed,
            removed,
            renamed: Vec::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_theme_watcher_scan_empty_dir() {
        let temp = std::env::temp_dir().join("fluxer_theme_test");
        let _ = fs::create_dir_all(&temp);
        let files = ThemeWatcher::scan_css_files(&temp);
        assert!(files.is_empty());
        let _ = fs::remove_dir_all(&temp);
    }

    #[test]
    fn test_theme_watcher_scan_with_css() {
        let temp = std::env::temp_dir().join("fluxer_theme_test_css");
        let _ = fs::create_dir_all(&temp);
        let css_file = temp.join("custom.css");
        fs::write(&css_file, "body { background: #000; }").unwrap();

        let files = ThemeWatcher::scan_css_files(&temp);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].file_name, "custom.css");
        assert!(files[0].css.contains("background: #000;"));

        let _ = fs::remove_dir_all(&temp);
    }
}
