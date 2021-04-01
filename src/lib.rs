//! GCSF -- A VIRTUAL FILE SYSTEM BASED ON GOOGLE DRIVE
#![deny(
    missing_docs,
    trivial_casts,
    trivial_numeric_casts,
    unsafe_code,
    unstable_features,
    unused_import_braces,
    unused_qualifications
)]
extern crate chrono;
extern crate failure;
extern crate fuser;
extern crate google_drive3_fork as drive3;
extern crate hyper;
extern crate hyper_native_tls;
extern crate id_tree;
extern crate libc;
extern crate mime_sniffer;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;
extern crate lru_time_cache;
extern crate pretty_env_logger;
extern crate rand;
extern crate serde;
extern crate serde_json;
#[macro_use]
extern crate serde_derive;
extern crate yup_oauth2 as oauth2;
#[macro_use]
extern crate lazy_static;

mod gcsf;

pub use gcsf::filesystem::{Gcsf, NullFs};
pub use gcsf::{Config, DriveFacade, FileManager};

#[cfg(test)]
mod tests;
