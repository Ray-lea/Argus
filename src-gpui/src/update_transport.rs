//! Signed update transport. Application lifecycle and installation stay in updater.rs.
use futures_util::StreamExt;
use serde_json::Value;
use std::{fmt, time::Duration};
use url::Url;

#[derive(Debug)]
pub enum Error {
    Minisign,
    MissingSignedVersion,
    ReleaseNotFound,
    Reqwest(reqwest::Error),
}
impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Reqwest(error) => write!(f, "{error}"),
            Self::ReleaseNotFound => f.write_str(&argus_core::tr!("updater.notFound")),
            _ => f.write_str(&argus_core::tr!("updater.signatureFailed")),
        }
    }
}
impl std::error::Error for Error {}
impl From<reqwest::Error> for Error {
    fn from(error: reqwest::Error) -> Self {
        Self::Reqwest(error)
    }
}

#[derive(Clone)]
pub struct Update {
    pub version: String,
    pub body: Option<String>,
    pub date: Option<time::OffsetDateTime>,
    pub signature: String,
    pub download_url: Url,
    pub raw_json: Value,
    pub timeout: Option<Duration>,
    pubkey: String,
}
fn client(timeout: Duration) -> Result<reqwest::Client, Error> {
    Ok(reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(concat!("Argus/", env!("CARGO_PKG_VERSION")))
        .build()?)
}
pub async fn check(
    endpoint: Url,
    pubkey: &str,
    timeout: Duration,
) -> Result<Option<Update>, Error> {
    let response = client(timeout)?.get(endpoint).send().await?;
    if response.status() == reqwest::StatusCode::NO_CONTENT {
        return Ok(None);
    }
    let response = response.error_for_status()?;
    if response
        .content_length()
        .is_some_and(|n| n > 2 * 1024 * 1024)
    {
        return Err(Error::ReleaseNotFound);
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk?;
        if bytes.len() + chunk.len() > 2 * 1024 * 1024 {
            return Err(Error::ReleaseNotFound);
        }
        bytes.extend_from_slice(&chunk);
    }
    let raw: Value = serde_json::from_slice(&bytes).map_err(|_| Error::ReleaseNotFound)?;
    parse_release(raw, pubkey)
}
fn parse_release(raw: Value, pubkey: &str) -> Result<Option<Update>, Error> {
    let version = raw["version"]
        .as_str()
        .ok_or(Error::ReleaseNotFound)?
        .trim_start_matches('v')
        .to_owned();
    let remote = semver::Version::parse(&version).map_err(|_| Error::ReleaseNotFound)?;
    if remote <= semver::Version::parse(env!("CARGO_PKG_VERSION")).expect("package version") {
        return Ok(None);
    }
    let platform = if raw.get("platforms").is_some() {
        crate::platform::update_targets()
            .iter()
            .find_map(|target| raw["platforms"].get(target))
            .ok_or(Error::ReleaseNotFound)?
    } else {
        &raw
    };
    let signature = platform["signature"]
        .as_str()
        .ok_or(Error::MissingSignedVersion)?
        .to_owned();
    let download_url = Url::parse(platform["url"].as_str().ok_or(Error::ReleaseNotFound)?)
        .map_err(|_| Error::ReleaseNotFound)?;
    if download_url.scheme() != "https" {
        return Err(Error::ReleaseNotFound);
    }
    let date = raw["pub_date"].as_str().and_then(|s| {
        time::OffsetDateTime::parse(s, &time::format_description::well_known::Rfc3339).ok()
    });
    Ok(Some(Update {
        version,
        body: raw["notes"].as_str().map(str::to_owned),
        date,
        signature,
        download_url,
        raw_json: raw,
        timeout: None,
        pubkey: pubkey.into(),
    }))
}
impl Update {
    pub async fn download(
        &self,
        mut progress: impl FnMut(usize, Option<u64>),
        finished: impl FnOnce(),
    ) -> Result<Vec<u8>, Error> {
        let response = client(self.timeout.unwrap_or(Duration::from_secs(300)))?
            .get(self.download_url.clone())
            .send()
            .await?
            .error_for_status()?;
        let total = response.content_length();
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk?;
            bytes.extend_from_slice(&chunk);
            progress(chunk.len(), total);
        }
        crate::updater::verify_signed(&bytes, &self.signature, &self.pubkey, &self.version)
            .map_err(|_| Error::Minisign)?;
        finished();
        Ok(bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_manifest_without_a_signed_artifact_cannot_be_offered() {
        assert!(matches!(
            parse_release(
                serde_json::json!({"version":"999.0.0","platforms":{"windows-x86_64":{"url":"https://example.invalid/update.exe"}}}),
                "unused"
            ),
            Err(Error::MissingSignedVersion)
        ));
    }
    #[test]
    fn unsafe_download_schemes_are_rejected() {
        for scheme in ["http", "file", "javascript"] {
            assert!(parse_release(serde_json::json!({"version":"999.0.0","url":format!("{scheme}://example.invalid/update"),"signature":"signature"}),"unused").is_err());
        }
    }
}
