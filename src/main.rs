extern crate clap;
extern crate config;
extern crate ctrlc;
extern crate failure;
extern crate fuser;
extern crate gcsf;
#[macro_use]
extern crate log;
extern crate pretty_env_logger;
extern crate serde;
extern crate serde_json;
extern crate xdg;

use clap::{Parser, Subcommand};
use failure::{err_msg, Error};
use std::fs;
use std::io::prelude::*;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::thread;
use std::time;

use gcsf::{Config, DriveFacade, Gcsf, NullFs};

const DEBUG_LOG: &str = "hyper::client=error,hyper::http=error,hyper::net=error,debug";

const INFO_LOG: &str =
    "hyper::client=error,hyper::http=error,hyper::net=error,fuse::session=error,info";

#[derive(Parser)]
#[command(name = "GCSF")]
#[command(version = "0.3.0")]
#[command(author = "Sergiu Puscas <srg.pscs@gmail.com>")]
#[command(about = "File system based on Google Drive")]
#[command(after_help = "Note: this is a work in progress. It might cause data loss. Use with caution.")]
#[command(subcommand_required = true)]
#[command(arg_required_else_help = true)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Mount the file system.
    Mount {
        /// An existing session name set during `gcsf login`
        #[arg(short = 's', long = "session", value_name = "session_name")]
        session_name: String,

        /// Path to mount directory
        #[arg(value_name = "mount_directory")]
        mountpoint: String,
    },
    /// Login to Drive (create a new session).
    Login {
        /// User-defined name for this session.
        #[arg(value_name = "session_name")]
        session_name: String,
    },
    /// Logout (delete a given session).
    Logout {
        /// User-defined session name.
        #[arg(value_name = "session_name")]
        session_name: String,
    },
    /// List sessions.
    List,
}

const DEFAULT_CONFIG: &str = r#"
### This is the configuration file that GCSF uses.
### It should be placed in $XDG_CONFIG_HOME/gcsf/gcsf.toml, which is usually
### defined as $HOME/.config/gcsf/gcsf.toml

# Show additional logging info?
debug = false

# Perform a mount check and fail early if it fails. Disable this if you
# encounter this error:
#
#     fuse: attempt to remount on active mount point: [...]
#     Could not mount to [...]: Undefined error: 0 (os error 0)
mount_check = true

# How long to cache the contents of a file after it has been accessed.
cache_max_seconds = 300

# How how many files to cache.
cache_max_items = 10

# How long to cache the size and capacity of the file system. These are the
# values reported by `df`.
cache_statfs_seconds = 60

# How many seconds to wait before checking for remote changes and updating them
# locally.
sync_interval = 60

# Mount options
mount_options = [
    "fsname=GCSF",
    # Allow file system access to root. This only works if `user_allow_other`
    # is set in /etc/fuse.conf
    "allow_root",
]

# If set to true, Google Drive will provide a code after logging in and
# authorizing GCSF. This code must be copied and pasted into GCSF in order to
# complete the process. Useful for running GCSF on a remote server.
#
# If set to false, Google Drive will attempt to communicate with GCSF directly.
# This is usually faster and more convenient.
authorize_using_code = false

# If set to true, all files with identical name will get an increasing number
# attached to the suffix. This is most likely not necessary.
rename_identical_files = false

# If set to true, will add an extension to special files (docs, presentations, sheets, drawings, sites), e.g. "\#.ods" for spreadsheets.
add_extensions_to_special_files = false

# If set to true, deleted files and folder will not be moved to Trash Folder,
# instead they get deleted permanently.
skip_trash = false

# The Google OAuth client secret for Google Drive APIs. Create your own
# credentials at https://console.developers.google.com and paste them here
client_secret = """
  {
  "installed": {
    "client_id": "892276709198-2ksebnrqkhihtf5p743k4ce5bk0n7p5a.apps.googleusercontent.com",
    "project_id": "gcsf-v02",
    "auth_uri": "https://accounts.google.com/o/oauth2/auth",
    "token_uri": "https://oauth2.googleapis.com/token",
    "auth_provider_x509_cert_url": "https://www.googleapis.com/oauth2/v1/certs",
    "client_secret": "1ImxorJzh-PuH2CxrcLPnJMU",
    "redirect_uris": ["urn:ietf:wg:oauth:2.0:oob", "http://localhost"]
  }
}"""
"#;

fn mount_gcsf(config: Config, mountpoint: &str) {
    // TODO: consider making these configurable in the config file
    let options = [
        fuser::MountOption::FSName(String::from("GCSF")),
        fuser::MountOption::AllowRoot,
    ];

    if config.mount_check() {
        match fuser::spawn_mount2(NullFs {}, mountpoint, &options) {
            Ok(session) => {
                debug!("Test mount of NullFs successful. Will mount GCSF next.");
                drop(session);
            }
            Err(e) => {
                error!("Could not mount to {}: {}", mountpoint, e);
                return;
            }
        };
    }

    info!("Creating and populating file system...");
    let fs: Gcsf = match Gcsf::with_config(config) {
        Ok(fs) => fs,
        Err(e) => {
            error!("{}", e);
            return;
        }
    };
    info!("File system created.");

    info!("Mounting to {}", &mountpoint);
    match fuser::spawn_mount2(fs, mountpoint, &options) {
        Ok(_session) => {
            info!("Mounted to {}", &mountpoint);

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();

            ctrlc::set_handler(move || {
                info!("Ctrl-C detected");
                r.store(false, Ordering::SeqCst);
            })
            .expect("Error setting Ctrl-C handler");

            while running.load(Ordering::SeqCst) {
                thread::sleep(time::Duration::from_millis(50));
            }
        }
        Err(e) => error!("Could not mount to {}: {}", &mountpoint, e),
    };
}

fn login(config: &mut Config) -> Result<(), Error> {
    debug!("{:#?}", &config);

    if config.token_file().exists() {
        return Err(err_msg(format!(
            "token file {:?} already exists.",
            config.token_file()
        )));
    }

    // Create a DriveFacade which will store the authentication token in the desired file.
    // And make an arbitrary request in order to trigger the authentication process.
    let mut df = DriveFacade::new(config);
    let _result = df.root_id()?;

    Ok(())
}

fn load_conf() -> Result<Config, Error> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("gcsf");
    let config_file = xdg_dirs
        .place_config_file("gcsf.toml")
        .map_err(|_| err_msg("Cannot create configuration directory"))?;

    info!("Config file: {:?}", &config_file);

    if !config_file.exists() {
        let mut config_file = fs::File::create(config_file.clone())
            .map_err(|_| err_msg("Could not create config file"))?;
        config_file.write_all(DEFAULT_CONFIG.as_bytes())?;
    }

    // let mut settings = config::Config::default();

    let settings = config::ConfigBuilder::<config::builder::DefaultState>::default()
        .add_source(config::File::with_name(config_file.to_str().unwrap()))
        .build()
        .unwrap();

    // settings
    //     .merge(config::File::with_name(config_file.to_str().unwrap()))
    //     .expect("Invalid configuration file");

    // let mut config = TryInto::<Config>::try_into(settings)?;
    let mut config: gcsf::Config = settings.try_deserialize()?;
    config.config_dir = xdg_dirs.get_config_home();

    Ok(config)
}

fn main() {
    let mut config = load_conf().expect("Could not load configuration file.");

    pretty_env_logger::formatted_builder()
        .parse_filters(if config.debug() { DEBUG_LOG } else { INFO_LOG })
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Login { session_name } => {
            config.session_name = Some(session_name);

            match login(&mut config) {
                Ok(_) => {
                    println!(
                        "Successfully logged in. Saved credentials to {:?}",
                        &config.token_file()
                    );
                }
                Err(e) => {
                    error!("Could not log in: {}", e);
                }
            };
        }
        Commands::Logout { session_name } => {
            config.session_name = Some(session_name);
            let tf = config.token_file();
            match fs::remove_file(&tf) {
                Ok(_) => {
                    println!("Successfully removed {:?}", &tf);
                }
                Err(e) => {
                    println!("Could not remove {:?}: {}", &tf, e);
                }
            };
        }
        Commands::List => {
            let exception = String::from("gcsf.toml");
            let mut sessions: Vec<_> = fs::read_dir(config.config_dir())
                .unwrap()
                .map(Result::unwrap)
                .map(|f| f.file_name().to_str().unwrap().to_string())
                .filter(|name| name != &exception)
                .collect();
            sessions.sort();

            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Sessions:");
                for session in sessions {
                    println!("\t- {}", &session);
                }
            }
        }
        Commands::Mount {
            session_name,
            mountpoint,
        } => {
            config.session_name = Some(session_name);

            if !config.token_file().exists() {
                error!("Token file {:?} does not exist.", config.token_file());
                error!("Try logging in first using `gcsf login`.");
                return;
            }

            if config.client_secret.is_none() {
                error!("No Google OAuth client secret was provided.");
                error!("Try deleting your config file to force GCSF to generate it with the default credentials.");
                error!("Alternatively, you can create your own credentials or manually set the default ones from https://github.com/harababurel/gcsf/blob/master/sample_config.toml");
                return;
            }

            mount_gcsf(config, &mountpoint);
        }
    }
}
