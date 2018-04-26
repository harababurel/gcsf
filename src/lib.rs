#![allow(unused_imports)]
#![allow(dead_code)]
#![allow(unused_must_use)]
#![allow(unused_mut)]
#![allow(unused_variables)]
#![allow(unreachable_code)]

extern crate failure;
extern crate fuse;
extern crate google_drive3 as drive3;
extern crate hyper;
extern crate hyper_rustls;
extern crate id_tree;
extern crate libc;
#[macro_use]
extern crate log;
#[macro_use]
extern crate maplit;
extern crate pretty_env_logger;
extern crate serde;
extern crate serde_json;
extern crate time;
extern crate yup_oauth2 as oauth2;

mod gcsf;

pub use gcsf::filesystem::GCSF;
