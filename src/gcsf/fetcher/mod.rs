mod data_fetcher;
mod google_drive_fetcher;
mod in_memory_fetcher;

pub use self::data_fetcher::DataFetcher;
pub use self::google_drive_fetcher::GoogleDriveFetcher;
pub use self::in_memory_fetcher::InMemoryFetcher;
