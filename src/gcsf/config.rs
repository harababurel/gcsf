use std::path::{Path, PathBuf};
use std::time::Duration;

/// Provides a few properties of the file system that can be configured. Includes sensible
/// defaults for the absent values.
#[derive(Deserialize, Clone, Debug, Default)]
pub struct Config {
    /// Show additional logging info?
    pub debug: Option<bool>,
    /// Perform a mount check and fail early if it fails.
    pub mount_check: Option<bool>,
    /// How long to cache the contents of a file after it has been accessed.
    pub cache_max_seconds: Option<u64>,
    /// How how many files to cache.
    pub cache_max_items: Option<u64>,
    /// How long to cache the size and capacity of the file system.
    pub cache_statfs_seconds: Option<u64>,
    /// How many seconds to wait before checking for remote changes and updating them locally.
    pub sync_interval: Option<u64>,
    /// Mount options.
    pub mount_options: Option<Vec<String>>,
    /// Config directory (see XDG_CONFIG_HOME).
    pub config_dir: Option<PathBuf>,
    /// Session name.
    pub session_name: Option<String>,
    /// If true, use InstalledRedirect auth flow instead of InstalledInteractive.
    pub authorize_using_code: Option<bool>,
    /// If set to true, all files with identical name will get an increasing number attached to the suffix.
    pub rename_identical_files: Option<bool>,
    /// If set to true, will add an extension to special files (docs, presentations, sheets, drawings, sites), e.g. "\#.ods" for spreadsheets.
    pub add_extensions_to_special_files: Option<bool>,
    /// If set to true, deleted files and folder will not be moved to Trash Folder, instead they get deleted permanently.
    pub skip_trash: Option<bool>,
    /// The Google OAuth client secret for Google Drive APIs (see <https://console.developers.google.com>)
    pub client_secret: Option<String>,
    /// Port for OAuth redirect during authentication.
    pub auth_port: Option<u16>,
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

    /// A list of mount options.
    pub fn mount_options(&self) -> Vec<String> {
        match self.mount_options {
            Some(ref options) => options.clone(),
            None => Vec::new(),
        }
    }

    /// The session name.
    pub fn session_name(&self) -> &String {
        self.session_name.as_ref().unwrap()
    }

    /// The path to the token file which authorizes access to a Drive account.
    pub fn token_file(&self) -> PathBuf {
        Path::new(self.config_dir.as_ref().unwrap()).join(Path::new(self.session_name()))
    }

    /// The path to the config dir.
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

    /// If set to true, all files with identical name will get an increasing number attached to the suffix.
    /// The mount-time also rises dramatically with higher file counts. Not recommended.
    pub fn rename_identical_files(&self) -> bool {
        self.rename_identical_files.unwrap_or(false)
    }

    /// If set to true, all files with identical name will get an increasing number attached to the suffix.
    pub fn add_extensions_to_special_files(&self) -> bool {
        self.add_extensions_to_special_files.unwrap_or(false)
    }

    /// If set to true, deleted files and folder will not be moved to Trash Folder, instead they get deleted permanently.
    pub fn skip_trash(&self) -> bool {
        self.skip_trash.unwrap_or(false)
    }

    /// The Google OAuth client secret for Google Drive APIs. Create your own
    /// credentials at <https://console.developers.google.com> and paste them here
    pub fn client_secret(&self) -> &String {
        self.client_secret.as_ref().unwrap()
    }

    /// Port for OAuth redirect during authentication. Default is 8081.
    pub fn auth_port(&self) -> u16 {
        self.auth_port.unwrap_or(8081)
    }
}
