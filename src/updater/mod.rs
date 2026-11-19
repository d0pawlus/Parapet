//! The built-in updater: asks GitHub for the latest release now and then, and on request downloads it,
//! checks its SHA-256, swaps it in for the running app and restarts. Work runs on short-lived threads that
//! report back through a callback; the tray menu shows the status.

mod install;
mod release;

pub use install::{cleanup_leftovers, AFTER_UPDATE_ARG};
pub use release::{Release, Version};

use install::InstallError;
use std::time::Duration;

/// How often to look for a new version while the app runs.
pub const CHECK_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);
/// The first automatic check, a little after start (not to compete with the weather and the first frames).
pub const FIRST_CHECK_DELAY: Duration = Duration::from_secs(30);

const CHECK_TIMEOUT: Duration = Duration::from_secs(15);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(10 * 60);

#[derive(Clone, Debug, PartialEq)]
pub enum Status {
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Installing(Version),
    /// The new version is in place and starting: the app should quit now.
    Restarting,
    Failed(String),
}

impl Status {
    pub fn is_busy(&self) -> bool {
        matches!(self, Status::Checking | Status::Installing(_) | Status::Restarting)
    }

    /// The text of the tray menu item.
    pub fn label(&self) -> String {
        match self {
            Status::Idle => "Check for updates".into(),
            Status::Checking => "Checking for updates…".into(),
            Status::UpToDate => format!("Up to date ({}): check again", Version::current()),
            Status::Available(r) if r.asset.is_some() => format!("Install update {} and restart", r.version),
            Status::Available(r) => format!("Download update {}…", r.version),
            Status::Installing(v) => format!("Installing update {v}…"),
            Status::Restarting => "Restarting…".into(),
            Status::Failed(e) => format!("Update failed ({e}): try again"),
        }
    }

    /// A line for the tray tooltip, when there is something worth telling.
    pub fn tooltip(&self) -> Option<String> {
        match self {
            Status::Available(r) => Some(format!("Update available: {}", r.version)),
            _ => None,
        }
    }
}

fn fetch_latest() -> Result<Release, String> {
    let agent = crate::net::agent(CHECK_TIMEOUT);
    let mut resp = agent
        .get(release::LATEST_URL)
        .header("Accept", "application/vnd.github+json")
        .call()
        .map_err(|e| crate::net::describe_error(release::LATEST_URL, e))?;
    let json = resp.body_mut().read_json::<serde_json::Value>().map_err(|_| "bad response".to_string())?;
    release::parse_release(&json, release::ASSET_NAME)
}

fn check_now() -> Status {
    match fetch_latest() {
        Ok(r) if r.version > Version::current() => Status::Available(r),
        Ok(_) => Status::UpToDate,
        Err(e) => Status::Failed(e),
    }
}

fn install_now(release: &Release) -> Status {
    let Some(asset) = &release.asset else {
        install::open_page(&release.page_url);
        return Status::Available(release.clone());
    };
    let agent = crate::net::agent(DOWNLOAD_TIMEOUT);
    let installed = install::install(&agent, asset).and_then(|app| install::relaunch(&app));
    match installed {
        Ok(()) => Status::Restarting,
        Err(InstallError::Manual(why)) => {
            eprintln!("parapet: update {} has to be installed by hand: {why}", release.version);
            install::open_page(&release.page_url);
            Status::Available(Release { asset: None, ..release.clone() })
        }
        Err(InstallError::Failed(why)) => Status::Failed(why),
    }
}

fn spawn(report: impl Fn(Status) + Send + 'static, work: impl FnOnce() -> Status + Send + 'static) {
    let spawned = std::thread::Builder::new().name("updater".into()).spawn(move || report(work()));
    if let Err(e) = spawned {
        eprintln!("parapet: cannot start the updater: {e}");
    }
}

/// Looks for a newer release in the background; `report` gets the outcome.
pub fn check(report: impl Fn(Status) + Send + 'static) {
    spawn(report, check_now);
}

/// Installs `release` in the background (or opens its page when it cannot); `report` gets the outcome.
pub fn install(release: Release, report: impl Fn(Status) + Send + 'static) {
    spawn(report, move || install_now(&release));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn release(with_asset: bool) -> Release {
        Release {
            version: Version(9, 0, 0),
            page_url: "https://github.com/d0pawlus/Parapet/releases/tag/v9.0.0".into(),
            asset: with_asset.then(|| release::Asset {
                name: "Parapet.exe".into(),
                url: "https://github.com/x".into(),
                size: 1,
                sha256: [0; 32],
            }),
        }
    }

    #[test]
    fn labels_follow_the_status() {
        assert_eq!(Status::Idle.label(), "Check for updates");
        assert_eq!(Status::Available(release(true)).label(), "Install update 9.0.0 and restart");
        assert_eq!(Status::Available(release(false)).label(), "Download update 9.0.0…");
        assert!(Status::Failed("HTTP 403".into()).label().contains("HTTP 403"));
    }

    #[test]
    fn only_running_work_is_busy() {
        assert!(Status::Checking.is_busy());
        assert!(Status::Installing(Version(1, 0, 0)).is_busy());
        assert!(!Status::Available(release(true)).is_busy());
        assert!(!Status::Failed(String::new()).is_busy());
    }

    #[test]
    fn tooltip_mentions_only_available_updates() {
        assert_eq!(Status::Available(release(true)).tooltip().as_deref(), Some("Update available: 9.0.0"));
        assert_eq!(Status::UpToDate.tooltip(), None);
    }
}
