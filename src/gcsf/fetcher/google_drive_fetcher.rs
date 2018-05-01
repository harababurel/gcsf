use super::DataFetcher;

type Inode = u64;

// type GCClient = hyper::Client;
// type GCAuthenticator = oauth2::Authenticator<
//     oauth2::DefaultAuthenticatorDelegate,
//     oauth2::DiskTokenStorage,
//     hyper::Client,
// >;
// type GCDrive = drive3::Drive<GCClient, GCAuthenticator>;

pub struct GoogleDriveFetcher {}

impl GoogleDriveFetcher {
    // fn read_client_secret(file: &str) -> Result<oauth2::ApplicationSecret, Error> {
    //     use std::fs::OpenOptions;
    //     use std::io::Read;

    //     let mut file = OpenOptions::new().read(true).open(file)?;

    //     let mut secret = String::new();
    //     file.read_to_string(&mut secret);

    //     let app_secret: oauth2::ConsoleApplicationSecret = serde_json::from_str(secret.as_str())?;
    //     app_secret
    //         .installed
    //         .ok_or(err_msg("Option did not contain a value."))
    // }

    //fn create_drive_auth() -> Result<GCAuthenticator, Error> {
    //    // Get an ApplicationSecret instance by some means. It contains the `client_id` and
    //    // `client_secret`, among other things.
    //    //
    //    let secret: oauth2::ApplicationSecret = GCSF::read_client_secret("client_secret.json")?;

    //    // Instantiate the authenticator. It will choose a suitable authentication flow for you,
    //    // unless you replace  `None` with the desired Flow.
    //    // Provide your own `AuthenticatorDelegate` to adjust the way it operates
    //    // and get feedback about
    //    // what's going on. You probably want to bring in your own `TokenStorage`
    //    // to persist tokens and
    //    // retrieve them from storage.
    //    let auth = oauth2::Authenticator::new(
    //        &secret,
    //        oauth2::DefaultAuthenticatorDelegate,
    //        hyper::Client::with_connector(hyper::net::HttpsConnector::new(
    //            hyper_rustls::TlsClient::new(),
    //        )),
    //        // <MemoryStorage as Default>::default(),
    //        oauth2::DiskTokenStorage::new(&String::from("/tmp/gcsf_token.json")).unwrap(),
    //        Some(oauth2::FlowType::InstalledRedirect(8080)), // This is the main change!
    //    );

    //    Ok(auth)
    //}

    // fn create_drive() -> Result<GCDrive, Error> {
    //     let auth = GCSF::create_drive_auth()?;
    //     Ok(drive3::Drive::new(
    //         hyper::Client::with_connector(hyper::net::HttpsConnector::new(
    //             hyper_rustls::TlsClient::new(),
    //         )),
    //         auth,
    //     ))
    // }

    // fn ls(&self) -> Vec<drive3::File> {
    //     let result = self.drive.files()
    //     .list()
    //     .spaces("drive")
    //     .page_size(10)
    //     // .order_by("folder,modifiedTime desc,name")
    //     .corpora("user") // or "domain"
    //     .doit();

    //     match result {
    //         Err(e) => {
    //             println!("{:#?}", e);
    //             vec![]
    //         }
    //         Ok(res) => res.1.files.unwrap().into_iter().collect(),
    //     }
    // }

    // fn cat(&self, filename: &str) -> String {
    //     let result = self.drive.files()
    //     .list()
    //     .spaces("drive")
    //     .page_size(10)
    //     // .order_by("folder,modifiedTime desc,name")
    //     .corpora("user") // or "domain"
    //     .doit();
    // }
}

impl DataFetcher for GoogleDriveFetcher {
    fn new() -> GoogleDriveFetcher {
        GoogleDriveFetcher {}
    }

    fn read(&self, inode: Inode, offset: usize, size: usize) -> Option<&[u8]> {
        None
    }

    fn write(&mut self, inode: Inode, offset: usize, data: &[u8]) {}

    fn remove(&mut self, inode: Inode) {}
}
