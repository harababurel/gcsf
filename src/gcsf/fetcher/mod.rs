mod data_fetcher;
mod in_memory_fetcher;
mod google_drive_fetcher;

pub use self::data_fetcher::DataFetcher;
pub use self::in_memory_fetcher::InMemoryFetcher;
pub use self::google_drive_fetcher::GoogleDriveFetcher;
