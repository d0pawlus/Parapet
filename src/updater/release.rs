//! What GitHub says about the latest release: its version and the download for this OS, with its SHA-256.

use serde_json::Value;

pub const LATEST_URL: &str = "https://api.github.com/repos/d0pawlus/Parapet/releases/latest";

/// The release asset this build updates itself from (None: no self-update on this OS).
pub const ASSET_NAME: Option<&str> = if cfg!(windows) {
    Some("Parapet.exe")
} else if cfg!(all(target_os = "macos", target_arch = "aarch64")) {
    Some("Parapet-macOS-arm64.zip")
} else {
    None
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Version(pub u32, pub u32, pub u32);

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}.{}", self.0, self.1, self.2)
    }
}

impl Version {
    /// "v1.2.3", "1.2.3" or "1.2" (missing parts are 0). Pre-release suffixes are rejected.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.trim();
        let s = s.strip_prefix(['v', 'V']).unwrap_or(s);
        let parts: Vec<u32> = s.split('.').map(|p| p.parse().ok()).collect::<Option<_>>()?;
        match parts[..] {
            [a] => Some(Version(a, 0, 0)),
            [a, b] => Some(Version(a, b, 0)),
            [a, b, c] => Some(Version(a, b, c)),
            _ => None,
        }
    }

    /// The version of this build. Debug builds can pretend to be older: PARAPET_PRETEND_VERSION=0.1.0
    pub fn current() -> Version {
        let own = Version::parse(env!("CARGO_PKG_VERSION")).unwrap_or(Version(0, 0, 0));
        if cfg!(debug_assertions) {
            if let Some(v) = std::env::var("PARAPET_PRETEND_VERSION").ok().and_then(|v| Version::parse(&v)) {
                return v;
            }
        }
        own
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Asset {
    pub name: String,
    pub url: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub version: Version,
    pub page_url: String,
    /// None when the release has no (verifiable) download for this OS: the page can still be opened.
    pub asset: Option<Asset>,
}

/// "sha256:<64 hex digits>" as GitHub reports it for release assets.
pub fn parse_digest(s: &str) -> Option<[u8; 32]> {
    let hex = s.strip_prefix("sha256:")?;
    if hex.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        *byte = u8::from_str_radix(hex.get(i * 2..i * 2 + 2)?, 16).ok()?;
    }
    Some(out)
}

fn parse_asset(a: &Value) -> Option<Asset> {
    let url = a["browser_download_url"].as_str()?;
    if !url.starts_with("https://github.com/") {
        return None;
    }
    Some(Asset {
        name: a["name"].as_str()?.to_string(),
        url: url.to_string(),
        size: a["size"].as_u64().filter(|s| *s > 0)?,
        sha256: parse_digest(a["digest"].as_str()?)?,
    })
}

/// The /releases/latest JSON → the release, with the asset called `asset_name` if it is there and verifiable.
pub fn parse_release(json: &Value, asset_name: Option<&str>) -> Result<Release, String> {
    let tag = json["tag_name"].as_str().ok_or("no tag in the release")?;
    let version = Version::parse(tag).ok_or_else(|| format!("unexpected version {tag}"))?;
    let page_url = json["html_url"]
        .as_str()
        .filter(|u| u.starts_with("https://github.com/"))
        .unwrap_or("https://github.com/d0pawlus/Parapet/releases/latest")
        .to_string();
    let asset = asset_name.and_then(|name| {
        json["assets"].as_array()?.iter().filter(|a| a["name"].as_str() == Some(name)).find_map(parse_asset)
    });
    Ok(Release { version, page_url, asset })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    const DIGEST: &str = "sha256:1139424bc72e3fbb166001a2098a7bb8dc72d433b04c3ab5361c5a44df58799b";

    fn release_json() -> Value {
        json!({
            "tag_name": "v0.3.0",
            "html_url": "https://github.com/d0pawlus/Parapet/releases/tag/v0.3.0",
            "assets": [
                { "name": "Parapet.exe", "size": 10, "digest": DIGEST,
                  "browser_download_url": "https://github.com/d0pawlus/Parapet/releases/download/v0.3.0/Parapet.exe" },
                { "name": "Parapet-macOS-arm64.zip", "size": 20, "digest": null,
                  "browser_download_url": "https://github.com/d0pawlus/Parapet/releases/download/v0.3.0/Parapet-macOS-arm64.zip" }
            ]
        })
    }

    #[test]
    fn parses_tags_with_and_without_prefix() {
        assert_eq!(Version::parse("v0.2.0"), Some(Version(0, 2, 0)));
        assert_eq!(Version::parse("1.4"), Some(Version(1, 4, 0)));
        assert_eq!(Version::parse("v1.0.0-beta"), None);
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
    }

    #[test]
    fn compares_versions_numerically() {
        assert!(Version(0, 10, 0) > Version(0, 9, 0));
        assert!(Version(1, 0, 0) > Version(0, 99, 99));
        assert!(Version(0, 2, 0) == Version::parse("v0.2").unwrap());
    }

    #[test]
    fn parses_github_digests() {
        let d = parse_digest(DIGEST).unwrap();
        assert_eq!(d[0], 0x11);
        assert_eq!(d[31], 0x9b);
        assert_eq!(parse_digest("sha256:abc"), None);
        assert_eq!(parse_digest("md5:1139424bc72e3fbb166001a2098a7bb8dc72d433b04c3ab5361c5a44df58799b"), None);
        assert_eq!(parse_digest(&DIGEST.replace('b', "z")), None);
    }

    #[test]
    fn picks_the_asset_by_exact_name() {
        let r = parse_release(&release_json(), Some("Parapet.exe")).unwrap();
        assert_eq!(r.version, Version(0, 3, 0));
        let a = r.asset.unwrap();
        assert_eq!((a.name.as_str(), a.size), ("Parapet.exe", 10));
        assert!(parse_release(&release_json(), Some("parapet.exe")).unwrap().asset.is_none());
        assert!(parse_release(&release_json(), None).unwrap().asset.is_none());
    }

    #[test]
    fn skips_assets_without_a_digest() {
        let r = parse_release(&release_json(), Some("Parapet-macOS-arm64.zip")).unwrap();
        assert!(r.asset.is_none());
        assert!(r.page_url.ends_with("/v0.3.0"));
    }

    #[test]
    fn rejects_downloads_from_other_hosts() {
        let mut j = release_json();
        j["assets"][0]["browser_download_url"] = json!("https://evil.example/Parapet.exe");
        assert!(parse_release(&j, Some("Parapet.exe")).unwrap().asset.is_none());
    }

    #[test]
    fn fails_without_a_usable_tag() {
        assert!(parse_release(&json!({}), None).is_err());
        assert!(parse_release(&json!({ "tag_name": "nightly" }), None).is_err());
    }
}
