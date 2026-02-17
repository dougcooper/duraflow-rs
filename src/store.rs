use parking_lot::Mutex;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

/// Object-safe storage backend used by `duraflow-rs`.
pub trait Storage: Send + Sync {
    /// Return the raw JSON string stored for `key`, if any.
    fn get_raw(&self, key: &str) -> Option<String>;
    /// Store a raw JSON string for `key`.
    fn save_raw(&self, key: &str, value: &str) -> std::io::Result<()>;
}

/// In-memory store used for examples and tests.
pub struct MemoryStore {
    storage: Mutex<HashMap<String, String>>,
}

impl MemoryStore {
    pub fn new() -> Self {
        Self {
            storage: Mutex::new(HashMap::new()),
        }
    }
}

impl Storage for MemoryStore {
    fn get_raw(&self, key: &str) -> Option<String> {
        self.storage.lock().get(key).cloned()
    }

    fn save_raw(&self, key: &str, value: &str) -> std::io::Result<()> {
        self.storage
            .lock()
            .insert(key.to_string(), value.to_string());
        Ok(())
    }
}

/// Simple file-backed store: stores each key as a file under a directory.
/// Keys are sanitized to filesystem-friendly names.
pub struct FileStore {
    dir: PathBuf,
    write_lock: Mutex<()>,
}

impl FileStore {
    /// Create a new `FileStore` backed by `dir`. Directory will be created if missing.
    pub fn new<P: AsRef<Path>>(dir: P) -> std::io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;
        Ok(Self {
            dir,
            write_lock: Mutex::new(()),
        })
    }

    fn key_path(&self, key: &str) -> PathBuf {
        let safe: String = key
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        self.dir.join(safe)
    }
}

impl Storage for FileStore {
    fn get_raw(&self, key: &str) -> Option<String> {
        let path = self.key_path(key);
        fs::read_to_string(path).ok()
    }

    fn save_raw(&self, key: &str, value: &str) -> std::io::Result<()> {
        let _g = self.write_lock.lock();
        let path = self.key_path(key);
        fs::write(&path, value)
    }
}
