use fuse;
use fuse::{FileType, ReplyDirectory, Request};
use libc::ENOENT;
use oauth2;
use hyper;
use hyper::Client;
use std::borrow::BorrowMut;
use hyper_rustls;
use serde;
use serde_json;
use drive3::{Drive, Error, Result};
use std::default::Default;
use oauth2::{ApplicationSecret, Authenticator, AuthenticatorDelegate, ConsoleApplicationSecret,
             DefaultAuthenticatorDelegate, DiskTokenStorage, GetToken, MemoryStorage, TokenStorage};

type GCClient = hyper::Client;
type GCAuthenticator = oauth2::Authenticator<
    oauth2::DefaultAuthenticatorDelegate,
    oauth2::DiskTokenStorage,
    hyper::Client,
>;
type GCDrive = Drive<GCClient, GCAuthenticator>;

pub struct GCSF {
    hub: GCDrive,
}

impl GCSF {
    pub fn new() -> GCSF {
        GCSF {
            hub: GCSF::create_drive_hub(),
        }
    }

    fn read_client_secret(file: &str) -> ApplicationSecret {
        use std::fs::{File, OpenOptions};
        use std::io::Read;

        let mut secret = String::new();
        OpenOptions::new()
            .read(true)
            .open(file)
            .unwrap()
            .read_to_string(&mut secret);
        let consappsec: ConsoleApplicationSecret = serde_json::from_str(secret.as_str()).unwrap();
        consappsec.installed.unwrap()
    }

    fn create_drive_auth() -> GCAuthenticator {
        // Get an ApplicationSecret instance by some means. It contains the `client_id` and
        // `client_secret`, among other things.
        let secret: ApplicationSecret = GCSF::read_client_secret("client_secret.json");

        // Instantiate the authenticator. It will choose a suitable authentication flow for you,
        // unless you replace  `None` with the desired Flow.
        // Provide your own `AuthenticatorDelegate` to adjust the way it operates and get feedback about
        // what's going on. You probably want to bring in your own `TokenStorage` to persist tokens and
        // retrieve them from storage.
        let auth = Authenticator::new(
            &secret,
            DefaultAuthenticatorDelegate,
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            // <MemoryStorage as Default>::default(),
            DiskTokenStorage::new(&String::from("/tmp/gcsf_token.json")).unwrap(),
            Some(oauth2::FlowType::InstalledRedirect(8080)), // This is the main change!
        );

        auth
    }

    fn create_drive_hub() -> GCDrive {
        let auth = GCSF::create_drive_auth();
        Drive::new(
            hyper::Client::with_connector(hyper::net::HttpsConnector::new(
                hyper_rustls::TlsClient::new(),
            )),
            auth,
        )
    }
}
/*
        // You can configure optional parameters by calling the respective setters at will, and
        // execute the final call using `doit()`.
        // Values shown here are possibly random and not representative !
        let result = hub.files()
        .list()
        // .team_drive_id("eirmod")
        // .supports_team_drives(true)
        .spaces("drive")
        // .q("sed")
        // .page_token("et")
        .page_size(70)
        // .order_by("folder,modifiedTime desc,name")
        // .include_team_drive_items(true)
        .corpora("user") // or "domain"
        .doit();

        match result {
            Err(e) => match e {
                // The Error enum provides details about what exactly happened.
                // You can also just use its `Debug`, `Display` or `Error` traits
                Error::HttpError(_)
                | Error::MissingAPIKey
                | Error::MissingToken(_)
                | Error::Cancelled
                | Error::UploadSizeLimitExceeded(_, _)
                | Error::Failure(_)
                | Error::BadRequest(_)
                | Error::FieldClash(_)
                | Error::JsonDecodeError(_, _) => println!("{:#?}", e),
            },
            Ok(res) => {
                // println!("Success: {:#?}", res.1);

                for file in res.1.files.unwrap() {
                    println!("{} ({})", file.name.unwrap(), file.mime_type.unwrap());
                }
            }
        }
    }
    */

impl fuse::Filesystem for GCSF {
    fn readdir(
        &mut self,
        _req: &Request,
        ino: u64,
        _fh: u64,
        offset: i64,
        mut reply: ReplyDirectory,
    ) {
        // if ino == 1 {
        //     if offset == 0 {
        reply.add(1, 0, FileType::Directory, ".");
        reply.add(1, 1, FileType::Directory, "..");
        reply.add(2, 2, FileType::RegularFile, "hello.txt");
        // }
        reply.ok();
        // } else {
        //     reply.error(ENOENT);
        // }
    }
}
