use super::{Checksums, Config};
use drive3::hyper;
use drive3::hyper_rustls;
use drive3::yup_oauth2 as oauth2;
use google_drive3::hyper_rustls::HttpsConnector;
use http_body_util::BodyExt;
use hyper::Response;

use failure::{Error, err_msg};
use lru_time_cache::LruCache;
use mime_sniffer::MimeTypeSniffer;
use std::cmp;
use std::collections::{HashMap, HashSet};
use std::io;
use std::io::{Read, Seek, SeekFrom};
use tokio::runtime::Runtime;

const PAGE_SIZE: i32 = 1000;
type DriveId = String;
type DriveIdRef<'a> = &'a str;

type DriveHub =
    drive3::api::DriveHub<HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>>;

/// Provides a simple high-level interface for interacting with the Google Drive API.
pub struct DriveFacade {
    /// The `drive3::DriveHub` used for interacting with the API.
    #[cfg(not(test))]
    pub hub: DriveHub,

    /// The optional Drive client used by offline unit tests.
    #[cfg(test)]
    pub hub: Option<DriveHub>,

    /// A buffer used for temporarily caching read blocks. Storing this inside the struct makes it possible to return a reference to the data without the danger of the data outliving the struct.
    buff: Vec<u8>,

    /// Maps Drive IDs to a list of pending write operations that must be applied on them.
    pending_writes: HashMap<DriveId, Vec<PendingOperation>>,

    /// The LRU cache used for storing the file contents for any given Drive ID.
    cache: LruCache<DriveId, Vec<u8>>,

    /// Checksums Drive reported for content this instance uploaded (on create or flush). They
    /// supersede the checksums in the file's listed metadata until the next change sync for the
    /// file delivers fresh metadata.
    uploaded_checksums: HashMap<DriveId, Checksums>,

    /// Keeps track of the page token used for receiving changes from the `changes.list` API endpoint.
    changes_token: Option<String>,

    /// The root id is only stored once, effectively caching the root id.
    root_id: Option<String>,
}

/// Represents a write operation that has been performed from the user's point of view but has not
/// yet been applied to the local or remote file.
#[derive(Clone, Debug, Eq, PartialEq)]
enum PendingOperation {
    Write { offset: usize, data: Vec<u8> },
    Truncate { size: usize },
}

struct FlushFailure {
    error: Error,
    outcome_indeterminate: bool,
}

enum UpdateFileError {
    BeforeUpload(Error),
    Drive(Box<drive3::Error>),
}

lazy_static! {
    static ref MIME_TYPES: HashMap<&'static str, &'static str> = hashmap! {
        "application/vnd.google-apps.document" => "application/vnd.oasis.opendocument.text",
        "application/vnd.google-apps.presentation" => "application/vnd.oasis.opendocument.presentation",
        // Drive advertises ODS in exportLinks but answers export requests for it
        // with HTTP 500, so ask for xlsx instead. Keep EXTENSIONS in file.rs in
        // sync with the formats chosen here.
        "application/vnd.google-apps.spreadsheet" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        "application/vnd.google-apps.drawing" => "image/png",
        "application/vnd.google-apps.site" => "text/plain",
    };
}

lazy_static! {
    static ref UNEXPORTABLE_MIME_TYPES: HashSet<&'static str> = hashset! {
        "application/vnd.google-apps.form",
        "application/vnd.google-apps.map",
    };
}

lazy_static! {
    /// The metadata GCSF needs for every file it lists, whether from `files.list` or
    /// `changes.list`.
    static ref LISTED_FILE_FIELDS: String = format!(
        "name,id,size,mimeType,owners,parents,trashed,modifiedTime,createdTime,viewedByMeTime,{}",
        Checksums::DRIVE_FIELDS
    );
}

lazy_static! {
    /// Reasons Drive gives for an export it will never be able to perform. Files
    /// hitting one of these are described in place rather than failing to read.
    static ref PERMANENT_EXPORT_FAILURES: HashSet<&'static str> = hashset! {
        // The file is past the size Drive is willing to export (10MB at the time
        // of writing). Reported as HTTP 403.
        "exportSizeLimitExceeded",
        // Export only applies to Docs Editors files.
        "fileNotExportable",
        "cannotDownloadAbusiveFile",
    };
}

/// Collects the `reason` fields Drive reports in an API error body. Returns an
/// empty vector for transport-level errors, which carry no such body.
fn error_reasons(error: &drive3::Error) -> Vec<String> {
    error_details(error, "reason")
}

/// Collects the human-readable `message` fields Drive reports in an API error body.
fn error_messages(error: &drive3::Error) -> Vec<String> {
    error_details(error, "message")
}

fn is_not_found(error: &drive3::Error) -> bool {
    match error {
        drive3::Error::Failure(response) => response.status() == hyper::StatusCode::NOT_FOUND,
        drive3::Error::BadRequest(body) => body["error"]["code"].as_u64() == Some(404),
        _ => false,
    }
}

fn error_details(error: &drive3::Error, field: &str) -> Vec<String> {
    let drive3::Error::BadRequest(body) = error else {
        return Vec::new();
    };

    body["error"]["errors"]
        .as_array()
        .map(|errors| {
            errors
                .iter()
                .filter_map(|e| e[field].as_str())
                .map(String::from)
                .collect()
        })
        .unwrap_or_default()
}

impl DriveFacade {
    /// Creates a new DriveFacade with a given config.
    pub fn new(config: &Config) -> Self {
        debug!("DriveFacade::new()");

        let ttl = config.cache_max_seconds();
        let max_count = config.cache_max_items() as usize;

        DriveFacade {
            #[cfg(not(test))]
            hub: DriveFacade::create_drive(config).unwrap(),
            #[cfg(test)]
            hub: Some(DriveFacade::create_drive(config).unwrap()),
            buff: Vec::new(),
            pending_writes: HashMap::new(),
            cache: LruCache::<String, Vec<u8>>::with_expiry_duration_and_capacity(ttl, max_count),
            uploaded_checksums: HashMap::new(),
            root_id: None,
            changes_token: None,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_for_testing() -> Self {
        Self {
            hub: None,
            buff: Vec::new(),
            pending_writes: HashMap::new(),
            cache: LruCache::with_capacity(10),
            uploaded_checksums: HashMap::new(),
            changes_token: None,
            root_id: None,
        }
    }

    #[cfg(not(test))]
    fn hub(&self) -> Result<&DriveHub, Error> {
        Ok(&self.hub)
    }

    #[cfg(test)]
    fn hub(&self) -> Result<&DriveHub, Error> {
        self.hub
            .as_ref()
            .ok_or_else(|| err_msg("Drive client is unavailable"))
    }

    /// Creates a Drive authenticator.
    fn create_drive_auth(
        config: &Config,
    ) -> Result<
        oauth2::authenticator::Authenticator<
            HttpsConnector<hyper_util::client::legacy::connect::HttpConnector>,
        >,
        Error,
    > {
        let secret: oauth2::ConsoleApplicationSecret =
            serde_json::from_str(config.client_secret())?;
        let secret = secret
            .installed
            .ok_or_else(|| err_msg("ConsoleApplicationSecret.installed is None"))?;

        let rt = Runtime::new().unwrap();
        let auth = rt.block_on(
            oauth2::InstalledFlowAuthenticator::builder(
                secret,
                // Always use HTTPPortRedirect - the OOB/Interactive flow is deprecated by Google
                oauth2::InstalledFlowReturnMethod::HTTPPortRedirect(config.auth_port()),
            )
            .persist_tokens_to_disk(config.token_file())
            .build(),
        )?;
        Ok(auth)
    }

    /// Creates a drive hub.
    fn create_drive(config: &Config) -> Result<DriveHub, Error> {
        let auth = Self::create_drive_auth(config)?;

        Ok(google_drive3::DriveHub::new(
            hyper_util::client::legacy::Client::builder(hyper_util::rt::TokioExecutor::new())
                .build(
                    hyper_rustls::HttpsConnectorBuilder::new()
                        .with_native_roots()?
                        .https_or_http()
                        .enable_http1()
                        .enable_http2()
                        .build(),
                ),
            auth,
        ))
    }

    /// Will still detect a file even if it is in Trash.
    fn contains(&self, id: DriveIdRef) -> Result<bool, Error> {
        let rt = Runtime::new().unwrap();
        let response = rt.block_on(
            self.hub()?
                .files()
                .get(id)
                .add_scope(drive3::api::Scope::Full)
                .doit(),
        );

        match response {
            Ok((_, file)) => Ok(file.id == Some(id.to_string())),
            Err(error) if is_not_found(&error) => Ok(false),
            Err(e) => Err(err_msg(format!("{:#?}", e))),
        }
    }

    #[allow(dead_code)]
    fn get_file_size(&self, drive_id: DriveIdRef, mime_type: Option<String>) -> u64 {
        self.get_file_content(drive_id, mime_type).unwrap().len() as u64
    }

    fn get_file_metadata(&self, id: DriveIdRef) -> Result<drive3::api::File, Error> {
        let rt = Runtime::new().unwrap();
        rt.block_on(
            self.hub()?
                .files()
                .get(id)
                .param("fields", "id,name,parents,mimeType,webViewLink")
                .add_scope(drive3::api::Scope::Full)
                .doit(),
        )
        .map(|(_response, file)| file)
        .map_err(|e| err_msg(format!("{:#?}", e)))
    }

    /// Builds the stand-in content served for a file Drive will not hand over,
    /// explaining why and pointing at the web UI where it can still be opened.
    fn unexportable_placeholder(&self, drive_id: &str, reason: &str) -> Vec<u8> {
        let link = self
            .get_file_metadata(drive_id)
            .ok()
            .and_then(|metadata| metadata.web_view_link)
            .unwrap_or_else(|| String::from("<unavailable>"));

        format!(
            "UNEXPORTABLE_FILE: This file could not be retrieved because {}. \
             It can be opened at: {}\n",
            reason.trim_end_matches('.'),
            link
        )
        .into_bytes()
    }

    /// Retrieves the content of a Drive file. If `mime_type` is specified, this method will
    /// attempt to export the file in some appropriate format rather than just download it as is.
    /// This is the only way of retrieving Docs, Sheets, Slides, Sites and Drawings.
    fn get_file_content(
        &self,
        drive_id: &str,
        mime_type: Option<String>,
    ) -> Result<Vec<u8>, Error> {
        if let Some(mime) = mime_type.clone()
            && UNEXPORTABLE_MIME_TYPES.contains::<str>(&mime)
        {
            return Ok(self.unexportable_placeholder(
                drive_id,
                &format!("its MIME type is {:?}, which Drive cannot export", mime),
            ));
        }

        let unexportable_mime = mime_type.clone().unwrap_or_default();
        let export_type: Option<&'static str> = mime_type
            .and_then(|ref t| MIME_TYPES.get::<str>(t))
            .cloned();

        let rt = Runtime::new().unwrap();

        let response = match export_type {
            Some(t) => {
                let result = rt.block_on(
                    self.hub()?
                        .files()
                        .export(drive_id, t)
                        .add_scope(drive3::api::Scope::Full)
                        .doit(),
                );

                match result {
                    Ok(response) => {
                        debug!("response: {:?}", response);
                        response
                    }
                    // These refusals are properties of the file itself, so retrying
                    // will never help. Describe the problem in the file content
                    // rather than failing the read with an error the user cannot
                    // act on. Anything else is treated as a genuine failure.
                    Err(ref e)
                        if error_reasons(e)
                            .iter()
                            .any(|reason| PERMANENT_EXPORT_FAILURES.contains(reason.as_str())) =>
                    {
                        warn!(
                            "Cannot export {}: {}",
                            drive_id,
                            error_messages(e).join("; ")
                        );
                        return Ok(self.unexportable_placeholder(
                            drive_id,
                            &format!(
                                "Drive refused to export it as {:?}: {}",
                                unexportable_mime,
                                error_messages(e).join("; ")
                            ),
                        ));
                    }
                    Err(e) => return Err(err_msg(format!("{:#?}", e))),
                }
            }
            None => {
                let (response, _empty_file) = rt
                    .block_on(
                        self.hub()?
                            .files()
                            .get(drive_id)
                            .supports_team_drives(false)
                            .param("alt", "media")
                            .add_scope(drive3::api::Scope::Full)
                            .doit(),
                    )
                    .map_err(|e| err_msg(format!("{:#?}", e)))?;
                response
            }
        };

        let content: Vec<u8> = rt
            .block_on(async { response.into_body().collect().await })?
            .to_bytes()
            .to_vec();
        Ok(content)
    }

    fn apply_pending_operations(
        operations: &[PendingOperation],
        data: &mut Vec<u8>,
    ) -> Result<(), Error> {
        for operation in operations {
            match operation {
                PendingOperation::Write {
                    offset,
                    data: write_data,
                } => {
                    let required_size = offset
                        .checked_add(write_data.len())
                        .ok_or_else(|| err_msg("Write offset overflow"))?;
                    Self::grow_data(data, required_size)?;
                    data[*offset..required_size].copy_from_slice(write_data);
                }
                PendingOperation::Truncate { size } => Self::resize_data(data, *size)?,
            }
        }
        Ok(())
    }

    fn grow_data(data: &mut Vec<u8>, size: usize) -> Result<(), Error> {
        if size > data.len() {
            data.try_reserve(size - data.len())
                .map_err(|error| err_msg(format!("Could not grow file buffer: {}", error)))?;
            data.resize(size, 0);
        }
        Ok(())
    }

    fn resize_data(data: &mut Vec<u8>, size: usize) -> Result<(), Error> {
        Self::grow_data(data, size)?;
        data.resize(size, 0);
        Ok(())
    }

    fn data_with_pending_operations(
        &self,
        id: DriveIdRef,
        mut data: Vec<u8>,
    ) -> Result<Vec<u8>, Error> {
        if let Some(operations) = self.pending_writes.get(id) {
            Self::apply_pending_operations(operations, &mut data)?;
        }
        Ok(data)
    }

    /// Validates that the current authentication is working.
    /// Makes a lightweight API call to verify the token is valid.
    /// Returns Ok(()) if valid, or an error describing the auth problem.
    pub fn validate_auth(&mut self) -> Result<(), Error> {
        self.root_id().map(|_| ())
    }

    /// Returns the Drive ID of the root "My Drive" directory
    pub fn root_id(&mut self) -> Result<&String, Error> {
        if self.root_id.is_none() {
            let rt = Runtime::new().unwrap();
            let parent = rt
                .block_on(
                    self.hub()?
                        .files()
                        .list()
                        .param("fields", "files(parents)")
                        .spaces("drive")
                        .corpora("user")
                        .page_size(1)
                        .q("'root' in parents")
                        .add_scope(drive3::api::Scope::Full)
                        .doit(),
                )
                .map_err(|e| err_msg(format!("{:#?}", e)))?
                .1
                .files
                .ok_or_else(|| err_msg("No files received"))?
                .into_iter()
                .take(1)
                .next()
                .ok_or_else(|| err_msg("No files on drive. Can't deduce drive id for 'My Drive'"))?
                .parents
                .ok_or_else(|| {
                    err_msg("Probed file has no parents. Can't deduce drive id for 'My Drive'")
                })?
                .into_iter()
                .take(1)
                .next()
                .ok_or_else(|| {
                    err_msg("No files on drive. Can't deduce drive id for 'My Drive'")
                })?;

            self.root_id = Some(parent);
        }

        Ok(self.root_id.as_ref().unwrap())
    }

    /// Returns the start page token for the `changes.list` API endpoint.
    fn get_start_page_token(&mut self) -> Result<String, Error> {
        let rt = Runtime::new().unwrap();
        let result = rt
            .block_on(
                self.hub()?
                    .changes()
                    .get_start_page_token()
                    .add_scope(drive3::api::Scope::Full)
                    .doit(),
            )
            .map_err(|e| err_msg(format!("{:#?}", e)))?;
        result
            .1
            .start_page_token
            .ok_or_else(|| err_msg("Received OK response from Drive without a startPageToken"))
    }

    /// Returns the current token for the `changes.list` API endpoint, or the start page token if
    /// absent.
    fn changes_token(&mut self) -> Result<&String, Error> {
        if self.changes_token.is_none() {
            self.changes_token = Some(self.get_start_page_token()?);
        }

        Ok(self.changes_token.as_ref().unwrap())
    }

    /// Returns a list of all changes reported by Drive which are more recent than the changes
    /// token indicates.
    pub fn get_all_changes(&mut self) -> Result<(Vec<drive3::api::Change>, String), Error> {
        let mut all_changes = Vec::new();
        let mut token = self.changes_token()?.clone();

        let rt = Runtime::new().unwrap();
        let fields = format!(
            "kind,newStartPageToken,changes(kind,type,time,removed,fileId,file({}))",
            LISTED_FILE_FIELDS.as_str()
        );

        loop {
            let (_response, changelist) = rt
                .block_on(
                    self.hub()?
                        .changes()
                        .list(&token)
                        .param("fields", &fields)
                        .spaces("drive")
                        .restrict_to_my_drive(true)
                        // Whether to include changes indicating that items have been removed from the list of changes, for example by deletion or loss of access. (Default: true)
                        .include_removed(true)
                        .supports_team_drives(false)
                        .include_team_drive_items(false)
                        .page_size(PAGE_SIZE)
                        .add_scope(drive3::api::Scope::Full)
                        .doit(),
                )
                .map_err(|e| err_msg(format!("{:#?}", e)))?;

            match changelist.changes {
                Some(changes) => all_changes.extend(changes),
                _ => warn!("Changelist does not contain any changes!"),
            };

            if let Some(next_page_token) = changelist.next_page_token {
                token = next_page_token;
            } else {
                let new_start_page_token = changelist.new_start_page_token.ok_or_else(|| {
                    err_msg("Drive returned a final changes page without a new start page token")
                })?;
                return Ok((all_changes, new_start_page_token));
            }
        }
    }

    /// Advances the Drive changes cursor after the caller applies the fetched batch locally.
    pub fn commit_changes_token(&mut self, token: String) {
        self.changes_token = Some(token);
    }

    /// Returns a list of all files from Drive. If the `parents` list is provided, only files which are children of any one of the list's elements are returned. If `trashed` is provided, only files which are trashed/not trashed are returned. The two filters can be used together.
    pub fn get_all_files(
        &mut self,
        parents: Option<Vec<DriveId>>,
        trashed: Option<bool>,
    ) -> Result<Vec<drive3::api::File>, Error> {
        let mut all_files = Vec::new();
        let mut page_token: Option<String> = None;
        let mut current_page = 1;
        let rt = Runtime::new().unwrap();
        let fields = format!("nextPageToken,files({})", LISTED_FILE_FIELDS.as_str());
        loop {
            let mut request = self
                .hub()?
                .files()
                .list()
                .param("fields", &fields)
                .spaces("drive") // TODO: maybe add photos as well
                .corpora("user")
                .page_size(PAGE_SIZE)
                .add_scope(drive3::api::Scope::Full);

            if let Some(token) = page_token {
                request = request.page_token(&token);
            };

            let mut query_chain: Vec<String> = Vec::new();
            if let Some(ref p) = parents {
                let q = p
                    .iter()
                    .map(|id| format!("'{}' in parents", id))
                    .collect::<Vec<_>>()
                    .join(" or ");

                query_chain.push(format!("({})", q));
            }
            if let Some(trash) = trashed {
                query_chain.push(format!("trashed = {}", trash));
            }

            let query = query_chain.join(" and ");
            let (_, filelist) = rt
                .block_on(request.q(&query).doit())
                .map_err(|e| err_msg(format!("{:#?}", e)))?;

            match filelist.files {
                Some(files) => {
                    info!(
                        "Received page {} containing {} files",
                        current_page,
                        files.len()
                    );
                    all_files.extend(files);
                }
                _ => warn!("Filelist does not contain any files!"),
            };

            current_page += 1;
            page_token = filelist.next_page_token;
            if page_token.is_none() {
                break;
            }
        }
        Ok(all_files)
    }

    /// Reads the contents of a Drive file starting at a certain offset.
    /// Prefers reading from cache if possible, otherwise fetches the content from Drive.
    ///
    /// The cache always holds the content as it is on Drive; pending operations are overlaid on
    /// the requested window only. A read therefore costs O(`size` + pending operations) rather
    /// than O(file size), which matters because the kernel splits sequential reads of a large
    /// file into many small requests.
    pub fn read(
        &mut self,
        drive_id: DriveIdRef,
        mime_type: Option<String>,
        offset: usize,
        size: usize,
    ) -> Option<&[u8]> {
        if self.cache.get(drive_id).is_none() {
            match self.get_file_content(drive_id, mime_type) {
                Ok(data) => {
                    self.cache.insert(drive_id.to_string(), data);
                }
                Err(e) => {
                    error!("Got error: {:?}", e);
                    return None;
                }
            }
        }

        let base = self.cache.peek(drive_id)?;
        let operations = self
            .pending_writes
            .get(drive_id)
            .map_or(&[][..], Vec::as_slice);
        self.buff = Self::read_window(base, operations, offset, size);
        Some(&self.buff)
    }

    /// Returns bytes `[offset, offset + size)` of the content obtained by applying `operations` to
    /// `base`, clamped to the resulting length. Only the requested window is materialized, so the
    /// cost does not depend on the size of `base`.
    fn read_window(
        base: &[u8],
        operations: &[PendingOperation],
        offset: usize,
        size: usize,
    ) -> Vec<u8> {
        let final_len = operations
            .iter()
            .fold(base.len(), |len, operation| match operation {
                PendingOperation::Write { offset, data } => {
                    cmp::max(len, offset.saturating_add(data.len()))
                }
                PendingOperation::Truncate { size } => *size,
            });
        let start = cmp::min(final_len, offset);
        let end = cmp::min(final_len, offset.saturating_add(size));

        // Bytes past the end of the base content start out as zeros, which is what growing the
        // file (sparse writes or extending truncates) fills them with.
        let mut window = vec![0; end - start];
        let base_end = cmp::min(base.len(), end);
        if start < base_end {
            window[..base_end - start].copy_from_slice(&base[start..base_end]);
        }

        for operation in operations {
            match operation {
                PendingOperation::Write {
                    offset: write_offset,
                    data,
                } => {
                    let write_end = write_offset.saturating_add(data.len());
                    let from = cmp::max(start, *write_offset);
                    let to = cmp::min(end, write_end);
                    if from < to {
                        window[from - start..to - start]
                            .copy_from_slice(&data[from - write_offset..to - write_offset]);
                    }
                }
                // Shrinking discards the tail; if the file later grows again, the regrown
                // region reads as zeros. Within the window both cases mean zeroing.
                PendingOperation::Truncate { size } => {
                    let from = cmp::max(start, *size);
                    if from < end {
                        window[from - start..].fill(0);
                    }
                }
            }
        }

        window
    }

    /// Creates a new file on Drive. If successful, returns the file id.
    pub fn create(&mut self, drive_file: &drive3::api::File) -> Result<DriveId, Error> {
        let dummy_file = DummyFile::new(&[]);
        let rt = Runtime::new().unwrap();
        let (_, file) = rt
            .block_on(
                self.hub()?
                    .files()
                    .create(drive_file.clone())
                    .use_content_as_indexable_text(true)
                    .supports_team_drives(false)
                    .ignore_default_visibility(true)
                    .param("fields", &format!("id,{}", Checksums::DRIVE_FIELDS))
                    .upload(dummy_file, "application/octet-stream".parse().unwrap()),
            )
            .map_err(|e| err_msg(format!("{:#?}", e)))?;
        let id = file
            .id
            .clone()
            .ok_or_else(|| err_msg("Received file from Drive without an id"))?;
        self.uploaded_checksums
            .insert(id.clone(), Checksums::of(&file));
        Ok(id)
    }

    /// Returns the checksums of the file content as it currently is on Drive, preferring those
    /// reported for this instance's latest upload over `listed` (the checksums from the file's
    /// listed metadata). Returns `None` while the file has pending operations, because its
    /// content as seen through GCSF then differs from what Drive has checksummed.
    pub fn checksums(&self, id: DriveIdRef, listed: Checksums) -> Option<Checksums> {
        if self.pending_writes.contains_key(id) {
            return None;
        }
        Some(self.uploaded_checksums.get(id).cloned().unwrap_or(listed))
    }

    /// Discards checksums recorded from uploads, once fresher metadata for the file has arrived
    /// from Drive.
    pub fn forget_uploaded_checksums(&mut self, id: DriveIdRef) {
        self.uploaded_checksums.remove(id);
    }

    #[cfg(test)]
    pub(crate) fn record_uploaded_checksums_for_testing(
        &mut self,
        id: DriveIdRef,
        checksums: Checksums,
    ) {
        self.uploaded_checksums.insert(id.to_string(), checksums);
    }

    /// Writes some data to a Drive file starting at a certain offset.
    /// This is a lazy operation. It creates a pending write which only gets executed when flush()
    /// is called.
    pub fn write(&mut self, id: DriveId, offset: usize, data: &[u8]) -> Result<(), Error> {
        offset
            .checked_add(data.len())
            .ok_or_else(|| err_msg("Write offset overflow"))?;
        let pending_write = PendingOperation::Write {
            offset,
            data: data.to_vec(),
        };

        self.pending_writes
            .entry(id)
            .or_insert_with(|| Vec::with_capacity(3000))
            .push(pending_write);
        Ok(())
    }

    /// Writes data and persists it before returning.
    pub fn write_and_flush(
        &mut self,
        id: DriveId,
        offset: usize,
        data: &[u8],
    ) -> Result<(), Error> {
        offset
            .checked_add(data.len())
            .ok_or_else(|| err_msg("Write offset overflow"))?;
        let operation_index = self.pending_writes.get(&id).map_or(0, Vec::len);
        self.write(id.clone(), offset, data)?;
        match self.flush_inner(&id) {
            Ok(()) => Ok(()),
            Err(failure) => {
                if !failure.outcome_indeterminate {
                    self.remove_pending_operation(&id, operation_index);
                }
                Err(failure.error)
            }
        }
    }

    /// Changes the file length when the pending operations are next flushed.
    pub fn truncate(&mut self, id: DriveId, size: usize) {
        self.pending_writes
            .entry(id)
            .or_insert_with(|| Vec::with_capacity(3000))
            .push(PendingOperation::Truncate { size });
    }

    /// Truncates a file and persists the new length before returning.
    pub fn truncate_and_flush(&mut self, id: DriveId, size: usize) -> Result<(), Error> {
        let operation_index = self.pending_writes.get(&id).map_or(0, Vec::len);
        self.truncate(id.clone(), size);
        match self.flush_inner(&id) {
            Ok(()) => Ok(()),
            Err(failure) => {
                if !failure.outcome_indeterminate {
                    self.remove_pending_operation(&id, operation_index);
                }
                Err(failure.error)
            }
        }
    }

    fn remove_pending_operation(&mut self, id: DriveIdRef, operation_index: usize) {
        if let Some(operations) = self.pending_writes.get_mut(id)
            && operations.len() > operation_index
        {
            operations.remove(operation_index);
            if operations.is_empty() {
                self.pending_writes.remove(id);
            }
        }
    }

    /// Deletes a file permanently from Drive.
    pub fn delete_permanently(&mut self, id: DriveIdRef) -> Result<bool, Error> {
        let rt = Runtime::new().unwrap();
        match rt.block_on(
            self.hub()?
                .files()
                .delete(id)
                .supports_team_drives(false)
                .add_scope(drive3::api::Scope::Full)
                .doit(),
        ) {
            Ok(response) => Ok(response.status().is_success()),
            Err(delete_error) => match self.contains(id) {
                Ok(false) => Ok(true),
                _ => Err(err_msg(format!("{:#?}", delete_error))),
            },
        }
    }

    /// `mv` operation. Can potentially move a file to a new directory and/or rename it.
    pub fn move_to(
        &mut self,
        id: DriveIdRef,
        parent: DriveIdRef,
        new_name: &str,
    ) -> Result<
        (
            Response<http_body_util::combinators::BoxBody<bytes::Bytes, hyper::Error>>,
            drive3::api::File,
        ),
        Error,
    > {
        let current_parents = self
            .get_file_metadata(id)?
            .parents
            .unwrap_or_else(|| vec![String::from("root")])
            .join(",");

        let f = drive3::api::File {
            name: Some(new_name.to_string()),
            ..Default::default()
        };
        let rt = Runtime::new().unwrap();
        rt.block_on(
            self.hub()?
                .files()
                .update(f, id)
                .remove_parents(&current_parents)
                .add_parents(parent)
                .add_scope(drive3::api::Scope::Full)
                .doit_without_upload(),
        )
        .map_err(|e| err_msg(format!("DriveFacade::move_to() {}", e)))
    }

    /// Marks a Google Drive file as trashed.
    pub fn move_to_trash(&mut self, id: DriveId) -> Result<(), Error> {
        let f = drive3::api::File {
            trashed: Some(true),
            ..Default::default()
        };

        let rt = Runtime::new().unwrap();
        rt.block_on(
            self.hub()?
                .files()
                .update(f, &id)
                .add_scope(drive3::api::Scope::Full)
                .doit_without_upload(),
        )
        .map(|_| ())
        .map_err(|e| err_msg(format!("DriveFacade::move_to_trash() {}", e)))
    }

    /// Renames a file and marks it as trashed in one Drive update.
    pub fn move_to_trash_with_name(&mut self, id: DriveIdRef, new_name: &str) -> Result<(), Error> {
        let file = drive3::api::File {
            name: Some(new_name.to_string()),
            trashed: Some(true),
            ..Default::default()
        };

        let rt = Runtime::new().unwrap();
        rt.block_on(
            self.hub()?
                .files()
                .update(file, id)
                .add_scope(drive3::api::Scope::Full)
                .doit_without_upload(),
        )
        .map(|_| ())
        .map_err(|error| err_msg(format!("DriveFacade::move_to_trash_with_name() {}", error)))
    }

    /// Applies pending write operations. Similar to flushing a stream.
    pub fn flush(&mut self, id: DriveIdRef) -> Result<(), Error> {
        self.flush_inner(id).map_err(|failure| failure.error)
    }

    fn flush_inner(&mut self, id: DriveIdRef) -> Result<(), FlushFailure> {
        if !self.pending_writes.contains_key(id) {
            debug!("flush({}): no pending writes", id);
            return Ok(());
        }
        if let Ok(false) = self.contains(id) {
            return Err(FlushFailure {
                error: err_msg(format!("flush({}): file doesn't exist on drive!", id)),
                outcome_indeterminate: false,
            });
        }

        // Pending writes are patches applied on top of the current content, so a
        // failure to fetch it must abort the flush. Defaulting to an empty buffer
        // here would upload the patches alone, discarding everything else the file
        // holds on Drive. The pending writes are left in place for a later retry.
        let file_data = self
            .get_file_content(id, None)
            .map_err(|error| FlushFailure {
                error: err_msg(format!(
                    "flush({}): refusing to overwrite, could not fetch current content: {}",
                    id, error
                )),
                outcome_indeterminate: false,
            })?;
        let file_data = self
            .data_with_pending_operations(id, file_data)
            .map_err(|error| FlushFailure {
                error,
                outcome_indeterminate: false,
            })?;
        let (_, uploaded) = self
            .update_file_content(DriveId::from(id), &file_data)
            .map_err(|error| match error {
                UpdateFileError::BeforeUpload(error) => FlushFailure {
                    error,
                    outcome_indeterminate: false,
                },
                UpdateFileError::Drive(error) => FlushFailure {
                    outcome_indeterminate: Self::upload_outcome_is_indeterminate(&error),
                    error: err_msg(format!("{:#?}", error)),
                },
            })?;
        self.pending_writes.remove(id);
        self.cache.insert(id.to_string(), file_data);
        self.uploaded_checksums
            .insert(id.to_string(), Checksums::of(&uploaded));

        Ok(())
    }

    fn upload_outcome_is_indeterminate(error: &drive3::Error) -> bool {
        match error {
            drive3::Error::HttpError(_)
            | drive3::Error::JsonDecodeError(_, _)
            | drive3::Error::Io(_) => true,
            drive3::Error::Failure(response) => response.status().is_server_error(),
            drive3::Error::UploadSizeLimitExceeded(_, _)
            | drive3::Error::BadRequest(_)
            | drive3::Error::MissingAPIKey
            | drive3::Error::MissingToken(_)
            | drive3::Error::Cancelled
            | drive3::Error::FieldClash(_) => false,
        }
    }

    /// Flushes every file with staged content, retaining failed operations for retry.
    pub fn flush_all(&mut self) -> Result<(), Error> {
        let ids: Vec<DriveId> = self.pending_writes.keys().cloned().collect();
        let mut failures = Vec::new();
        for id in ids {
            if let Err(error) = self.flush(&id) {
                failures.push(format!("{}: {}", id, error));
            }
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(err_msg(format!(
                "Could not flush all pending writes: {}",
                failures.join("; ")
            )))
        }
    }

    /// Discards cached and staged content after a file has been permanently deleted.
    pub fn discard_file_state(&mut self, id: DriveIdRef) {
        self.pending_writes.remove(id);
        self.cache.remove(id);
        self.uploaded_checksums.remove(id);
    }

    /// Updates the content of a file on Drive. The MIME type is guessed appropriately based on the
    /// content.
    fn update_file_content(
        &mut self,
        id: DriveId,
        data: &[u8],
    ) -> Result<
        (
            Response<http_body_util::combinators::BoxBody<bytes::Bytes, hyper::Error>>,
            drive3::api::File,
        ),
        UpdateFileError,
    > {
        let mime_guess = data.sniff_mime_type().unwrap_or("application/octet-stream");
        debug!(
            "Updating file content for {}. Mime type guess based on content: {}",
            id, mime_guess
        );

        let file = drive3::api::File {
            mime_type: Some(mime_guess.to_string()),
            ..Default::default()
        };

        let rt = Runtime::new().unwrap();
        let request = self
            .hub()
            .map_err(UpdateFileError::BeforeUpload)?
            .files()
            .update(file, &id)
            .param("fields", &format!("id,{}", Checksums::DRIVE_FIELDS))
            .add_scope(drive3::api::Scope::Full);
        let result = if data.is_empty() {
            rt.block_on(request.upload(DummyFile::new(data), mime_guess.parse().unwrap()))
        } else {
            rt.block_on(request.upload_resumable(DummyFile::new(data), mime_guess.parse().unwrap()))
        };
        result.map_err(|error| UpdateFileError::Drive(Box::new(error)))
    }

    /// Returns the size and capacity of the Drive account. In some cases, the limit can be absent.
    pub fn size_and_capacity(&mut self) -> Result<(u64, Option<u64>), Error> {
        let rt = Runtime::new().unwrap();
        let (_response, about) = rt
            .block_on(
                self.hub()?
                    .about()
                    .get()
                    .param("fields", "storageQuota")
                    .add_scope(drive3::api::Scope::Full)
                    .doit(),
            )
            .map_err(|e| err_msg(format!("{:#?}", e)))?;

        let storage_quota = about
            .storage_quota
            .ok_or_else(|| err_msg("size_and_capacity(): no storage quota in response"))?;

        let usage = u64::try_from(
            storage_quota
                .usage
                .ok_or_else(|| err_msg("size_and_capacity(): no usage in storage quota"))?,
        )?;
        let limit = storage_quota
            .limit
            .map(|s| u64::try_from(s).unwrap_or_default());

        Ok((usage, limit))
    }
}

/// A virtual (in-memory) file which implements the Read + Seek traits. Can be constructed from a
/// slice of bytes. Useful for uploading some file content to Drive without actually storing the
/// file locally on disk.
struct DummyFile {
    cursor: u64,
    data: Vec<u8>,
}

impl DummyFile {
    fn new(data: &[u8]) -> DummyFile {
        DummyFile {
            cursor: 0,
            data: Vec::from(data),
        }
    }
}

impl Seek for DummyFile {
    fn seek(&mut self, pos: SeekFrom) -> io::Result<u64> {
        let position: i64 = match pos {
            SeekFrom::Start(offset) => offset as i64,
            SeekFrom::End(offset) => self.data.len() as i64 - offset,
            SeekFrom::Current(offset) => self.cursor as i64 + offset,
        };

        if position < 0 {
            Err(io::Error::from(io::ErrorKind::InvalidInput))
        } else {
            self.cursor = position as u64;
            Ok(self.cursor)
        }
    }
}

impl Read for DummyFile {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let remaining = self.data.len() - self.cursor as usize;
        let copied = cmp::min(buf.len(), remaining);

        if copied > 0 {
            buf[..copied]
                .copy_from_slice(&self.data[self.cursor as usize..self.cursor as usize + copied]);
        }

        self.cursor += copied as u64;
        Ok(copied)
    }
}

#[cfg(test)]
mod tests {
    use super::{Checksums, DriveFacade, PendingOperation, is_not_found};

    fn write(offset: usize, data: &[u8]) -> PendingOperation {
        PendingOperation::Write {
            offset,
            data: data.to_vec(),
        }
    }

    #[test]
    fn recognizes_json_not_found_errors() {
        let error = drive3::Error::BadRequest(serde_json::json!({
            "error": { "code": 404 }
        }));
        assert!(is_not_found(&error));
    }

    #[test]
    fn partial_write_preserves_existing_suffix() {
        let mut data = b"abcdefghij".to_vec();
        DriveFacade::apply_pending_operations(&[write(2, b"XY")], &mut data).unwrap();
        assert_eq!(data, b"abXYefghij");
    }

    #[test]
    fn append_preserves_existing_content() {
        let mut data = b"first".to_vec();
        DriveFacade::apply_pending_operations(&[write(5, b" second")], &mut data).unwrap();
        assert_eq!(data, b"first second");
    }

    #[test]
    fn sparse_write_zero_fills_gap() {
        let mut data = b"abc".to_vec();
        DriveFacade::apply_pending_operations(&[write(5, b"z")], &mut data).unwrap();
        assert_eq!(data, b"abc\0\0z");
    }

    #[test]
    fn operations_apply_in_order() {
        let operations = [
            write(4, b"EF"),
            PendingOperation::Truncate { size: 3 },
            write(5, b"Z"),
        ];
        let mut data = b"abcdef".to_vec();
        DriveFacade::apply_pending_operations(&operations, &mut data).unwrap();
        assert_eq!(data, b"abc\0\0Z");
    }

    #[test]
    fn write_offset_overflow_is_rejected_without_mutating_data() {
        let mut data = b"unchanged".to_vec();
        let result = DriveFacade::apply_pending_operations(&[write(usize::MAX, b"x")], &mut data);
        assert!(result.is_err());
        assert_eq!(data, b"unchanged");
    }

    #[test]
    fn synchronous_write_overflow_does_not_create_pending_entry() {
        let mut facade = DriveFacade::new_for_testing();
        let result = facade.write_and_flush("file".to_string(), usize::MAX, b"x");
        assert!(result.is_err());
        assert!(!facade.pending_writes.contains_key("file"));
    }

    #[test]
    fn definitive_drive_errors_are_not_indeterminate() {
        let error = drive3::Error::BadRequest(serde_json::json!({
            "error": { "code": 400 }
        }));
        assert!(!DriveFacade::upload_outcome_is_indeterminate(&error));
    }

    #[test]
    fn preparing_data_does_not_clear_pending_operations() {
        let mut facade = DriveFacade::new_for_testing();
        facade.write("file".to_string(), 1, b"X").unwrap();

        let data = facade
            .data_with_pending_operations("file", b"abc".to_vec())
            .unwrap();

        assert_eq!(data, b"aXc");
        assert_eq!(facade.pending_writes["file"].len(), 1);
    }

    #[test]
    fn failed_flush_retains_pending_operations_for_retry() {
        let mut facade = DriveFacade::new_for_testing();
        facade.write("file".to_string(), 0, b"data").unwrap();

        assert!(facade.flush("file").is_err());
        assert_eq!(facade.pending_writes["file"].len(), 1);
    }

    #[test]
    fn failed_synchronous_truncate_rolls_back_before_upload() {
        let mut facade = DriveFacade::new_for_testing();
        facade.write("file".to_string(), 0, b"data").unwrap();

        assert!(facade.truncate_and_flush("file".to_string(), 0).is_err());
        assert_eq!(facade.pending_writes["file"], vec![write(0, b"data")]);
    }

    #[test]
    fn failed_synchronous_write_rolls_back_before_upload() {
        let mut facade = DriveFacade::new_for_testing();
        facade.write("file".to_string(), 0, b"old").unwrap();

        assert!(
            facade
                .write_and_flush("file".to_string(), 3, b"new")
                .is_err()
        );
        assert_eq!(facade.pending_writes["file"], vec![write(0, b"old")]);
    }

    #[test]
    fn oversized_truncate_is_rejected_without_mutating_data() {
        let mut data = b"unchanged".to_vec();
        let result = DriveFacade::apply_pending_operations(
            &[PendingOperation::Truncate { size: usize::MAX }],
            &mut data,
        );
        assert!(result.is_err());
        assert_eq!(data, b"unchanged");
    }

    #[test]
    fn reads_include_pending_operations() {
        let mut facade = DriveFacade::new_for_testing();
        facade.cache.insert("file".to_string(), b"abcdef".to_vec());
        facade.write("file".to_string(), 2, b"XY").unwrap();
        facade.truncate("file".to_string(), 5);

        assert_eq!(facade.read("file", None, 0, 20), Some(&b"abXYe"[..]));
    }

    #[test]
    fn reads_do_not_copy_or_modify_the_cached_content() {
        // Regression test: read() used to clone the whole cached file on every call, making each
        // small read O(file size) and sequential reads of large files quadratic. The cached
        // buffer must be served in place and must keep holding the content as it is on Drive.
        let mut facade = DriveFacade::new_for_testing();
        facade.cache.insert("file".to_string(), b"abcdef".to_vec());
        let cached_ptr = facade.cache.peek("file").unwrap().as_ptr();

        assert_eq!(facade.read("file", None, 1, 2), Some(&b"bc"[..]));
        assert_eq!(facade.read("file", None, 4, 10), Some(&b"ef"[..]));
        assert_eq!(facade.read("file", None, 10, 10), Some(&b""[..]));

        facade.write("file".to_string(), 0, b"XY").unwrap();
        assert_eq!(facade.read("file", None, 0, 3), Some(&b"XYc"[..]));

        let cached = facade.cache.peek("file").unwrap();
        assert_eq!(cached.as_ptr(), cached_ptr);
        assert_eq!(cached, b"abcdef");
    }

    #[test]
    fn read_window_matches_applying_operations_to_whole_file() {
        // Deterministic pseudo-random differential test against apply_pending_operations, which
        // is the reference for what the file content looks like after the pending operations.
        let mut seed: u64 = 0x9e37_79b9_7f4a_7c15;
        let mut next = |bound: usize| {
            seed = seed
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            ((seed >> 33) as usize) % bound
        };

        for _ in 0..2000 {
            let base: Vec<u8> = (0..next(24)).map(|i| b'a' + (i % 26) as u8).collect();
            let operations: Vec<PendingOperation> = (0..next(5))
                .map(|_| {
                    if next(3) == 0 {
                        PendingOperation::Truncate { size: next(32) }
                    } else {
                        let len = next(8);
                        write(next(32), &vec![b'A' + next(26) as u8; len])
                    }
                })
                .collect();
            let offset = next(40);
            let size = next(40);

            let mut expected = base.clone();
            DriveFacade::apply_pending_operations(&operations, &mut expected).unwrap();
            let start = expected.len().min(offset);
            let end = expected.len().min(offset + size);

            assert_eq!(
                DriveFacade::read_window(&base, &operations, offset, size),
                &expected[start..end],
                "base={:?} operations={:?} offset={} size={}",
                base,
                operations,
                offset,
                size
            );
        }
    }

    fn checksums(md5: &str) -> Checksums {
        Checksums {
            md5: Some(md5.to_string()),
            sha256: Some(format!("sha256-of-{}", md5)),
        }
    }

    #[test]
    fn checksums_come_from_listed_metadata_by_default() {
        let facade = DriveFacade::new_for_testing();
        assert_eq!(
            facade.checksums("file", checksums("listed")),
            Some(checksums("listed"))
        );
    }

    #[test]
    fn uploaded_checksums_supersede_listed_ones_until_forgotten() {
        let mut facade = DriveFacade::new_for_testing();
        facade
            .uploaded_checksums
            .insert("file".to_string(), checksums("uploaded"));
        assert_eq!(
            facade.checksums("file", checksums("listed")),
            Some(checksums("uploaded"))
        );

        facade.forget_uploaded_checksums("file");
        assert_eq!(
            facade.checksums("file", checksums("listed")),
            Some(checksums("listed"))
        );
    }

    #[test]
    fn uploads_without_reported_checksums_hide_listed_ones() {
        // The listed checksums describe content from before the upload, so they must not be
        // reported even if Drive did not return checksums for the upload itself.
        let mut facade = DriveFacade::new_for_testing();
        facade
            .uploaded_checksums
            .insert("file".to_string(), Checksums::default());
        assert_eq!(
            facade.checksums("file", checksums("listed")),
            Some(Checksums::default())
        );
    }

    #[test]
    fn no_checksums_while_operations_are_pending() {
        let mut facade = DriveFacade::new_for_testing();
        facade
            .uploaded_checksums
            .insert("file".to_string(), checksums("uploaded"));
        facade.write("file".to_string(), 0, b"new").unwrap();
        assert_eq!(facade.checksums("file", checksums("listed")), None);

        let mut facade = DriveFacade::new_for_testing();
        facade.truncate("file".to_string(), 0);
        assert_eq!(facade.checksums("file", checksums("listed")), None);
    }

    #[test]
    fn failed_flush_keeps_checksums_hidden() {
        let mut facade = DriveFacade::new_for_testing();
        facade.write("file".to_string(), 0, b"data").unwrap();
        assert!(facade.flush("file").is_err());
        assert_eq!(facade.checksums("file", checksums("listed")), None);
    }

    #[test]
    fn discarding_file_state_forgets_uploaded_checksums() {
        let mut facade = DriveFacade::new_for_testing();
        facade
            .uploaded_checksums
            .insert("file".to_string(), checksums("uploaded"));
        facade.discard_file_state("file");
        assert!(facade.uploaded_checksums.is_empty());
    }

    #[test]
    fn read_window_handles_extreme_offsets() {
        let base = b"abc";
        assert_eq!(
            DriveFacade::read_window(base, &[], usize::MAX, usize::MAX),
            b""
        );
        assert_eq!(DriveFacade::read_window(base, &[], 1, usize::MAX), b"bc");
    }
}
