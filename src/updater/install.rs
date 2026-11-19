//! Downloading a release asset (checked against its SHA-256), swapping it in for the running app and
//! starting the new one. The old copy is moved aside, not deleted, and is cleared up by the next start.

use super::release::Asset;
use sha2::{Digest, Sha256};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

/// Passed to the relaunched app: wait for the old instance to let go of the single-instance lock.
pub const AFTER_UPDATE_ARG: &str = "--after-update";

#[derive(Debug, PartialEq)]
pub enum InstallError {
    /// The app lives somewhere it cannot replace itself (read-only, translocated, not a bundle):
    /// the user has to download the update by hand.
    Manual(String),
    Failed(String),
}

impl std::fmt::Display for InstallError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InstallError::Manual(why) | InstallError::Failed(why) => f.write_str(why),
        }
    }
}

fn failed(what: &str, e: impl std::fmt::Display) -> InstallError {
    InstallError::Failed(format!("{what}: {e}"))
}

/// Streams `reader` into `dest`, hashing as it goes; `dest` is removed unless size and SHA-256 match.
pub fn save_verified(reader: &mut impl Read, dest: &Path, size: u64, sha256: &[u8; 32]) -> Result<(), InstallError> {
    let result = (|| {
        let mut file = std::fs::File::create(dest).map_err(|e| failed("cannot save the download", e))?;
        let mut hasher = Sha256::new();
        let mut buf = vec![0u8; 64 * 1024];
        let mut written = 0u64;
        loop {
            let n = reader.read(&mut buf).map_err(|e| failed("download interrupted", e))?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            file.write_all(&buf[..n]).map_err(|e| failed("cannot save the download", e))?;
            written += n as u64;
        }
        file.sync_all().map_err(|e| failed("cannot save the download", e))?;
        if written != size {
            return Err(InstallError::Failed(format!("download incomplete ({written} of {size} bytes)")));
        }
        if hasher.finalize().as_slice() != sha256 {
            return Err(InstallError::Failed("download corrupted (checksum mismatch)".into()));
        }
        Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(dest);
    }
    result
}

fn download(agent: &ureq::Agent, asset: &Asset, dest: &Path) -> Result<(), InstallError> {
    let resp = agent.get(&asset.url).call().map_err(|e| InstallError::Failed(crate::net::describe_error(&asset.url, e)))?;
    // never read more than the release says the file has (+1 so an oversized body shows up as a size mismatch)
    let mut reader = resp.into_body().into_with_config().limit(asset.size + 1).reader();
    save_verified(&mut reader, dest, asset.size, &asset.sha256)
}

/// Can we create files in `dir`? (Probing is the only reliable answer: ACLs, read-only volumes, sandboxing.)
fn writable(dir: &Path) -> bool {
    let probe = dir.join(".parapet-write-test");
    let ok = std::fs::File::create(&probe).is_ok();
    let _ = std::fs::remove_file(&probe);
    ok
}

/// Replaces `target` with `new`, keeping the old one at `backup`; puts it back if the second step fails.
pub fn swap_in(target: &Path, new: &Path, backup: &Path) -> Result<(), InstallError> {
    remove_any(backup);
    std::fs::rename(target, backup).map_err(|e| failed("cannot move the old version aside", e))?;
    if let Err(e) = std::fs::rename(new, target) {
        let _ = std::fs::rename(backup, target);
        return Err(failed("cannot put the new version in place", e));
    }
    Ok(())
}

fn remove_any(path: &Path) {
    match std::fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => {
            let _ = std::fs::remove_dir_all(path);
        }
        Ok(_) => {
            let _ = std::fs::remove_file(path);
        }
        Err(_) => {}
    }
}

#[cfg(any(windows, test))]
fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
    name.push(suffix);
    path.with_file_name(name)
}

// ---------------------------------------------------------------- Windows: one .exe next to settings.ini

#[cfg(windows)]
fn locations() -> Result<PathBuf, InstallError> {
    let exe = std::env::current_exe().map_err(|e| failed("cannot find the running app", e))?;
    let dir = exe.parent().ok_or_else(|| InstallError::Manual("unexpected app location".into()))?;
    if !writable(dir) {
        return Err(InstallError::Manual(format!("{} is read-only", dir.display())));
    }
    Ok(exe)
}

/// Downloads, verifies and swaps in the new version; the caller then relaunches and exits.
#[cfg(windows)]
pub fn install(agent: &ureq::Agent, asset: &Asset) -> Result<PathBuf, InstallError> {
    let exe = locations()?;
    let staged = with_suffix(&exe, ".new");
    download(agent, asset, &staged)?;
    // a running .exe can be renamed, just not overwritten
    let result = swap_in(&exe, &staged, &with_suffix(&exe, ".old"));
    remove_any(&staged);
    result.map(|_| exe)
}

#[cfg(windows)]
pub fn relaunch(exe: &Path) -> Result<(), InstallError> {
    std::process::Command::new(exe).arg(AFTER_UPDATE_ARG).spawn().map(|_| ()).map_err(|e| failed("cannot restart", e))
}

#[cfg(windows)]
pub fn cleanup_leftovers() {
    if let Ok(exe) = std::env::current_exe() {
        remove_any(&with_suffix(&exe, ".old"));
        remove_any(&with_suffix(&exe, ".new"));
    }
}

// ---------------------------------------------------------------- macOS: the whole Parapet.app bundle

#[cfg(target_os = "macos")]
fn bundle_of(exe: &Path) -> Option<PathBuf> {
    // <bundle>.app/Contents/MacOS/<exe>
    let bundle = exe.parent()?.parent()?.parent()?;
    let is_bundle = exe.parent()?.ends_with("Contents/MacOS") && bundle.extension().is_some_and(|e| e == "app");
    is_bundle.then(|| bundle.to_path_buf())
}

#[cfg(target_os = "macos")]
fn staging_dir(bundle: &Path) -> Option<PathBuf> {
    Some(bundle.parent()?.join(".parapet-update"))
}

#[cfg(target_os = "macos")]
fn locations() -> Result<PathBuf, InstallError> {
    let exe = std::env::current_exe().map_err(|e| failed("cannot find the running app", e))?;
    if exe.to_string_lossy().contains("/AppTranslocation/") {
        return Err(InstallError::Manual("the app runs from a quarantined copy; move it to Applications".into()));
    }
    let bundle = bundle_of(&exe).ok_or_else(|| InstallError::Manual("not running from Parapet.app".into()))?;
    let parent = bundle.parent().ok_or_else(|| InstallError::Manual("unexpected app location".into()))?;
    if !writable(parent) {
        return Err(InstallError::Manual(format!("{} is read-only", parent.display())));
    }
    Ok(bundle)
}

#[cfg(target_os = "macos")]
fn run(cmd: &mut std::process::Command, what: &str) -> Result<(), InstallError> {
    let out = cmd.output().map_err(|e| failed(what, e))?;
    if out.status.success() {
        Ok(())
    } else {
        Err(failed(what, String::from_utf8_lossy(&out.stderr).trim()))
    }
}

/// Unpacks a downloaded zip next to `bundle` and swaps the Parapet.app inside it in for `bundle`.
#[cfg(target_os = "macos")]
pub fn install_bundle_from_zip(zip: &Path, bundle: &Path) -> Result<(), InstallError> {
    use std::process::Command;
    let staging = staging_dir(bundle).ok_or_else(|| InstallError::Manual("unexpected app location".into()))?;
    let unpacked = staging.join("unpacked");
    remove_any(&unpacked);
    // ditto keeps the bundle's symlinks, permissions and signature intact
    run(Command::new("/usr/bin/ditto").arg("-x").arg("-k").arg(zip).arg(&unpacked), "cannot unpack the update")?;
    let new_app = unpacked.join("Parapet.app");
    if !new_app.join("Contents/MacOS/parapet").is_file() {
        return Err(InstallError::Failed("the update has no Parapet.app inside".into()));
    }
    run(Command::new("/usr/bin/codesign").arg("--verify").arg("--deep").arg(&new_app), "the update's signature is broken")?;
    swap_in(bundle, &new_app, &staging.join("previous.app"))
}

#[cfg(target_os = "macos")]
pub fn install(agent: &ureq::Agent, asset: &Asset) -> Result<PathBuf, InstallError> {
    let bundle = locations()?;
    let staging = staging_dir(&bundle).ok_or_else(|| InstallError::Manual("unexpected app location".into()))?;
    remove_any(&staging);
    std::fs::create_dir_all(&staging).map_err(|e| failed("cannot prepare the update", e))?;
    let zip = staging.join(&asset.name);
    download(agent, asset, &zip)?;
    install_bundle_from_zip(&zip, &bundle)?;
    Ok(bundle)
}

#[cfg(target_os = "macos")]
pub fn relaunch(bundle: &Path) -> Result<(), InstallError> {
    // -n: a new instance even though one with the same bundle id is still running (it is about to quit)
    std::process::Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .args(["--args", AFTER_UPDATE_ARG])
        .spawn()
        .map(|_| ())
        .map_err(|e| failed("cannot restart", e))
}

#[cfg(target_os = "macos")]
pub fn cleanup_leftovers() {
    let staging = std::env::current_exe().ok().as_deref().and_then(bundle_of).and_then(|b| staging_dir(&b));
    if let Some(dir) = staging {
        remove_any(&dir);
    }
}

// ---------------------------------------------------------------- elsewhere: download by hand

#[cfg(not(any(windows, target_os = "macos")))]
pub fn install(_agent: &ureq::Agent, _asset: &Asset) -> Result<PathBuf, InstallError> {
    let _ = (download, writable);
    Err(InstallError::Manual("no self-update on this system".into()))
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn relaunch(_app: &Path) -> Result<(), InstallError> {
    Ok(())
}

#[cfg(not(any(windows, target_os = "macos")))]
pub fn cleanup_leftovers() {}

/// Opens the release page in the default browser (for updates that have to be installed by hand).
pub fn open_page(url: &str) {
    let opener = if cfg!(windows) {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "/usr/bin/open"
    } else {
        "xdg-open"
    };
    if let Err(e) = std::process::Command::new(opener).arg(url).spawn() {
        eprintln!("parapet: cannot open {url}: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parapet-test-{name}-{}", std::process::id()));
        remove_any(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sha(bytes: &[u8]) -> [u8; 32] {
        Sha256::digest(bytes).into()
    }

    #[test]
    fn keeps_a_download_that_matches() {
        let dir = temp_dir("ok");
        let dest = dir.join("file");
        let data = b"new parapet";
        save_verified(&mut &data[..], &dest, data.len() as u64, &sha(data)).unwrap();
        assert_eq!(std::fs::read(&dest).unwrap(), data);
        remove_any(&dir);
    }

    #[test]
    fn deletes_a_download_with_the_wrong_checksum() {
        let dir = temp_dir("bad-sha");
        let dest = dir.join("file");
        let err = save_verified(&mut &b"tampered"[..], &dest, 8, &sha(b"original")).unwrap_err();
        assert!(err.to_string().contains("checksum"));
        assert!(!dest.exists());
        remove_any(&dir);
    }

    #[test]
    fn deletes_a_truncated_download() {
        let dir = temp_dir("short");
        let dest = dir.join("file");
        let err = save_verified(&mut &b"half"[..], &dest, 8, &sha(b"half")).unwrap_err();
        assert!(err.to_string().contains("incomplete"));
        assert!(!dest.exists());
        remove_any(&dir);
    }

    #[test]
    fn swaps_files_and_keeps_the_old_one() {
        let dir = temp_dir("swap");
        let (target, new, backup) = (dir.join("app"), dir.join("app.new"), dir.join("app.old"));
        std::fs::write(&target, "old").unwrap();
        std::fs::write(&new, "new").unwrap();
        std::fs::write(&backup, "stale").unwrap();
        swap_in(&target, &new, &backup).unwrap();
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "new");
        assert_eq!(std::fs::read_to_string(&backup).unwrap(), "old");
        assert!(!new.exists());
        remove_any(&dir);
    }

    #[test]
    fn puts_the_old_version_back_when_the_new_one_is_missing() {
        let dir = temp_dir("rollback");
        let (target, backup) = (dir.join("app"), dir.join("app.old"));
        std::fs::write(&target, "old").unwrap();
        assert!(swap_in(&target, &dir.join("missing"), &backup).is_err());
        assert_eq!(std::fs::read_to_string(&target).unwrap(), "old");
        remove_any(&dir);
    }

    #[test]
    fn appends_suffixes_to_the_whole_file_name() {
        assert_eq!(with_suffix(Path::new("C:/x/Parapet.exe"), ".old"), PathBuf::from("C:/x/Parapet.exe.old"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn finds_the_bundle_of_the_running_binary() {
        let exe = Path::new("/Applications/Parapet.app/Contents/MacOS/parapet");
        assert_eq!(bundle_of(exe), Some(PathBuf::from("/Applications/Parapet.app")));
        assert_eq!(bundle_of(Path::new("/Users/me/target/release/parapet")), None);
    }

    /// End to end on a real bundle: PARAPET_TEST_BUNDLE=/path/to/Parapet.app cargo test -- --ignored
    /// Installs the latest GitHub release over that copy.
    #[cfg(target_os = "macos")]
    #[test]
    #[ignore]
    fn installs_the_latest_release_over_a_bundle() {
        let bundle = PathBuf::from(std::env::var("PARAPET_TEST_BUNDLE").expect("PARAPET_TEST_BUNDLE"));
        let agent = crate::net::agent(std::time::Duration::from_secs(120));
        let json = agent.get(super::super::release::LATEST_URL).call().unwrap().body_mut().read_json().unwrap();
        let release = super::super::release::parse_release(&json, super::super::release::ASSET_NAME).unwrap();
        let asset = release.asset.expect("a verifiable macOS asset");
        let staging = staging_dir(&bundle).unwrap();
        std::fs::create_dir_all(&staging).unwrap();
        let zip = staging.join(&asset.name);
        download(&agent, &asset, &zip).unwrap();
        install_bundle_from_zip(&zip, &bundle).unwrap();
        let plist = std::fs::read_to_string(bundle.join("Contents/Info.plist")).unwrap();
        assert!(plist.contains(&release.version.to_string()));
        assert!(staging.join("previous.app/Contents/MacOS/parapet").is_file());
    }
}
