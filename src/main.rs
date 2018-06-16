#[macro_use]
extern crate clap;
extern crate config;
extern crate ctrlc;
extern crate failure;
extern crate fuse;
extern crate gcsf;
#[macro_use]
extern crate log;
extern crate itertools;
extern crate pretty_env_logger;
extern crate serde;
extern crate serde_json;
extern crate xdg;

use clap::App;
use failure::{err_msg, Error};
use itertools::Itertools;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::iter;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time;

use gcsf::{Config, NullFS, GCSF};

const DEFAULT_LOG: &str =
    "hyper::client=error,rustls::client_hs=error,hyper::http=error,hyper::net=error,debug";

fn mount_gcsf(config: Config, mountpoint: &str) {
    let vals = config.mount_options();
    let mut options = iter::repeat("-o")
        .interleave_shortest(vals.iter().map(String::as_ref))
        .map(OsStr::new)
        .collect::<Vec<_>>();
    options.pop();

    unsafe {
        match fuse::spawn_mount(NullFS {}, &mountpoint, &options) {
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

    let fs: GCSF = GCSF::with_config(config);
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

fn load_conf() -> Result<Config, Error> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("gcsf").unwrap();
    let config_path = xdg_dirs
        .place_config_file("gcsf.toml")
        .map_err(|_| err_msg("Cannot create configuration directory"))?;

    info!("Config file: {:?}", &config_path);

    let token_path = xdg_dirs
        .place_config_file("auth_token.json")
        .map_err(|_| err_msg("Cannot create configuration directory"))?;

    let mut settings = config::Config::default();
    settings.merge(config::File::with_name(config_path.to_str().unwrap()))?;
    settings.merge(config::Environment::with_prefix("GCSF"))?;

    let mut config = settings.try_into::<Config>()?;
    config.token_path = Some(token_path.to_str().unwrap().to_string());

    Ok(config)
}

fn main() {
    pretty_env_logger::formatted_builder()
        .unwrap()
        .parse(&env::var("RUST_LOG").unwrap_or(DEFAULT_LOG.to_string()))
        .init();

    let config = match load_conf() {
        Ok(config) => {
            debug!("{:#?}", &config);
            config
        }
        Err(e) => {
            error!("{}", e);
            return;
        }
    };

    let yaml = load_yaml!("cli.yml");
    let matches = App::from_yaml(yaml).get_matches();

    if let Some(_matches) = matches.subcommand_matches("logout") {
        let filename = config.token_path.as_ref().unwrap();
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
        mount_gcsf(config, mountpoint);
    }
}
