#![allow(dead_code)]
#![allow(unused_must_use)]
#![allow(unused_imports)]
#![allow(unused_mut)]
#![allow(unreachable_code)]

extern crate fuse;
extern crate gcsf;

use fuse::Filesystem;
use std::ffi::OsStr;

fn mount_gcsf() {
    let mountpoint = String::from("/tmp/gcsf");
    let options = ["-o", "ro", "-o", "fsname=GCSF"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let fs = gcsf::GCSF::new();
    fuse::mount(fs, &mountpoint, &options).unwrap();
}

fn main() {
    mount_gcsf();
}
