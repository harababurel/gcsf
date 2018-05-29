#![allow(dead_code)]
#![allow(unused_imports)]
#![allow(unused_variables)]

extern crate failure;
extern crate fuse;
extern crate google_drive3 as drive3;
extern crate hyper;
extern crate hyper_rustls;
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
extern crate time;
extern crate yup_oauth2 as oauth2;
#[macro_use]
extern crate lazy_static;

mod gcsf;

pub use gcsf::DriveFacade;
pub use gcsf::filesystem::GCSF;

#[cfg(test)]
mod tests;
