use std::time::Duration;

#[derive(Deserialize, Clone, Debug)]
pub struct Config {
    debug: Option<bool>,
    cache_max_seconds: Option<u64>,
    cache_max_items: Option<u64>,
    cache_statfs_seconds: Option<u64>,
    sync_interval: Option<u64>,
    mount_options: Option<Vec<String>>,
    pub token_path: Option<String>,
    authorize_using_code: Option<bool>,
}

impl Config {
    pub fn debug(&self) -> bool {
        self.debug.unwrap_or(false)
    }

    pub fn cache_max_seconds(&self) -> Duration {
        Duration::from_secs(self.cache_max_seconds.unwrap_or(10))
    }

    pub fn cache_max_items(&self) -> u64 {
        self.cache_max_items.unwrap_or(10)
    }

    pub fn cache_statfs_seconds(&self) -> Duration {
        Duration::from_secs(self.cache_statfs_seconds.unwrap_or(100))
    }

    pub fn sync_interval(&self) -> Duration {
        Duration::from_secs(self.sync_interval.unwrap_or(10))
    }

    pub fn mount_options(&self) -> Vec<String> {
        match self.mount_options {
            Some(ref options) => options.clone(),
            None => Vec::new(),
        }
    }

    pub fn token_path(&self) -> &str {
        self.token_path.as_ref().unwrap()
    }

    pub fn authorize_using_code(&self) -> bool {
        self.authorize_using_code.unwrap_or(true)
    }
}
