use anyhow::{Context, Result};
use serde::Deserialize;
use std::cmp::Ordering;

use crate::cli::UpdateCommand;

const LATEST_RELEASE_API: &str = "https://api.github.com/repos/ikhdark/KDS/releases/latest";
const RELEASES_URL: &str = "https://github.com/ikhdark/KDS/releases";

#[derive(Debug, Deserialize)]
struct LatestRelease {
    tag_name: String,
    html_url: Option<String>,
}

pub fn run(command: UpdateCommand) -> Result<i32> {
    match command {
        UpdateCommand::Check => check(),
    }
}

fn check() -> Result<i32> {
    let current = env!("CARGO_PKG_VERSION");
    println!("KDS update check");
    println!("Current version: {current}");
    println!("Releases: {RELEASES_URL}");

    let Some(release) = fetch_latest_release()? else {
        println!("Latest release: unavailable");
        println!("Status: could not check latest release");
        println!("Update: check the release page before reinstalling");
        return Ok(0);
    };
    let latest = release.tag_name.trim();
    println!("Latest release: {latest}");
    if let Some(url) = release
        .html_url
        .as_deref()
        .filter(|url| !url.trim().is_empty())
    {
        println!("Latest release URL: {url}");
    }

    match compare_versions(current, latest) {
        Some(Ordering::Less) => {
            println!("Status: update available");
            println!("Update: rerun the bootstrap installer for {latest}");
            println!("Bootstrap: irm https://raw.githubusercontent.com/ikhdark/KDS/{latest}/scripts/bootstrap.ps1 | iex");
        }
        Some(Ordering::Equal) => println!("Status: current"),
        Some(Ordering::Greater) => println!("Status: local version is newer than latest release"),
        None => {
            println!("Status: could not compare versions");
            println!("Update: check the release page before reinstalling");
        }
    }

    Ok(0)
}

fn fetch_latest_release() -> Result<Option<LatestRelease>> {
    let response = match ureq::get(LATEST_RELEASE_API)
        .set("User-Agent", concat!("kds/", env!("CARGO_PKG_VERSION")))
        .set("Accept", "application/vnd.github+json")
        .call()
    {
        Ok(response) => response,
        Err(ureq::Error::Status(404, _)) => return Ok(None),
        Err(err) => return Err(err).context("fetch latest KDS release from GitHub"),
    };
    let text = response
        .into_string()
        .context("read latest KDS release response")?;
    serde_json::from_str(&text)
        .map(Some)
        .context("parse latest KDS release response")
}

fn compare_versions(current: &str, latest: &str) -> Option<Ordering> {
    Some(parse_version(current)?.cmp(&parse_version(latest)?))
}

fn parse_version(raw: &str) -> Option<(u64, u64, u64)> {
    let trimmed = raw
        .trim()
        .strip_prefix('v')
        .or_else(|| raw.trim().strip_prefix('V'))
        .unwrap_or_else(|| raw.trim());
    let stable = trimmed.split(['-', '+']).next().unwrap_or(trimmed);
    let mut parts = stable.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next().unwrap_or("0").parse().ok()?;
    let patch = parts.next().unwrap_or("0").parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions_with_optional_v_prefix() {
        assert_eq!(parse_version("0.1.2"), Some((0, 1, 2)));
        assert_eq!(parse_version("v1.2.3"), Some((1, 2, 3)));
        assert_eq!(parse_version("V2.0.0+build"), Some((2, 0, 0)));
        assert_eq!(parse_version("1.2.3-beta.1"), Some((1, 2, 3)));
        assert_eq!(parse_version("not-a-version"), None);
    }

    #[test]
    fn compares_versions() {
        assert_eq!(compare_versions("0.1.0", "v0.1.1"), Some(Ordering::Less));
        assert_eq!(compare_versions("0.1.0", "v0.1.0"), Some(Ordering::Equal));
        assert_eq!(compare_versions("0.2.0", "v0.1.9"), Some(Ordering::Greater));
        assert_eq!(compare_versions("0.1.0", "latest"), None);
    }
}
