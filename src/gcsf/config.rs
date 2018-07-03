use std::path::{Path, PathBuf};
use std::time::Duration;

/// Provides a few properties of the file system that can be configured. Includes sensible
/// defaults for the absent values.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Config {
    pub debug: Option<bool>,
    pub mount_check: Option<bool>,
    pub cache_max_seconds: Option<u64>,
    pub cache_max_items: Option<u64>,
    pub cache_statfs_seconds: Option<u64>,
    pub sync_interval: Option<u64>,
    pub mount_options: Option<Vec<String>>,
    pub config_dir: Option<PathBuf>,
    pub session_name: Option<String>,
    pub authorize_using_code: Option<bool>,
}

impl Config {
    /// Whether to show additional logging info.
    pub fn debug(&self) -> bool {
        self.debug.unwrap_or(false)
    }

    /// Whether to perform a mount check before creating the file system and fail early if it fails.
    pub fn mount_check(&self) -> bool {
        self.mount_check.unwrap_or(true)
    }

    /// How long to cache the contents of a file after it has been accessed.
    pub fn cache_max_seconds(&self) -> Duration {
        Duration::from_secs(self.cache_max_seconds.unwrap_or(10))
    }

    /// How how many files to cache.
    pub fn cache_max_items(&self) -> u64 {
        self.cache_max_items.unwrap_or(10)
    }

    /// How long to cache the size and capacity of the filesystem. These are the values reported by `df`.
    pub fn cache_statfs_seconds(&self) -> Duration {
        Duration::from_secs(self.cache_statfs_seconds.unwrap_or(100))
    }

    /// How many seconds to wait before checking for remote changes and updating them locally.
    pub fn sync_interval(&self) -> Duration {
        Duration::from_secs(self.sync_interval.unwrap_or(10))
    }

    pub fn mount_options(&self) -> Vec<String> {
        match self.mount_options {
            Some(ref options) => options.clone(),
            None => Vec::new(),
        }
    }

    pub fn session_name(&self) -> &String {
        self.session_name.as_ref().unwrap()
    }

    /// The path to the token file which authorizes access to a Drive account.
    pub fn token_file(&self) -> PathBuf {
        Path::new(self.config_dir.as_ref().unwrap()).join(Path::new(self.session_name()))
    }

    pub fn config_dir(&self) -> &PathBuf {
        self.config_dir.as_ref().unwrap()
    }

    /// If set to true, Google Drive will provide a code after logging in and
    /// authorizing GCSF. This code must be copied and pasted into GCSF in order to
    /// complete the process. Useful for running GCSF on a remote (headless) server.
    ///
    /// If set to false, Google Drive will attempt to communicate with GCSF directly.
    /// This is usually faster and more convenient.
    pub fn authorize_using_code(&self) -> bool {
        self.authorize_using_code.unwrap_or(true)
    }
}
