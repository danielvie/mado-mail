use crate::message::{MessageBody, raster_image};
use crate::labels::{Catalog, Label};
use crate::triage::{Action, Outcome, Work};
use anyhow::{Context, Result, bail, ensure};
use base64::{
    Engine as _,
    engine::general_purpose::{URL_SAFE, URL_SAFE_NO_PAD},
};
use oauth2::{
    AuthUrl, AuthorizationCode, ClientId, ClientSecret, CsrfToken, EndpointNotSet, EndpointSet,
    PkceCodeChallenge, PkceCodeVerifier, RedirectUrl, RefreshToken, RequestTokenError, Scope,
    TokenResponse, TokenUrl,
    basic::{BasicClient, BasicErrorResponseType, BasicTokenResponse},
    url::Url,
};
use reqwest::blocking::Client;
use serde::Deserialize;
use std::{
    collections::BTreeSet,
    fs,
    io::{BufRead, BufReader, Read, Write},
    net::TcpListener,
    sync::{Arc, Mutex},
    thread,
    time::{Duration, Instant},
};

const GMAIL_MESSAGES: &str = "https://gmail.googleapis.com/gmail/v1/users/me/messages";
const GMAIL_LABELS: &str = "https://gmail.googleapis.com/gmail/v1/users/me/labels";
const MODIFY_SCOPE: &str = "https://www.googleapis.com/auth/gmail.modify";
const CREDENTIAL_SERVICE: &str = "mado-mail/gmail.modify";
type OAuthClient =
    BasicClient<EndpointSet, EndpointNotSet, EndpointNotSet, EndpointNotSet, EndpointSet>;

#[derive(Deserialize)]
struct ClientFile {
    installed: Credentials,
}

#[derive(Deserialize)]
struct Credentials {
    client_id: String,
    client_secret: String,
}

impl Credentials {
    fn load() -> Result<Self> {
        let path = crate::settings::directory()?.join("client_secret.json");
        let bytes = fs::read(&path).with_context(|| {
            format!(
                "Save your Google Desktop OAuth client JSON at {}. Enable the Gmail API and add your account as a test user if the consent app is in Testing.",
                path.display()
            )
        })?;
        let credentials: ClientFile = serde_json::from_slice(&bytes)
            .context("Expected a Google Desktop OAuth client file with an 'installed' section")?;
        ensure!(
            !credentials.installed.client_id.trim().is_empty()
                && !credentials.installed.client_secret.trim().is_empty(),
            "Desktop OAuth client ID and secret must not be empty"
        );

        Ok(credentials.installed)
    }

    fn client(&self) -> Result<OAuthClient> {
        Ok(BasicClient::new(ClientId::new(self.client_id.clone()))
            .set_client_secret(ClientSecret::new(self.client_secret.clone()))
            .set_auth_uri(AuthUrl::new(
                "https://accounts.google.com/o/oauth2/v2/auth".into(),
            )?)
            .set_token_uri(TokenUrl::new("https://oauth2.googleapis.com/token".into())?))
    }
}

fn credential_entry(user: &str) -> Result<keyring::Entry> {
    keyring::Entry::new(CREDENTIAL_SERVICE, user)
        .context("Could not open the operating system credential store (Keychain on macOS, Credential Manager on Windows)")
}

fn http_client() -> Result<Client> {
    Ok(Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()?)
}

pub struct SignIn {
    pub url: Url,
    listener: TcpListener,
    client: OAuthClient,
    credential_user: String,
    state: CsrfToken,
    verifier: PkceCodeVerifier,
}

impl SignIn {
    pub fn prepare() -> Result<Self> {
        let credentials = Credentials::load()?;
        let listener = TcpListener::bind("127.0.0.1:0")?;
        listener.set_nonblocking(true)?;
        let redirect = format!(
            "http://127.0.0.1:{}/oauth/callback",
            listener.local_addr()?.port()
        );
        let client = credentials
            .client()?
            .set_redirect_uri(RedirectUrl::new(redirect)?);
        let (challenge, verifier) = PkceCodeChallenge::new_random_sha256();
        let (url, state) = client
            .authorize_url(CsrfToken::new_random)
            .add_scope(Scope::new(MODIFY_SCOPE.into()))
            .add_extra_param("access_type", "offline")
            .add_extra_param("prompt", "consent")
            .set_pkce_challenge(challenge)
            .url();
        Ok(Self {
            url,
            listener,
            client,
            credential_user: credentials.client_id,
            state,
            verifier,
        })
    }

    pub fn finish(self) -> Result<Session> {
        let deadline = Instant::now() + Duration::from_secs(300);
        let code = loop {
            ensure!(
                Instant::now() < deadline,
                "Sign-in timed out. Click Connect Gmail to retry."
            );
            let (mut stream, _) = match self.listener.accept() {
                Ok(connection) => connection,
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    thread::sleep(Duration::from_millis(50));
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            stream.set_read_timeout(Some(Duration::from_secs(2)))?;
            stream.set_write_timeout(Some(Duration::from_secs(2)))?;
            let mut line = String::new();
            // Bound untrusted localhost requests; only the request line is needed.
            if BufReader::new((&mut stream).take(8192))
                .read_line(&mut line)
                .is_err()
            {
                continue;
            }
            let result = callback_code(&line, &self.state);
            let (status, message) = match &result {
                Ok(Some(_)) => ("200 OK", "Sign-in received. Return to Mado Mail."),
                Ok(None) => ("404 Not Found", "Not an OAuth callback."),
                Err(_) => (
                    "400 Bad Request",
                    "Sign-in failed. Return to Mado Mail and retry.",
                ),
            };
            let response = format!(
                "HTTP/1.1 {status}\r\nContent-Type: text/plain; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{message}",
                message.len()
            );
            let _ = stream.write_all(response.as_bytes());
            if let Some(code) = result? {
                break code;
            }
        };
        let http = http_client()?;
        let token = self
            .client
            .exchange_code(code)
            .set_pkce_verifier(self.verifier)
            .request(&http)
            .context("Google token exchange failed")?;
        let refresh_token = token
            .refresh_token()
            .context("Google did not issue a refresh token. Reconnect and grant offline access")?
            .clone();
        let mut session =
            Session::with_refresh_token(http, self.client, self.credential_user, refresh_token);
        session.accept_token(token)?;
        Ok(session)
    }
}

fn callback_code(line: &str, expected_state: &CsrfToken) -> Result<Option<AuthorizationCode>> {
    let mut words = line.split_whitespace();
    ensure!(words.next() == Some("GET"), "Expected a GET callback");
    let target = words.next().context("Missing callback path")?;
    ensure!(target.starts_with('/'), "Invalid callback path");
    let url = Url::parse(&format!("http://127.0.0.1{target}"))?;
    if url.path() != "/oauth/callback" {
        return Ok(None);
    }
    let params: Vec<_> = url.query_pairs().collect();
    let states: Vec<_> = params.iter().filter(|(key, _)| key == "state").collect();
    ensure!(
        states.len() == 1 && CsrfToken::new(states[0].1.to_string()) == *expected_state,
        "OAuth state mismatch; no token was requested"
    );
    if params.iter().any(|(key, _)| key == "error") {
        bail!("Google sign-in was denied or cancelled");
    }
    let codes: Vec<_> = params.iter().filter(|(key, _)| key == "code").collect();
    ensure!(
        codes.len() == 1 && !codes[0].1.is_empty(),
        "Missing or ambiguous authorization code"
    );
    Ok(Some(AuthorizationCode::new(codes[0].1.to_string())))
}

#[derive(Clone)]
pub struct Session {
    http: Client,
    client: OAuthClient,
    credential_user: String,
    refresh_token: RefreshToken,
    token: String,
    expires_at: Instant,
    revoked: bool,
    labels: Catalog,
}

pub type SharedSession = Arc<Mutex<Session>>;

#[derive(Clone, Debug)]
pub struct Email {
    pub id: String,
    pub from: String,
    pub subject: String,
    pub date: String,
    pub snippet: String,
    pub label_ids: Vec<String>,
}

#[derive(Deserialize)]
struct Inbox {
    #[serde(default)]
    messages: Vec<MessageId>,
}

#[derive(Deserialize)]
struct MessageId {
    id: String,
}

#[derive(Deserialize)]
struct Message {
    id: String,
    #[serde(default, rename = "labelIds")]
    label_ids: Vec<String>,
    #[serde(default)]
    snippet: String,
    payload: Payload,
}

#[derive(Deserialize)]
struct Payload {
    #[serde(default)]
    headers: Vec<Header>,
    #[serde(default, rename = "mimeType")]
    mime_type: String,
    #[serde(default)]
    filename: String,
    #[serde(default)]
    body: PartBody,
    #[serde(default)]
    parts: Vec<Payload>,
}

#[derive(Default, Deserialize)]
struct PartBody {
    #[serde(default)]
    data: String,
    #[serde(default, rename = "attachmentId")]
    attachment_id: String,
}

impl Payload {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|header| header.name.eq_ignore_ascii_case(name))
            .map(|header| header.value.as_str())
    }

    fn is_attachment(&self) -> bool {
        self.header("Content-Disposition").is_some_and(|value| {
            value
                .split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .eq_ignore_ascii_case("attachment")
        }) || self.mime_type.eq_ignore_ascii_case("message/rfc822")
    }

    fn body_part(&self, mime: &str) -> Option<&Self> {
        if self.is_attachment() || !self.filename.is_empty() {
            return None;
        }
        if self.mime_type.eq_ignore_ascii_case(mime) {
            return Some(self);
        }
        self.parts.iter().find_map(|part| part.body_part(mime))
    }

    fn inline_parts<'a>(&'a self, parts: &mut Vec<&'a Self>) {
        if self.is_attachment() {
            return;
        }
        if raster_image(&self.mime_type) && self.header("Content-ID").is_some() {
            parts.push(self);
        }
        for part in &self.parts {
            part.inline_parts(parts);
        }
    }
}

fn decode_part(data: &str) -> Result<Vec<u8>> {
    URL_SAFE_NO_PAD
        .decode(data)
        .or_else(|_| URL_SAFE.decode(data))
        .context("Invalid Gmail base64url body")
}

fn decode_text(part: &Payload, bytes: &[u8]) -> Result<String> {
    let charset = part
        .header("Content-Type")
        .unwrap_or_default()
        .split(';')
        .filter_map(|parameter| parameter.trim().split_once('='))
        .find(|(name, _)| name.trim().eq_ignore_ascii_case("charset"))
        .map(|(_, value)| value.trim().trim_matches(['\"', '\'']))
        .unwrap_or("utf-8");
    let encoding = encoding_rs::Encoding::for_label(charset.as_bytes())
        .context("Unsupported message character encoding")?;
    let (text, _, invalid) = encoding.decode(bytes);
    ensure!(
        !invalid,
        "Message contains invalid text for its character encoding"
    );
    Ok(text.into_owned())
}

#[derive(Deserialize)]
struct Header {
    name: String,
    value: String,
}

impl Message {
    fn into_email(self) -> Email {
        let header = |name: &str, fallback: &str| {
            self.payload
                .headers
                .iter()
                .find(|header| header.name.eq_ignore_ascii_case(name))
                .map(|header| header.value.clone())
                .unwrap_or_else(|| fallback.into())
        };
        Email {
            from: header("From", "Unknown sender"),
            subject: header("Subject", "(No subject)"),
            date: header("Date", ""),
            id: self.id,
            snippet: self.snippet,
            label_ids: self.label_ids,
        }
    }
}

impl Session {
    fn with_refresh_token(
        http: Client,
        client: OAuthClient,
        credential_user: String,
        refresh_token: RefreshToken,
    ) -> Self {
        Self {
            http,
            client,
            credential_user,
            refresh_token,
            token: String::new(),
            expires_at: Instant::now(),
            revoked: false,
            labels: Catalog::default(),
        }
    }

    pub fn restore() -> Result<Option<Self>> {
        let credentials = Credentials::load()?;
        let refresh_token = match credential_entry(&credentials.client_id)?.get_password() {
            Ok(secret) => RefreshToken::new(secret),
            Err(keyring::Error::NoEntry) => return Ok(None),
            Err(error) => return Err(error).context("Could not read saved Google authorization"),
        };
        let mut session = Self::with_refresh_token(
            http_client()?,
            credentials.client()?,
            credentials.client_id,
            refresh_token,
        );
        Ok(session.refresh()?.then_some(session))
    }

    fn accept_token(&mut self, token: BasicTokenResponse) -> Result<()> {
        if let Some(refresh_token) = token.refresh_token() {
            credential_entry(&self.credential_user)?
                .set_password(refresh_token.secret())
                .context("Could not save Google authorization in the operating system credential store")?;
            self.refresh_token = refresh_token.clone();
        }
        self.token = token.access_token().secret().clone();
        self.expires_at = Instant::now() + token.expires_in().unwrap_or_default();
        Ok(())
    }

    fn refresh(&mut self) -> Result<bool> {
        // A queued reader job must not delete a newer grant after the user reconnects.
        if self.revoked {
            return Ok(false);
        }
        match self
            .client
            .exchange_refresh_token(&self.refresh_token)
            .request(&self.http)
        {
            Ok(token) => {
                self.accept_token(token)?;
                Ok(true)
            }
            Err(RequestTokenError::ServerResponse(error))
                if error.error() == &BasicErrorResponseType::InvalidGrant =>
            {
                self.revoked = true;
                match credential_entry(&self.credential_user)?.delete_credential() {
                    Ok(()) | Err(keyring::Error::NoEntry) => {}
                    Err(error) => {
                        return Err(error).context("Could not remove revoked Google authorization");
                    }
                }
                Ok(false)
            }
            Err(error) => {
                Err(error).context("Could not refresh Google authorization; saved sign-in was kept")
            }
        }
    }

    pub fn submit(&mut self, work: &[Work], recheck: bool) -> Vec<(Work, Outcome)> {
        // ponytail: per-message outcomes cost up to 400 serial requests; batch only after equivalent partial-failure checks exist.
        work.iter()
            .map(|w| {
                let result = self.authorized(|session| {
                    if recheck {
                        session.recheck_url(Self::message_url(&w.id)?, w.action, &w.label_ids)
                    } else {
                        session.write_url(Self::message_url(&w.id)?, w.action, &w.label_ids)
                    }
                });
                let outcome = match result {
                    Ok(Some(outcome)) => outcome,
                    Ok(None) => {
                        if recheck {
                            Outcome::Unknown("Reconnect Gmail, then recheck.".into())
                        } else {
                            Outcome::Failed("Reconnect Gmail before retrying.".into())
                        }
                    }
                    Err(_) => {
                        if recheck {
                            Outcome::Unknown("Could not recheck Gmail. No retry was sent.".into())
                        } else {
                            Outcome::Failed(
                                "Authorization failed before submission. Retry after reconnecting."
                                    .into(),
                            )
                        }
                    }
                };
                (w.clone(), outcome)
            })
            .collect()
    }

    pub fn remove_label(&mut self, id: &str, label: &str) -> Result<Option<Outcome>> {
        self.authorized(|session| session.remove_label_url(Self::message_url(id)?, label))
    }

    fn remove_label_url(&self, mut url: Url, label: &str) -> Result<Outcome> {
        crate::spaces::validate_labels(&[label.to_owned()])?;
        url.path_segments_mut().expect("Gmail URL").push("modify");
        self.post_url(url, serde_json::json!({"removeLabelIds":[label]}))
    }

    pub fn add_label(&mut self, id: &str, label: &str) -> Result<Option<Outcome>> {
        self.authorized(|session| session.add_label_url(Self::message_url(id)?, label))
    }

    fn add_label_url(&self, mut url: Url, label: &str) -> Result<Outcome> {
        crate::spaces::validate_labels(&[label.to_owned()])?;
        url.path_segments_mut().expect("Gmail URL").push("modify");
        self.post_url(url, serde_json::json!({"addLabelIds":[label]}))
    }

    pub fn set_read(&mut self, id: &str, read: bool) -> Result<Option<Outcome>> {
        self.authorized(|session| session.set_read_url(Self::message_url(id)?, read))
    }

    fn set_read_url(&self, mut url: Url, read: bool) -> Result<Outcome> {
        url.path_segments_mut().expect("Gmail URL").push("modify");
        self.post_url(url, if read { serde_json::json!({"removeLabelIds":["UNREAD"]}) }
            else { serde_json::json!({"addLabelIds":["UNREAD"]}) })
    }

    fn write_url(&self, mut url: Url, action: Action, labels: &[String]) -> Result<Outcome> {
        if matches!(action, Action::Space(_)) { crate::spaces::validate_labels(labels)?; }
        url.path_segments_mut()
            .expect("Gmail URL")
            .push(match action {
                Action::Archive | Action::Space(_) => "modify",
                Action::Trash => "trash",
            });
        let payload = match action {
            Action::Archive => serde_json::json!({"removeLabelIds":["INBOX"]}),
            Action::Trash => serde_json::json!({}),
            Action::Space(_) => serde_json::json!({"addLabelIds":labels,"removeLabelIds":["INBOX"]}),
        };
        self.post_url(url, payload)
    }

    fn post_url(&self, url: Url, payload: serde_json::Value) -> Result<Outcome> {
        // Never replay a timed-out POST. Label reads reconcile it before the user can retry.
        let response = match self
            .http
            .post(url)
            .bearer_auth(&self.token)
            .json(&payload)
            .send()
        {
            Ok(r) => r,
            Err(_) => {
                return Ok(Outcome::Unknown(
                    "Connection lost during submission. Recheck Gmail.".into(),
                ));
            }
        };
        let status = response.status();
        if status == reqwest::StatusCode::UNAUTHORIZED {
            response.error_for_status()?;
            unreachable!();
        }
        if status.is_success() {
            return Ok(Outcome::Confirmed);
        }
        if status.is_client_error() && status != reqwest::StatusCode::REQUEST_TIMEOUT {
            return Ok(Outcome::Failed(format!(
                "Gmail rejected this action: HTTP {}. Nothing confirmed.",
                status.as_u16()
            )));
        }
        Ok(Outcome::Unknown(format!(
            "Gmail returned HTTP {}. Recheck before retrying.",
            status.as_u16()
        )))
    }

    fn recheck_url(&self, url: Url, action: Action, labels: &[String]) -> Result<Outcome> {
        if matches!(action, Action::Space(_)) { crate::spaces::validate_labels(labels)?; }
        #[derive(Deserialize)]
        struct Labels {
            #[serde(rename = "labelIds")]
            labels: Vec<String>,
        }
        let state: Labels =
            self.bounded_json(url, &[("format", "minimal"), ("fields", "labelIds")])?;
        let done = match action {
            Action::Archive => !state.labels.iter().any(|s| s == "INBOX"),
            Action::Trash => state.labels.iter().any(|s| s == "TRASH"),
            Action::Space(_) => !state.labels.iter().any(|s| s == "INBOX")
                && labels.iter().all(|label| state.labels.contains(label)),
        };
        Ok(if done {
            Outcome::Confirmed
        } else {
            Outcome::Failed("Gmail has not reached the requested state. Retry or put back.".into())
        })
    }

    pub fn inbox(&mut self) -> Result<Option<Vec<Email>>> {
        self.authorized(Self::fetch_inbox)
    }

    pub fn label_catalog(&mut self) -> Catalog {
        self.label_catalog_at(Url::parse(GMAIL_LABELS).unwrap())
    }

    fn label_catalog_at(&mut self, url: Url) -> Catalog {
        #[derive(Deserialize)]
        struct List { #[serde(default)] labels: Vec<Label> }
        match self.authorized(|s| s.bounded_json::<List>(url.clone(), &[])) {
            Ok(Some(list)) => {
                let previous = &self.labels.entries;
                self.labels.entries = list.labels.into_iter().map(|mut label| {
                    label.color = previous.get(&label.id).and_then(|old| old.color.clone());
                    (label.id.clone(), label)
                }).collect();
                self.labels.unavailable = false;
            }
            _ => self.labels.unavailable = true,
        }
        self.labels.clone()
    }

    pub fn label_colors(&mut self, ids: &[String]) -> Catalog {
        self.label_colors_at(ids, Url::parse(GMAIL_LABELS).unwrap())
    }

    fn label_colors_at(&mut self, ids: &[String], base: Url) -> Catalog {
        // ponytail: one bounded request at a time, per used label, never per message. Use a small pool if measured slow.
        for id in ids.iter().collect::<BTreeSet<_>>() {
            if !self.labels.entries.get(id).is_some_and(|l| l.kind == "user") { continue; }
            let mut url = base.clone();
            url.path_segments_mut().expect("Gmail label URL").push(id);
            match self.authorized(|s| s.bounded_json::<Label>(url.clone(), &[])) {
                Ok(Some(label)) if label.id == *id => { self.labels.entries.insert(id.clone(), label); }
                Ok(None) => { self.labels.unavailable = true; break; }
                _ => self.labels.unavailable = true,
            }
        }
        self.labels.clone()
    }

    pub fn message_body(&mut self, id: &str) -> Result<Option<MessageBody>> {
        self.authorized(|session| session.fetch_body(id))
    }

    fn authorized<T>(&mut self, fetch: impl Fn(&Self) -> Result<T>) -> Result<Option<T>> {
        if self.revoked {
            return Ok(None);
        }
        if self.expires_at.saturating_duration_since(Instant::now()) <= Duration::from_secs(30)
            && !self.refresh()?
        {
            return Ok(None);
        }
        let result = fetch(self);
        if result
            .as_ref()
            .err()
            .and_then(|error| error.downcast_ref::<reqwest::Error>())
            .and_then(|error| error.status())
            == Some(reqwest::StatusCode::UNAUTHORIZED)
        {
            if !self.refresh()? {
                return Ok(None);
            }
            return fetch(self).map(Some);
        }
        result.map(Some)
    }

    fn message_url(id: &str) -> Result<Url> {
        let mut url = Url::parse(GMAIL_MESSAGES)?;
        url.path_segments_mut()
            .expect("Gmail URL has path segments")
            .push(id);
        Ok(url)
    }

    fn part_bytes(&self, message_url: &Url, part: &Payload) -> Result<Vec<u8>> {
        let data = if part.body.data.is_empty() && !part.body.attachment_id.is_empty() {
            let mut url = message_url.clone();
            url.path_segments_mut()
                .expect("Message URL has path segments")
                .push("attachments")
                .push(&part.body.attachment_id);
            let body: PartBody = self.bounded_json(url, &[])?;
            body.data
        } else {
            part.body.data.clone()
        };
        let bytes = decode_part(&data)?;
        ensure!(
            bytes.len() <= 4 * 1024 * 1024,
            "Message part exceeds the 4 MiB limit"
        );
        Ok(bytes)
    }

    fn bounded_json<T: serde::de::DeserializeOwned>(
        &self,
        url: Url,
        query: &[(&str, &str)],
    ) -> Result<T> {
        const LIMIT: u64 = 24 * 1024 * 1024;
        let response = self
            .http
            .get(url)
            .bearer_auth(&self.token)
            .query(query)
            .send()?
            .error_for_status()?;
        let mut bytes = Vec::new();
        response.take(LIMIT + 1).read_to_end(&mut bytes)?;
        ensure!(
            bytes.len() as u64 <= LIMIT,
            "Message response exceeds the 24 MiB limit"
        );
        Ok(serde_json::from_slice(&bytes)?)
    }

    fn fetch_body(&self, id: &str) -> Result<MessageBody> {
        self.fetch_body_url(Self::message_url(id)?)
    }

    fn fetch_body_url(&self, url: Url) -> Result<MessageBody> {
        let message: Message = self.bounded_json(url.clone(), &[("format", "full")])?;
        let part = message
            .payload
            .body_part("text/html")
            .or_else(|| message.payload.body_part("text/plain"))
            .context("No supported HTML or plain-text body in this message")?;
        let bytes = self.part_bytes(&url, part)?;
        let text = decode_text(part, &bytes)?;
        let mut body = if part.mime_type.eq_ignore_ascii_case("text/html") {
            MessageBody::html(text)
        } else {
            MessageBody::plain(&text)
        };
        let mut total = bytes.len();
        let mut parts = Vec::new();
        message.payload.inline_parts(&mut parts);
        if parts.len() > 32 {
            body.warnings
                .push("Some embedded images exceeded the 32-image limit.".into());
        }
        for part in parts.into_iter().take(32) {
            let cid = part
                .header("Content-ID")
                .unwrap()
                .trim()
                .trim_matches(['<', '>']);
            // ponytail: embedded raster parts are capped at 12 MiB total; larger mail needs streamed image serving.
            match self.part_bytes(&url, part) {
                Ok(bytes) if total + bytes.len() <= 12 * 1024 * 1024 => {
                    total += bytes.len();
                    body.add_image(cid, &part.mime_type, &bytes);
                }
                Ok(_) => {
                    body.warnings
                        .push("Some embedded images exceeded the 12 MiB total limit.".into());
                    break;
                }
                Err(error) => {
                    if error
                        .downcast_ref::<reqwest::Error>()
                        .and_then(|error| error.status())
                        == Some(reqwest::StatusCode::UNAUTHORIZED)
                    {
                        return Err(error);
                    }
                    body.warnings
                        .push("An embedded image could not be loaded.".into());
                }
            }
        }
        Ok(body)
    }

    fn fetch_inbox(&self) -> Result<Vec<Email>> {
        let inbox: Inbox = self
            .http
            .get(GMAIL_MESSAGES)
            .bearer_auth(&self.token)
            .query(&[("labelIds", "INBOX"), ("maxResults", "400")])
            .send()?
            .error_for_status()?
            .json()?;
        let mut emails = Vec::with_capacity(inbox.messages.len());
        // ponytail: four blocking requests per chunk; use an async pool if adding larger or concurrent workloads.
        for chunk in inbox.messages.chunks(4) {
            let batch = thread::scope(|scope| {
                let jobs: Vec<_> = chunk
                    .iter()
                    .map(|message| {
                        scope.spawn(|| {
                            let mut url = Url::parse(GMAIL_MESSAGES)?;
                            url.path_segments_mut()
                                .expect("Gmail URL has path segments")
                                .push(&message.id);
                            let message: Message = self
                                .http
                                .get(url)
                                .bearer_auth(&self.token)
                                .query(&[
                                    ("format", "metadata"),
                                    ("metadataHeaders", "From"),
                                    ("metadataHeaders", "Subject"),
                                    ("metadataHeaders", "Date"),
                                ])
                                .send()?
                                .error_for_status()?
                                .json()?;
                            Ok(message.into_email())
                        })
                    })
                    .collect();
                jobs.into_iter()
                    .map(|job| {
                        job.join()
                            .map_err(|_| anyhow::anyhow!("Message fetch worker failed"))?
                    })
                    .collect::<Result<Vec<_>>>()
            })?;
            emails.extend(batch);
        }
        Ok(emails)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_checks_state_and_decodes_code() {
        let state = CsrfToken::new("expected".into());
        let code = callback_code(
            "GET /oauth/callback?state=expected&code=abc%2B123 HTTP/1.1",
            &state,
        )
        .unwrap()
        .unwrap();
        assert_eq!(code.secret(), "abc+123");
        for query in [
            "state=wrong&code=abc",
            "code=abc",
            "state=expected",
            "state=expected&code=",
            "state=expected&state=wrong&code=abc",
            "state=expected&code=a&code=b",
        ] {
            assert!(
                callback_code(&format!("GET /oauth/callback?{query} HTTP/1.1"), &state).is_err()
            );
        }
        assert!(
            callback_code(
                "GET /oauth/callback?state=expected&error=access_denied HTTP/1.1",
                &state
            )
            .is_err()
        );
        assert!(
            callback_code("GET /favicon.ico HTTP/1.1", &state)
                .unwrap()
                .is_none()
        );
    }

    fn token_server(status: &str, body: &str) -> (String, thread::JoinHandle<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}/token", listener.local_addr().unwrap());
        let response = format!(
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        let disconnect = status == "disconnect";
        let worker = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut reader = BufReader::new(&mut stream);
            let mut length = 0;
            let mut headers = String::new();
            loop {
                let mut line = String::new();
                assert!(reader.read_line(&mut line).unwrap() > 0);
                headers.push_str(&line);
                if line == "\r\n" {
                    break;
                }
                if let Some((name, value)) = line.split_once(':')
                    && name.eq_ignore_ascii_case("content-length")
                {
                    length = value.trim().parse().unwrap();
                }
            }
            let mut body = vec![0; length];
            reader.read_exact(&mut body).unwrap();
            drop(reader);
            if !disconnect {
                stream.write_all(response.as_bytes()).unwrap();
            }
            headers + &String::from_utf8(body).unwrap()
        });
        (url, worker)
    }

    #[test]
    fn label_catalog_gets_colors_once_per_used_id_and_retains_cache_on_failure() {
        let client = BasicClient::new(ClientId::new("synthetic-label-check".into()))
            .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
            .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
        let mut session = Session::with_refresh_token(http_client().unwrap(), client, "synthetic-label-check".into(), RefreshToken::new("unused".into()));
        session.token = "synthetic-access".into();
        session.expires_at = Instant::now() + Duration::from_secs(3600);
        let (url, server) = token_server("200 OK", r#"{"labels":[{"id":"project","name":"Projects","type":"user"},{"id":"INBOX","name":"Inbox","type":"system"}]}"#);
        let catalog = session.label_catalog_at(Url::parse(&url).unwrap());
        assert!(!catalog.unavailable);
        assert!(catalog.entries["project"].color.is_none());
        assert!(server.join().unwrap().starts_with("GET /token HTTP/1.1"));
        let (url, server) = token_server("200 OK", r##"{"id":"project","name":"Projects","type":"user","color":{"textColor":"#ffffff","backgroundColor":"#076239"}}"##);
        let ids = vec!["project".into(), "project".into(), "INBOX".into(), "missing".into()];
        let colored = session.label_colors_at(&ids, Url::parse(&url).unwrap());
        assert_eq!(colored.entries["project"].color.as_ref().unwrap().background, "#076239");
        assert!(server.join().unwrap().starts_with("GET /token/project HTTP/1.1"));
        for colors in [false, true] {
            let (url, server) = token_server("503 Service Unavailable", "{}");
            let cached = if colors { session.label_colors_at(&ids, Url::parse(&url).unwrap()) }
                else { session.label_catalog_at(Url::parse(&url).unwrap()) };
            assert!(cached.unavailable);
            assert_eq!(cached.entries["project"].name, "Projects");
            assert!(cached.entries["project"].color.is_some());
            assert!(server.join().unwrap().starts_with("GET "));
        }
        let (url, server) = token_server("200 OK", r#"{"labels":[]}"#);
        let deleted = session.label_catalog_at(Url::parse(&url).unwrap());
        assert!(deleted.entries.is_empty() && !deleted.unavailable);
        server.join().unwrap();
    }
    #[test]
    fn metadata_retains_labels_and_defaults_old_fixtures_to_unlabeled() {
        let message: Message = serde_json::from_str(r#"{"id":"m","labelIds":["INBOX","project"],"payload":{}}"#).unwrap();
        assert_eq!(message.into_email().label_ids, vec!["INBOX", "project"]);
        let message: Message = serde_json::from_str(r#"{"id":"m","payload":{}}"#).unwrap();
        assert!(message.into_email().label_ids.is_empty());
    }
    #[test]
    fn refresh_remembers_rotation_and_only_forgets_revoked_grants() {
        let cases = [
            (
                "200 OK",
                r#"{"access_token":"new-access","token_type":"Bearer","expires_in":3600,"refresh_token":"rotated-refresh"}"#,
                Some("rotated-refresh"),
                Some(true),
            ),
            (
                "200 OK",
                r#"{"access_token":"new-access","token_type":"Bearer","expires_in":3600}"#,
                Some("old-refresh"),
                Some(true),
            ),
            (
                "400 Bad Request",
                r#"{"error":"invalid_grant"}"#,
                None,
                Some(false),
            ),
            (
                "503 Service Unavailable",
                r#"{"error":"temporarily_unavailable"}"#,
                Some("old-refresh"),
                None,
            ),
        ];
        for (status, response, expected_saved, expected_result) in cases {
            // Only synthetic credentials are touched, never the user's OAuth client entry.
            let user = format!(
                "mado-mail-test-{}-{}",
                std::process::id(),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_nanos()
            );
            let entry = credential_entry(&user).unwrap();
            entry.set_password("old-refresh").unwrap();
            let (url, server) = token_server(status, response);
            let client = BasicClient::new(ClientId::new("test-client".into()))
                .set_auth_uri(AuthUrl::new("https://example.com/authorize".into()).unwrap())
                .set_token_uri(TokenUrl::new(url).unwrap());
            let mut session = Session::with_refresh_token(
                http_client().unwrap(),
                client,
                user.clone(),
                RefreshToken::new("old-refresh".into()),
            );
            let result = if expected_result == Some(false) {
                session.inbox().map(|emails| emails.is_some())
            } else {
                session.refresh()
            };
            let saved = credential_entry(&user).unwrap().get_password();
            let cleanup = entry.delete_credential();
            let request = server.join().unwrap();
            assert!(matches!(cleanup, Ok(()) | Err(keyring::Error::NoEntry)));
            assert_eq!(result.as_ref().ok().copied(), expected_result);
            match expected_saved {
                Some(secret) => assert_eq!(saved.unwrap(), secret),
                None => assert!(matches!(saved, Err(keyring::Error::NoEntry))),
            }
            assert!(request.contains("grant_type=refresh_token"));
            assert!(request.contains("refresh_token=old-refresh"));
            if expected_result == Some(false) {
                entry.set_password("new-authorization").unwrap();
                let old_reader = session.inbox();
                let new_saved = entry.get_password();
                entry.delete_credential().unwrap();
                assert!(old_reader.unwrap().is_none());
                assert_eq!(new_saved.unwrap(), "new-authorization");
            }
            if expected_result == Some(true) {
                assert_eq!(session.token, "new-access");
                assert_eq!(session.refresh_token.secret(), expected_saved.unwrap());
                assert!(session.expires_at > Instant::now() + Duration::from_secs(30));
            }
        }
    }

    #[test]
    fn mime_prefers_body_over_attachments_and_decodes_charsets() {
        let payload: Payload = serde_json::from_value(serde_json::json!({
            "mimeType":"multipart/mixed", "parts":[
                {"mimeType":"text/html","filename":"attachment.html","body":{"data":"YXR0YWNoZWQ"}},
                {"mimeType":"message/rfc822","parts":[{"mimeType":"text/html"}]},
                {"mimeType":"multipart/alternative","parts":[
                    {"mimeType":"text/plain","body":{"data":"UGxhaW4="}},
                    {"mimeType":"text/html","headers":[{"name":"content-type","value":"text/html; CHARSET=\"windows-1252\""}],"body":{"data":"Y2Fm6Q"}}
                ]}
            ]
        })).unwrap();
        let html = payload.body_part("text/html").unwrap();
        assert_eq!(
            decode_text(html, &decode_part(&html.body.data).unwrap()).unwrap(),
            "café"
        );
        let plain = payload.body_part("text/plain").unwrap();
        assert_eq!(
            decode_text(plain, &decode_part(&plain.body.data).unwrap()).unwrap(),
            "Plain"
        );
        assert!(decode_part("not base64!").is_err());
        let invalid: Payload = serde_json::from_str(r#"{"mimeType":"text/plain"}"#).unwrap();
        assert!(decode_text(&invalid, &[0xff]).is_err());
        let empty: Payload = serde_json::from_str("{}").unwrap();
        assert!(empty.body_part("text/html").is_none());
    }

    #[test]
    fn full_message_fetch_reads_body_and_cid_parts_with_get_only() {
        let html = "<h1>Body</h1><img src=\"cid:banner\">";
        let payload = serde_json::json!({"id":"message", "payload":{
            "mimeType":"multipart/mixed", "parts":[
                {"mimeType":"text/html","filename":"attachment.html","body":{"data":"YXR0YWNoZWQ"}},
                {"mimeType":"multipart/related","parts":[
                    {"mimeType":"multipart/alternative","parts":[
                        {"mimeType":"text/plain","body":{"data":"UGxhaW4"}},
                        {"mimeType":"text/html","body":{"attachmentId":"body-file"}}
                    ]},
                    {"mimeType":"image/png","filename":"banner.png","headers":[{"name":"Content-ID","value":"<banner>"}],"body":{"attachmentId":"img-file"}}
                ]}
            ]
        }}).to_string();
        let body_response = serde_json::json!({"data":URL_SAFE_NO_PAD.encode(html)}).to_string();
        let image_response = serde_json::json!({"data":URL_SAFE_NO_PAD.encode(include_bytes!("../fixtures/reader-banner.png"))}).to_string();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = Url::parse(&format!(
            "http://{}/message",
            listener.local_addr().unwrap()
        ))
        .unwrap();
        let server = thread::spawn(move || {
            let mut requests = Vec::new();
            for response_body in [payload, body_response, image_response] {
                let (mut stream, _) = listener.accept().unwrap();
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut reader = BufReader::new(&mut stream);
                let mut headers = String::new();
                loop {
                    let mut line = String::new();
                    assert!(reader.read_line(&mut line).unwrap() > 0);
                    if line == "\r\n" {
                        break;
                    }
                    headers.push_str(&line);
                }
                drop(reader);
                requests.push(headers);
                write!(stream, "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}", response_body.len(), response_body).unwrap();
            }
            requests
        });
        let client = BasicClient::new(ClientId::new("synthetic".into()))
            .set_auth_uri(AuthUrl::new("https://example.invalid/authorize".into()).unwrap())
            .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
        let mut session = Session::with_refresh_token(
            http_client().unwrap(),
            client,
            "synthetic".into(),
            RefreshToken::new("synthetic".into()),
        );
        session.token = "synthetic-access".into();
        let body = session.fetch_body_url(url).unwrap();
        let doc = body.document();
        let requests = server.join().unwrap();
        assert_eq!(requests.len(), 3);
        for (request, target) in requests.iter().zip([
            "/message?format=full",
            "/message/attachments/body-file",
            "/message/attachments/img-file",
        ]) {
            assert!(request.starts_with(&format!("GET {target} HTTP/1.1")));
            assert!(
                request
                    .to_ascii_lowercase()
                    .contains("authorization: bearer synthetic-access")
            );
        }
        assert!(doc.contains("<h1>Body</h1>"));
        assert!(doc.contains("data:image/png;base64,"));
        assert!(!doc.contains("synthetic-access"));
        assert!(!doc.contains("attached"));
        assert!(body.warnings.is_empty());
    }

    #[test]
    fn direct_label_removal_sends_only_the_user_label_and_reports_failures() {
        let client = BasicClient::new(ClientId::new("synthetic".into()))
            .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
            .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
        let session = Session::with_refresh_token(http_client().unwrap(), client, "synthetic".into(), RefreshToken::new("unused".into()));
        for (status, expected) in [("200 OK", 0), ("403 Forbidden", 1), ("503 Service Unavailable", 2), ("disconnect", 2)] {
            let (url, server) = token_server(status, "{}");
            let result = session.remove_label_url(Url::parse(&url).unwrap(), "projects").unwrap();
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /token/modify HTTP/1.1"));
            let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, serde_json::json!({"removeLabelIds":["projects"]}));
            assert_eq!(match result { Outcome::Confirmed => 0, Outcome::Failed(_) => 1, Outcome::Unknown(_) => 2 }, expected);
        }
        for invalid in ["", "INBOX", "UNREAD", "TRASH", "SPAM"] {
            assert!(session.remove_label_url(Url::parse("http://127.0.0.1:1/").unwrap(), invalid).is_err());
        }
    }
    #[test]
    fn direct_label_assignment_sends_only_add_labels_and_reports_failures() {
        let client = BasicClient::new(ClientId::new("synthetic".into()))
            .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
            .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
        let session = Session::with_refresh_token(http_client().unwrap(), client, "synthetic".into(), RefreshToken::new("unused".into()));
        for (status, expected) in [("200 OK", 0), ("403 Forbidden", 1), ("429 Too Many Requests", 1), ("503 Service Unavailable", 2), ("disconnect", 2)] {
            let (url, server) = token_server(status, "{}");
            let result = session.add_label_url(Url::parse(&url).unwrap(), "projects").unwrap();
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /token/modify HTTP/1.1"));
            let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, serde_json::json!({"addLabelIds":["projects"]}));
            assert_eq!(match result { Outcome::Confirmed => 0, Outcome::Failed(_) => 1, Outcome::Unknown(_) => 2 }, expected);
        }
        for invalid in ["", "INBOX", "TRASH", "UNREAD"] {
            assert!(session.add_label_url(Url::parse("http://127.0.0.1:1/").unwrap(), invalid).is_err());
        }
    }
    #[test]
    fn spaces_add_every_label_and_remove_only_inbox_with_per_message_outcomes() {
        let client = BasicClient::new(ClientId::new("synthetic".into()))
            .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
            .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
        let session = Session::with_refresh_token(http_client().unwrap(), client, "synthetic".into(), RefreshToken::new("unused".into()));
        let labels = vec!["projects".into(), "finance".into()];
        for (status, expected) in [("200 OK", 0), ("403 Forbidden", 1), ("503 Service Unavailable", 2), ("disconnect", 2)] {
            let (url, server) = token_server(status, "{}");
            let result = session.write_url(Url::parse(&url).unwrap(), Action::Space(1), &labels).unwrap();
            let request = server.join().unwrap();
            assert!(request.starts_with("POST /token/modify HTTP/1.1"));
            let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(body, serde_json::json!({"addLabelIds":["projects","finance"],"removeLabelIds":["INBOX"]}));
            assert_eq!(match result { Outcome::Confirmed => 0, Outcome::Failed(_) => 1, Outcome::Unknown(_) => 2 }, expected);
        }
        for (state, done) in [
            (vec!["projects", "finance", "UNREAD", "other"], true),
            (vec!["projects", "finance", "INBOX"], false),
            (vec!["projects", "UNREAD"], false),
            (vec!["UNREAD"], false),
        ] {
            let (url, server) = token_server("200 OK", &serde_json::json!({"labelIds":state}).to_string());
            assert_eq!(session.recheck_url(Url::parse(&url).unwrap(), Action::Space(1), &labels).unwrap() == Outcome::Confirmed, done);
            assert!(server.join().unwrap().starts_with("GET /token?format=minimal&fields=labelIds HTTP/1.1"));
        }
        // Reject unsafe configuration before even connecting.
        assert!(session.write_url(Url::parse("http://127.0.0.1:1/").unwrap(), Action::Space(1), &["INBOX".into()]).is_err());
    }
    #[test]
    fn writes_preserve_unread_and_separate_failed_from_unknown() {
        for (action, status, expected) in [
            (Action::Archive, "200 OK", 0),
            (Action::Trash, "200 OK", 0),
            (Action::Archive, "403 Forbidden", 1),
            (Action::Trash, "429 Too Many Requests", 1),
            (Action::Archive, "503 Service Unavailable", 2),
            (Action::Trash, "408 Request Timeout", 2),
            (Action::Archive, "disconnect", 2),
        ] {
            let (url, server) = token_server(status, "{}");
            let client = BasicClient::new(ClientId::new("synthetic".into()))
                .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
                .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
            let session = Session::with_refresh_token(
                http_client().unwrap(),
                client,
                "synthetic".into(),
                RefreshToken::new("synthetic".into()),
            );
            let result = session
                .write_url(Url::parse(&url).unwrap(), action, &[])
                .unwrap();
            let request = server.join().unwrap();
            let endpoint = if action == Action::Archive {
                "modify"
            } else {
                "trash"
            };
            assert!(request.starts_with(&format!("POST /token/{endpoint} HTTP/1.1")));
            let body: serde_json::Value =
                serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
            assert_eq!(
                body,
                match action {
                    Action::Archive => serde_json::json!({"removeLabelIds":["INBOX"]}),
                    Action::Trash => serde_json::json!({}),
                    Action::Space(_) => unreachable!(),
                }
            );
            assert!(!body.to_string().contains("UNREAD"));
            assert_eq!(
                match result {
                    Outcome::Confirmed => 0,
                    Outcome::Failed(_) => 1,
                    Outcome::Unknown(_) => 2,
                },
                expected
            );
        }
    }

    #[test]
    fn read_actions_change_only_unread_and_report_write_outcomes() {
        for read in [true, false] {
            for (status, expected) in [
                ("200 OK", 0), ("403 Forbidden", 1), ("503 Service Unavailable", 2),
                ("408 Request Timeout", 2), ("disconnect", 2),
            ] {
                let (url, server) = token_server(status, "{}");
                let client = BasicClient::new(ClientId::new("synthetic".into()))
                    .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
                    .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
                let mut session = Session::with_refresh_token(
                    http_client().unwrap(), client, "synthetic".into(), RefreshToken::new("synthetic".into()),
                );
                session.token = "synthetic-access".into();
                session.expires_at = Instant::now() + Duration::from_secs(300);
                let outcome = session.authorized(|s| s.set_read_url(Url::parse(&url).unwrap(), read)).unwrap().unwrap();
                let request = server.join().unwrap();
                assert!(request.starts_with("POST /token/modify HTTP/1.1"));
                assert!(request.to_ascii_lowercase().contains("authorization: bearer synthetic-access"));
                let body: serde_json::Value = serde_json::from_str(request.split_once("\r\n\r\n").unwrap().1).unwrap();
                assert_eq!(body, if read { serde_json::json!({"removeLabelIds":["UNREAD"]}) }
                    else { serde_json::json!({"addLabelIds":["UNREAD"]}) });
                assert_eq!(match outcome { Outcome::Confirmed => 0, Outcome::Failed(_) => 1, Outcome::Unknown(_) => 2 }, expected);
            }
        }
    }

    #[test]
    fn reconciliation_uses_labels_not_the_capped_inbox() {
        for (action, labels, done) in [
            (Action::Archive, vec!["UNREAD"], true),
            (Action::Archive, vec!["INBOX", "UNREAD"], false),
            (Action::Trash, vec!["TRASH", "UNREAD"], true),
            (Action::Trash, vec!["UNREAD"], false),
        ] {
            let (url, server) = token_server(
                "200 OK",
                &serde_json::json!({"labelIds":labels}).to_string(),
            );
            let client = BasicClient::new(ClientId::new("synthetic".into()))
                .set_auth_uri(AuthUrl::new("https://example.invalid/auth".into()).unwrap())
                .set_token_uri(TokenUrl::new("https://example.invalid/token".into()).unwrap());
            let session = Session::with_refresh_token(
                http_client().unwrap(),
                client,
                "synthetic".into(),
                RefreshToken::new("synthetic".into()),
            );
            assert_eq!(
                session
                    .recheck_url(Url::parse(&url).unwrap(), action, &[])
                    .unwrap()
                    == Outcome::Confirmed,
                done
            );
            let request = server.join().unwrap();
            assert!(request.starts_with("GET /token?format=minimal&fields=labelIds HTTP/1.1"));
            assert!(request.split_once("\r\n\r\n").unwrap().1.is_empty());
        }
    }

    #[test]
    fn metadata_handles_header_case_and_missing_subject() {
        let message: Message = serde_json::from_str(r#"{"id":"1","snippet":"Preview","payload":{"headers":[{"name":"from","value":"Sender <sender@example.com>"}]}}"#).unwrap();
        let email = message.into_email();
        assert_eq!(email.id, "1");
        assert_eq!(email.from, "Sender <sender@example.com>");
        assert_eq!(email.subject, "(No subject)");
        assert_eq!(email.snippet, "Preview");
        let inbox: Inbox = serde_json::from_str("{}").unwrap();
        assert!(inbox.messages.is_empty());
    }
}
