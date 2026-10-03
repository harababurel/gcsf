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
use failure::{Error, err_msg};
use std::fs;
use std::io::prelude::*;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time;

use gcsf::{Config, DriveFacade, Gcsf, NullFs};

const DEBUG_LOG: &str = "hyper::client=error,hyper::http=error,hyper::net=error,debug";

fn print_auth_troubleshooting(session_name: &str) {
    eprintln!();
    eprintln!("This usually means your refresh token has expired.");
    eprintln!();
    eprintln!("Common causes:");
    eprintln!("  1. Google Cloud project is in 'Testing' mode (tokens expire after 7 days)");
    eprintln!("  2. You switched to 'Production' mode but haven't re-authenticated");
    eprintln!("  3. You revoked access in your Google Account settings");
    eprintln!();
    eprintln!("To fix:");
    eprintln!("  1. Ensure your Google Cloud project is in 'Production' mode:");
    eprintln!("     - Go to https://console.cloud.google.com");
    eprintln!("     - Navigate to 'APIs & Services' -> 'OAuth consent screen'");
    eprintln!("     - If status is 'Testing', click 'Publish App'");
    eprintln!("  2. Re-authenticate:");
    eprintln!("     gcsf logout {}", session_name);
    eprintln!("     gcsf login {}", session_name);
}

const INFO_LOG: &str =
    "hyper::client=error,hyper::http=error,hyper::net=error,fuse::session=error,info";

#[derive(Parser)]
#[command(name = "GCSF")]
#[command(version = "0.3.10")]
#[command(author = "Sergiu Puscas <srg.pscs@gmail.com>")]
#[command(about = "File system based on Google Drive")]
#[command(
    after_help = "Note: this is a work in progress. It might cause data loss. Use with caution."
)]
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

        /// Mount in read-only mode (overrides config)
        #[arg(long = "read-only", short = 'r')]
        read_only: bool,
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
    /// Verify that authentication is working for a session.
    Verify {
        /// Session name to verify.
        #[arg(value_name = "session_name")]
        session_name: String,
    },
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

# Port for OAuth redirect during authentication. Change this if port 8081
# is already in use by another application.
auth_port = 8081

# If set to true, all files with identical name will get an increasing number
# attached to the suffix. This is most likely not necessary.
rename_identical_files = false

# If set to true, will add an extension to special files (docs, presentations, sheets, drawings, sites), e.g. "\#.ods" for spreadsheets.
add_extensions_to_special_files = false

# If set to true, deleted files and folder will not be moved to Trash Folder,
# instead they get deleted permanently.
skip_trash = false

# If set to true, the filesystem will be mounted in read-only mode.
# All write operations (create, delete, rename, write) will be rejected.
# This is useful for:
#   - Preventing accidental modifications
#   - Safely browsing Drive contents
#   - Backup/archival scenarios
read_only = false

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

/// The operating system refused to mount a file system. Kept as a distinct error type so that
/// troubleshooting steps can be printed for it.
#[derive(Debug)]
struct MountFailure {
    mountpoint: String,
    source: std::io::Error,
}

impl std::fmt::Display for MountFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Could not mount to {}: {}", self.mountpoint, self.source)
    }
}

impl std::error::Error for MountFailure {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Path of the macFUSE kernel extension built for the running macOS release, if it exists.
#[cfg(target_os = "macos")]
fn macfuse_kext_path() -> Option<String> {
    let output = std::process::Command::new("/usr/bin/sw_vers")
        .arg("-productVersion")
        .output()
        .ok()?;
    let version = String::from_utf8(output.stdout).ok()?;
    let major = version.trim().split('.').next()?;
    let path = format!(
        "/Library/Filesystems/macfuse.fs/Contents/Extensions/{}/macfuse.kext",
        major
    );
    Path::new(&path).exists().then_some(path)
}

#[cfg(target_os = "macos")]
fn print_mount_troubleshooting() {
    let kext_path = macfuse_kext_path().unwrap_or_else(|| {
        String::from(
            "/Library/Filesystems/macfuse.fs/Contents/Extensions/<macOS major version>/macfuse.kext",
        )
    });

    eprintln!();
    eprintln!("On macOS, the error code above often does not reflect the actual cause.");
    eprintln!();
    eprintln!("If the macFUSE kernel extension is not approved or loaded:");
    eprintln!("  1. Open System Settings -> Privacy & Security and allow the system software");
    eprintln!("     from developer \"Benjamin Fleischer\" (macFUSE).");
    eprintln!("  2. If nothing is shown there, load the extension manually. This prints the");
    eprintln!("     actual error and usually brings up the approval prompt:");
    eprintln!("       sudo kmutil load -p {}", kext_path);
    eprintln!("  3. Restart if prompted, then mount again.");
    eprintln!();
    eprintln!("With macFUSE 5.3 or later, GCSF must be built from its GitHub repository, which");
    eprintln!("includes a fix for https://github.com/cberner/fuser/issues/752. Builds from");
    eprintln!("crates.io (`cargo install gcsf`) cannot mount until fuser releases a fix.");
    eprintln!();
    eprintln!("Also make sure the mount point exists and is not already mounted.");
}

#[cfg(not(target_os = "macos"))]
fn print_mount_troubleshooting() {}

fn mount_gcsf(config: Config, mountpoint: &str) -> Result<(), Error> {
    let mount_config = config.fuser_config();

    if config.read_only() {
        info!("Mounting in read-only mode");
    }

    let mount_failure = |source| MountFailure {
        mountpoint: mountpoint.to_string(),
        source,
    };

    if config.mount_check() {
        info!("Checking that {} can be mounted...", mountpoint);
        match fuser::spawn_mount(NullFs {}, mountpoint, &mount_config) {
            Ok(session) => {
                debug!("Test mount of NullFs successful. Will mount GCSF next.");
                if let Err(e) = session.umount_and_join() {
                    return Err(err_msg(format!(
                        "Could not cleanly unmount test filesystem: {}",
                        e
                    )));
                }
            }
            Err(e) => return Err(mount_failure(e).into()),
        };
    } else {
        warn!(
            "mount_check is disabled in the config file. Mount problems will only be detected \
             after the whole Drive file list has been fetched."
        );
    }

    info!("Creating and populating file system...");
    let fs: Gcsf = match Gcsf::with_config(config) {
        Ok(fs) => fs,
        Err(e) => {
            return Err(e);
        }
    };
    info!("File system created.");
    let control = fs.control();

    info!("Mounting to {}", mountpoint);
    match fuser::spawn_mount(fs, mountpoint, &mount_config) {
        Ok(session) => {
            info!("Mounted to {}", mountpoint);

            let running = Arc::new(AtomicBool::new(true));
            let r = running.clone();

            ctrlc::set_handler(move || {
                info!("Ctrl-C detected");
                r.store(false, Ordering::SeqCst);
            })
            .expect("Error setting Ctrl-C handler");

            while running.load(Ordering::SeqCst) && !session.guard.is_finished() {
                thread::sleep(time::Duration::from_millis(50));
            }

            let initial_flush_error = control.begin_shutdown().err();
            let session_result = if session.guard.is_finished() {
                session.join()
            } else {
                session.umount_and_join()
            };
            let final_flush_result = control.flush_pending();

            let mut errors = Vec::new();
            if let Err(error) = session_result {
                errors.push(format!("Filesystem session ended with an error: {}", error));
            }
            if let Err(error) = final_flush_result {
                if let Some(initial_error) = initial_flush_error {
                    errors.push(format!("Initial shutdown flush failed: {}", initial_error));
                }
                errors.push(format!("Final shutdown flush failed: {}", error));
            }

            if errors.is_empty() {
                Ok(())
            } else {
                Err(err_msg(errors.join("; ")))
            }
        }
        Err(e) => Err(mount_failure(e).into()),
    }
}

fn login(config: &mut Config) -> Result<(), Error> {
    debug!("{:#?}", config);

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

/// Whether a file in the configuration directory holds session credentials.
///
/// A session is the token file yup-oauth2 persists: entries pairing the scopes
/// a token was granted for with the token itself. Every field of the token is
/// optional, so the shape is what identifies the file. Anything else living in
/// the same directory (the configuration, backups of it, subdirectories) is not
/// a session and should not be listed as one.
fn is_session_file(path: &Path) -> bool {
    let Ok(contents) = fs::read_to_string(path) else {
        return false;
    };

    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&contents) else {
        return false;
    };

    // Current versions store a bare array. Older ones wrapped the same entries
    // in an object under "tokens"; sessions written back then are still listed
    // so that they remain visible to `gcsf logout`.
    let entries = match parsed.get("tokens") {
        Some(tokens) => tokens.as_array(),
        None => parsed.as_array(),
    };

    entries.is_some_and(|entries| {
        !entries.is_empty()
            && entries
                .iter()
                .all(|entry| entry["scopes"].is_array() && entry["token"].is_object())
    })
}

fn load_conf() -> Result<Config, Error> {
    let xdg_dirs = xdg::BaseDirectories::with_prefix("gcsf");
    let config_file = xdg_dirs
        .place_config_file("gcsf.toml")
        .map_err(|_| err_msg("Cannot create configuration directory"))?;

    info!("Config file: {:?}", config_file);

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
    // reqwest is built with `rustls-no-provider`, which avoids pulling in
    // aws-lc-rs (and its cmake build dependency). The process-wide crypto
    // provider must therefore be installed before any TLS client is built.
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Could not install rustls crypto provider.");

    let mut config = load_conf().expect("Could not load configuration file.");

    pretty_env_logger::formatted_builder()
        .parse_filters(if config.debug() { DEBUG_LOG } else { INFO_LOG })
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Login { session_name } => {
            config.session_name = Some(session_name);

            if config.token_file().exists() {
                error!("Token file {:?} already exists.", config.token_file());
                return;
            }

            let result = if config.authorize_using_code() {
                // Headless mode: use manual URL paste flow for remote servers
                let secret: serde_json::Value =
                    serde_json::from_str(config.client_secret()).expect("Invalid client_secret");
                let installed = &secret["installed"];

                gcsf::auth::headless_login(
                    installed["client_id"].as_str().expect("Missing client_id"),
                    installed["client_secret"]
                        .as_str()
                        .expect("Missing client_secret"),
                    &config.token_file(),
                    config.auth_port(),
                )
            } else {
                // Standard mode: automatic localhost redirect
                login(&mut config)
            };

            match result {
                Ok(_) => {
                    println!(
                        "Successfully logged in. Saved credentials to {:?}",
                        config.token_file()
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
                    println!("Successfully removed {:?}", tf);
                }
                Err(e) => {
                    println!("Could not remove {:?}: {}", tf, e);
                }
            };
        }
        Commands::List => {
            let mut sessions: Vec<_> = fs::read_dir(config.config_dir())
                .unwrap()
                .map(Result::unwrap)
                .filter(|entry| is_session_file(&entry.path()))
                .map(|f| f.file_name().to_str().unwrap().to_string())
                .collect();
            sessions.sort();

            if sessions.is_empty() {
                println!("No sessions found.");
            } else {
                println!("Sessions:");
                for session in sessions {
                    println!("\t- {}", session);
                }
            }
        }
        Commands::Verify { session_name } => {
            config.session_name = Some(session_name.clone());

            if !config.token_file().exists() {
                error!("Token file {:?} does not exist.", config.token_file());
                error!("Run `gcsf login {}` first.", session_name);
                std::process::exit(1);
            }

            if config.client_secret.is_none() {
                error!("No Google OAuth client secret was provided.");
                std::process::exit(1);
            }

            println!("Verifying authentication for session '{}'...", session_name);

            let mut df = DriveFacade::new(&config);
            match df.validate_auth() {
                Ok(_) => {
                    println!("Authentication is valid.");
                    println!("Token file: {:?}", config.token_file());
                }
                Err(e) => {
                    error!("Authentication failed: {}", e);
                    print_auth_troubleshooting(&session_name);
                    std::process::exit(1);
                }
            }
        }
        Commands::Mount {
            session_name,
            mountpoint,
            read_only,
        } => {
            config.session_name = Some(session_name.clone());

            // CLI flag overrides config if set
            if read_only {
                config.read_only = Some(true);
            }

            if !config.token_file().exists() {
                error!("Token file {:?} does not exist.", config.token_file());
                error!("Try logging in first using `gcsf login`.");
                return;
            }

            if config.client_secret.is_none() {
                error!("No Google OAuth client secret was provided.");
                error!(
                    "Try deleting your config file to force GCSF to generate it with the default credentials."
                );
                error!(
                    "Alternatively, you can create your own credentials or manually set the default ones from https://github.com/harababurel/gcsf/blob/master/sample_config.toml"
                );
                return;
            }

            // Validate authentication before attempting to mount
            info!("Validating authentication...");
            let mut df = DriveFacade::new(&config);
            match df.validate_auth() {
                Ok(_) => {
                    info!("Authentication valid.");
                }
                Err(e) => {
                    error!("Authentication failed: {}", e);
                    print_auth_troubleshooting(&session_name);
                    return;
                }
            }
            drop(df); // Release the DriveFacade before mount creates its own

            if let Err(error) = mount_gcsf(config, &mountpoint) {
                error!("{}", error);
                if error.downcast_ref::<MountFailure>().is_some() {
                    print_mount_troubleshooting();
                }
                std::process::exit(1);
            }
        }
    }
}
