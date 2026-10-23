//! Release-only signer. Keys come from the release job's environment, never command arguments.
use anyhow::{Context, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use std::{fs, io::Cursor};

fn signed(data: &[u8], key: &minisign::SecretKey, version: &str) -> Result<String> {
    semver::Version::parse(version)?;
    let comment = format!("version:{version}");
    let signature = minisign::sign(None, key, Cursor::new(data), Some(&comment), None)?;
    Ok(STANDARD.encode(signature.to_string()))
}
fn main() -> Result<()> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    anyhow::ensure!(args.len() == 2, "usage: argus-update-signer VERSION FILE");
    let encoded =
        std::env::var("ARGUS_SIGNING_PRIVATE_KEY").context("release signing key unavailable")?;
    let text = String::from_utf8(STANDARD.decode(encoded.trim())?)
        .context("invalid signing key encoding")?;
    let password = std::env::var("ARGUS_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
    let key = minisign::SecretKeyBox::from_string(&text)?.into_secret_key(Some(password))?;
    let signature = signed(&fs::read(&args[1])?, &key, &args[0])?;
    fs::write(format!("{}.sig", args[1]), signature)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn temporary_key_signatures_verify_and_bind_the_release_version() {
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let payload = b"native installer test payload";
        let encoded = signed(payload, &keys.sk, "1.2.3-beta.1").unwrap();
        let signature = minisign_verify::Signature::decode(
            &String::from_utf8(STANDARD.decode(encoded).unwrap()).unwrap(),
        )
        .unwrap();
        let key =
            minisign_verify::PublicKey::decode(&keys.pk.to_box().unwrap().to_string()).unwrap();
        key.verify(payload, &signature, false).unwrap();
        assert!(signature.trusted_comment().contains("version:1.2.3-beta.1"));
        assert!(key.verify(b"changed", &signature, false).is_err());
    }
}
