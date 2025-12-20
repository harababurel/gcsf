//! Headless OAuth authentication for remote/SSH scenarios.
//!
//! This module provides authentication for users running GCSF on remote servers
//! where the browser is on a different machine. It supports both automatic localhost
//! redirect (when browser and GCSF are on the same machine) and manual URL paste
//! (when they're on different machines).

use failure::{err_msg, Error};
use serde::{Deserialize, Serialize};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::mpsc;
use std::thread;
use std::time::Duration;
use time::OffsetDateTime;
use url::Url;

/// Token structure compatible with yup_oauth2's storage format.
#[derive(Serialize)]
struct StoredTokenEntry {
    scopes: Vec<String>,
    token: StoredToken,
}

#[derive(Serialize)]
struct StoredToken {
    access_token: String,
    refresh_token: String,
    /// Time tuple: (year, day_of_year, hour, minute, second, nanosecond, 0, 0, 0)
    expires_at: (i32, u16, u8, u8, u8, u32, u8, u8, u8),
    id_token: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
    #[allow(dead_code)]
    token_type: String,
}

/// Performs headless OAuth login.
///
/// Starts a server on the specified port and also accepts pasted redirect URLs.
/// Returns the authorization code from whichever method succeeds first.
pub fn headless_login(
    client_id: &str,
    client_secret: &str,
    token_file: &Path,
    port: u16,
) -> Result<(), Error> {
    let redirect_uri = format!("http://127.0.0.1:{}", port);

    // Build the authorization URL
    let auth_url = format!(
        "https://accounts.google.com/o/oauth2/auth?\
         client_id={}&\
         redirect_uri={}&\
         response_type=code&\
         scope=https://www.googleapis.com/auth/drive&\
         access_type=offline&\
         prompt=consent",
        urlencoding::encode(client_id),
        urlencoding::encode(&redirect_uri)
    );

    println!("\n=== GCSF Authentication ===\n");
    println!("Please visit this URL to authorize GCSF:\n");
    println!("{}\n", auth_url);
    println!("After authorizing:");
    println!("  - If running locally: authentication completes automatically");
    println!("  - If on a remote server: copy the FULL URL from your browser");
    println!("    (it will show 'connection refused') and paste it below\n");

    // Get code via redirect server or manual paste
    let code = get_auth_code(port)?;

    // Exchange code for tokens
    let tokens = exchange_code_for_tokens(client_id, client_secret, &code, &redirect_uri)?;

    // Save tokens in yup_oauth2 format
    save_tokens(token_file, &tokens)?;

    Ok(())
}

/// Waits for auth code from either localhost redirect or stdin paste.
fn get_auth_code(port: u16) -> Result<String, Error> {
    let (tx, rx) = mpsc::channel::<Result<String, String>>();

    // Spawn thread to listen for HTTP redirect
    let tx_http = tx.clone();
    thread::spawn(move || {
        if let Ok(listener) = TcpListener::bind(format!("127.0.0.1:{}", port)) {
            listener.set_nonblocking(false).ok();
            if let Ok((mut stream, _)) = listener.accept() {
                let mut reader = BufReader::new(&stream);
                let mut request_line = String::new();
                if reader.read_line(&mut request_line).is_ok() {
                    // Parse: GET /?code=xxx&scope=... HTTP/1.1
                    if let Some(code) = extract_code_from_request(&request_line) {
                        // Send success response to browser
                        let response = "HTTP/1.1 200 OK\r\n\
                            Content-Type: text/html\r\n\r\n\
                            <html><body><h1>Success!</h1>\
                            <p>You can close this window and return to GCSF.</p>\
                            </body></html>";
                        stream.write_all(response.as_bytes()).ok();
                        tx_http.send(Ok(code)).ok();
                        return;
                    }
                }
            }
        }
        tx_http.send(Err("HTTP listener failed".to_string())).ok();
    });

    // Spawn thread to read from stdin
    let tx_stdin = tx;
    thread::spawn(move || {
        print!("Paste redirect URL here (or wait for automatic redirect): ");
        std::io::stdout().flush().ok();

        let stdin = std::io::stdin();
        for line in stdin.lock().lines().map_while(Result::ok) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }
            if let Some(code) = extract_code_from_url(trimmed) {
                tx_stdin.send(Ok(code)).ok();
                return;
            } else {
                println!("Could not find 'code' parameter in URL. Please try again.");
                print!("Paste redirect URL: ");
                std::io::stdout().flush().ok();
            }
        }
    });

    // Wait for either method (with timeout)
    match rx.recv_timeout(Duration::from_secs(300)) {
        Ok(Ok(code)) => {
            println!("\nAuthorization code received!");
            Ok(code)
        }
        Ok(Err(e)) => Err(err_msg(e)),
        Err(_) => Err(err_msg("Authentication timed out after 5 minutes")),
    }
}

/// Extract code from HTTP request line: "GET /?code=xxx&scope=... HTTP/1.1"
fn extract_code_from_request(request_line: &str) -> Option<String> {
    let parts: Vec<&str> = request_line.split_whitespace().collect();
    if parts.len() >= 2 {
        let path = parts[1];
        let full_url = format!("http://localhost{}", path);
        extract_code_from_url(&full_url)
    } else {
        None
    }
}

/// Extract code from full URL or path with query string
fn extract_code_from_url(url_str: &str) -> Option<String> {
    Url::parse(url_str).ok().and_then(|url| {
        url.query_pairs()
            .find(|(key, _)| key == "code")
            .map(|(_, value)| value.to_string())
    })
}

/// Exchange authorization code for access/refresh tokens
fn exchange_code_for_tokens(
    client_id: &str,
    client_secret: &str,
    code: &str,
    redirect_uri: &str,
) -> Result<TokenResponse, Error> {
    let client = reqwest::blocking::Client::new();

    let response = client
        .post("https://oauth2.googleapis.com/token")
        .form(&[
            ("client_id", client_id),
            ("client_secret", client_secret),
            ("code", code),
            ("grant_type", "authorization_code"),
            ("redirect_uri", redirect_uri),
        ])
        .send()
        .map_err(|e| err_msg(format!("Token request failed: {}", e)))?;

    if response.status().is_success() {
        response
            .json::<TokenResponse>()
            .map_err(|e| err_msg(format!("Failed to parse token response: {}", e)))
    } else {
        let error_text = response.text().unwrap_or_default();
        Err(err_msg(format!("Token exchange failed: {}", error_text)))
    }
}

/// Save tokens in yup_oauth2 compatible JSON format
fn save_tokens(path: &Path, tokens: &TokenResponse) -> Result<(), Error> {
    // Calculate expiration time
    let now = OffsetDateTime::now_utc();
    let expires_in_secs = tokens.expires_in.unwrap_or(3600) as i64;
    let expires_at = now + time::Duration::seconds(expires_in_secs);

    let entry = StoredTokenEntry {
        scopes: vec!["https://www.googleapis.com/auth/drive".to_string()],
        token: StoredToken {
            access_token: tokens.access_token.clone(),
            refresh_token: tokens.refresh_token.clone().unwrap_or_default(),
            expires_at: (
                expires_at.year(),
                expires_at.ordinal(),
                expires_at.hour(),
                expires_at.minute(),
                expires_at.second(),
                expires_at.nanosecond(),
                0,
                0,
                0,
            ),
            id_token: None,
        },
    };

    // yup_oauth2 expects an array of token entries
    let json = serde_json::to_string(&vec![entry])
        .map_err(|e| err_msg(format!("Failed to serialize tokens: {}", e)))?;

    std::fs::write(path, json)
        .map_err(|e| err_msg(format!("Failed to write token file: {}", e)))?;

    Ok(())
}
