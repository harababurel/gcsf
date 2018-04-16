#![allow(dead_code)]
#![allow(unused_must_use)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

extern crate fuse;
extern crate gcsf;
extern crate pretty_env_logger;

use fuse::{BackgroundSession, Filesystem};
use std::env;
use std::ffi::OsStr;
use std::io;

fn mount_gcsf(mountpoint: &str) {
    let options = ["-o", "ro", "-o", "fsname=GCSF"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let fs = gcsf::GCSF::new();
    fuse::mount(fs, &mountpoint, &options);
}

fn main() {
    pretty_env_logger::init();

    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        println!("Usage: {} /path/to/mountpoint", &args[0]);
        return;
    }

    let mountpoint = &args[1];
    mount_gcsf(mountpoint);
}
