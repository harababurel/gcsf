#[macro_use]
extern crate clap;
extern crate ctrlc;
extern crate fuse;
extern crate gcsf;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;

use clap::App;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time;

use gcsf::{NullFS, GCSF};

const DEFAULT_LOG: &str =
    "hyper::client=error,rustls::client_hs=error,hyper::http=error,hyper::net=error,debug";

fn mount_gcsf(mountpoint: &str) {
    let options = [
        "-o",
        "fsname=GCSF",
        "-o",
        "allow_root",
        "-o",
        "big_writes",
        "-o",
        "max_write=131072",
    ].iter()
        .map(|o| o.as_ref())
        .collect::<Vec<&OsStr>>();

    let nullfs = NullFS {};
    unsafe {
        match fuse::spawn_mount(nullfs, &mountpoint, &options) {
            Ok(session) => {
                debug!("Test mount of NullFS successful. Will mount GCSF next.");
                drop(session);
            }
            Err(e) => {
                error!("Could not mount to {}: {}", &mountpoint, e);
                return;
            }
        };
    }

    let fs: GCSF = GCSF::new();
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
    pretty_env_logger::formatted_builder()
        .unwrap()
        .parse(&env::var("RUST_LOG").unwrap_or(DEFAULT_LOG.to_string()))
        .init();

    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    if let Some(_matches) = matches.subcommand_matches("logout") {
        let filename = "/tmp/gcsf_token.json";
        match fs::remove_file(filename) {
            Ok(_) => {
                println!("Successfully removed {}", filename);
            }
            Err(e) => {
                println!("Could not remove {}: {}", filename, e);
            }
        };
    }

    if let Some(matches) = matches.subcommand_matches("mount") {
        let mountpoint = matches.value_of("mountpoint").unwrap();
        mount_gcsf(mountpoint);
    }
}
