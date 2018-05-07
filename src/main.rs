extern crate ctrlc;
extern crate fuse;
extern crate gcsf;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;

use std::env;
use std::ffi::OsStr;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time;

use gcsf::{GoogleDriveFetcher, GCSF};

fn mount_gcsf(mountpoint: &str) {
    let options = ["-o", "fsname=GCSF", "-o", "allow_root"]
        .iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let fs: GCSF<GoogleDriveFetcher> = GCSF::new();
    unsafe {
        match fuse::spawn_mount(fs, &mountpoint, &options) {
            Ok(_session) => {
                info!("Mounted to {}", &mountpoint);

                let running = Arc::new(AtomicBool::new(true));
                let r = running.clone();

                ctrlc::set_handler(move || {
                    info!("Ctrl-C detected");
                    r.store(false, Ordering::SeqCst);
                }).expect("Error setting Ctrl-C handler");

                while running.load(Ordering::SeqCst) {
                    thread::sleep(time::Duration::from_millis(50));
                }
            }
            Err(e) => error!("Could not mount to {}: {}", &mountpoint, e),
        };
    }
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
