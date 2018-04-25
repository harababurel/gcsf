mod file;
pub mod filesystem;

pub use self::file::File;

// type GCClient = hyper::Client;
// type GCAuthenticator = oauth2::Authenticator<
//     oauth2::DefaultAuthenticatorDelegate,
//     oauth2::DiskTokenStorage,
//     hyper::Client,
// >;
// type GCDrive = drive3::Drive<GCClient, GCAuthenticator>;
