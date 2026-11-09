//! Self-update from GitHub Releases (macOS, Apple Silicon). No UI here.
//!
//! Every function that touches the network or the disk blocks: call it from a worker thread.
//! HTTP goes through the system `curl`, like the assistant does, so there is no HTTP crate.

use serde_json::Value;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

const USER_AGENT: &str = "Adaptaudio";
const TRUSTED_PREFIX: &str = "https://github.com/";
const ZIP_SUFFIX: &str = "macos-arm64.zip";
const EXECUTABLE_IN_BUNDLE: &str = "Contents/MacOS/adaptaudio";
const STATUS_MARKER: &str = "\n__adaptaudio_http_status__:";

/// One published release that has a build for this machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Release {
    /// Version without the leading "v" of the tag.
    pub version: String,
    pub notes: String,
    pub zip_url: String,
    pub sha256_url: Option<String>,
    pub page_url: String,
}

/// Turns "owner/name" or a github.com URL into "owner/name". Anything with characters GitHub
/// does not allow in those two names is rejected, because the result goes into a URL.
pub fn parse_repo(input: &str) -> Option<String> {
    let trimmed = input.trim();
    let url_prefixes = [
        "https://github.com/",
        "http://github.com/",
        "https://www.github.com/",
        "github.com/",
        "git@github.com:",
    ];
    let (rest, from_url) = url_prefixes
        .iter()
        .find_map(|prefix| strip_prefix_ignore_case(trimmed, prefix))
        .map_or((trimmed, false), |rest| (rest, true));
    let rest = rest.trim_end_matches('/');
    let mut parts = rest.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    // A pasted URL may point deeper (…/releases, …/tree/main); the plain form may not.
    if !from_url && parts.next().is_some() {
        return None;
    }
    let name = name.strip_suffix(".git").unwrap_or(name);
    let owner_ok = !owner.is_empty()
        && owner.len() <= 39
        && owner.chars().all(|c| c.is_ascii_alphanumeric() || c == '-');
    let name_ok = !name.is_empty()
        && name.len() <= 100
        && name != "."
        && name != ".."
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
    (owner_ok && name_ok).then(|| format!("{owner}/{name}"))
}

fn strip_prefix_ignore_case<'a>(text: &'a str, prefix: &str) -> Option<&'a str> {
    let head = text.get(..prefix.len())?;
    head.eq_ignore_ascii_case(prefix)
        .then(|| &text[prefix.len()..])
}

/// True when `candidate` is a later version than `current`. Compares the dotted numbers
/// ("v1.10.0" > "1.9.3"); with equal numbers a final release beats its pre-release
/// ("1.2.0" > "1.2.0-beta"). A candidate that is not a version is never newer.
pub fn is_newer(candidate: &str, current: &str) -> bool {
    let Some((candidate_numbers, candidate_pre)) = parse_version(candidate) else {
        return false;
    };
    let Some((current_numbers, current_pre)) = parse_version(current) else {
        return false;
    };
    let width = candidate_numbers.len().max(current_numbers.len());
    for i in 0..width {
        let a = candidate_numbers.get(i).copied().unwrap_or(0);
        let b = current_numbers.get(i).copied().unwrap_or(0);
        if a != b {
            return a > b;
        }
    }
    current_pre && !candidate_pre
}

/// The dotted numbers of a version and whether it carries a pre-release suffix.
fn parse_version(text: &str) -> Option<(Vec<u64>, bool)> {
    let text = text.trim();
    let text = text
        .strip_prefix('v')
        .or_else(|| text.strip_prefix('V'))
        .unwrap_or(text);
    let without_build = text.split('+').next().unwrap_or(text);
    let (core, pre) = match without_build.split_once('-') {
        Some((core, _)) => (core, true),
        None => (without_build, false),
    };
    let numbers: Option<Vec<u64>> = core.split('.').map(|part| part.parse().ok()).collect();
    let numbers = numbers?;
    (!numbers.is_empty()).then_some((numbers, pre))
}

/// Reads GitHub's "latest release" JSON and picks the `…macos-arm64.zip` asset, plus its
/// `.sha256` sibling when the release has one. Only github.com download links are accepted.
pub fn parse_latest(json: &str) -> Result<Release, String> {
    let root: Value = serde_json::from_str(json)
        .map_err(|e| format!("GitHub sent something that is not release JSON: {e}"))?;
    let tag = root
        .get("tag_name")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .ok_or("The release has no tag name.")?;
    let version = tag
        .strip_prefix('v')
        .or_else(|| tag.strip_prefix('V'))
        .unwrap_or(tag)
        .to_string();
    let notes = root
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let page_url = root
        .get("html_url")
        .and_then(Value::as_str)
        .filter(|url| is_trusted_url(url))
        .ok_or("The release has no github.com page link.")?
        .to_string();

    let assets: Vec<(&str, &str)> = root
        .get("assets")
        .and_then(Value::as_array)
        .map(|assets| {
            assets
                .iter()
                .filter_map(|asset| {
                    let name = asset.get("name")?.as_str()?;
                    let url = asset.get("browser_download_url")?.as_str()?;
                    Some((name, url))
                })
                .collect()
        })
        .unwrap_or_default();

    let (zip_name, zip_url) = assets
        .iter()
        .find(|(name, _)| name.ends_with(ZIP_SUFFIX))
        .ok_or_else(|| {
            format!("Release {tag} has no macOS build (a file ending in {ZIP_SUFFIX}).")
        })?;
    if !is_trusted_url(zip_url) {
        return Err(format!(
            "Release {tag}: the download link for {zip_name} does not point to github.com."
        ));
    }
    let sha_name = format!("{zip_name}.sha256");
    let sha256_url = match assets.iter().find(|(name, _)| *name == sha_name) {
        Some((_, url)) if is_trusted_url(url) => Some((*url).to_string()),
        Some(_) => {
            return Err(format!(
                "Release {tag}: the download link for {sha_name} does not point to github.com."
            ));
        }
        None => None,
    };

    Ok(Release {
        version,
        notes,
        zip_url: (*zip_url).to_string(),
        sha256_url,
        page_url,
    })
}

/// The URL ends up in a curl argument, so besides the host it must be free of whitespace,
/// quotes and control characters.
fn is_trusted_url(url: &str) -> bool {
    url.starts_with(TRUSTED_PREFIX)
        && url
            .chars()
            .all(|c| c.is_ascii_graphic() && !matches!(c, '"' | '\'' | '\\' | '{' | '}'))
}

/// Asks GitHub for the latest release of `repo` ("owner/name" or a github.com URL).
/// `Ok(None)` means this build is already the latest.
pub fn check(repo: &str) -> Result<Option<Release>, String> {
    let repo = parse_repo(repo)
        .ok_or("The update repository should look like \"owner/name\" (set it in Settings).")?;
    let url = format!("https://api.github.com/repos/{repo}/releases/latest");
    let output = run(
        curl()
            .args(["--silent", "--show-error", "--location"])
            .args(["--proto", "=https", "--proto-redir", "=https"])
            .args(["--connect-timeout", "15", "--max-time", "30"])
            .args(["--user-agent", USER_AGENT])
            .args(["--header", "Accept: application/vnd.github+json"])
            .args(["--write-out", &format!("{STATUS_MARKER}%{{http_code}}")])
            .arg(&url),
        "curl",
    )?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    let (payload, status) = split_status(&stdout).unwrap_or(("", 0));
    match status {
        200 => {
            let release = parse_latest(payload)?;
            Ok(is_newer(&release.version, CURRENT_VERSION).then_some(release))
        }
        0 => Err(format!(
            "Could not reach GitHub: {}",
            failure_text(&output, "curl")
        )),
        404 => Err(format!(
            "No releases found for {repo}. Either nothing has been published yet, or the \
             repository name is wrong or the repository is private."
        )),
        403 | 429 => Err(
            "GitHub is limiting requests from this network. Try again in a few minutes."
                .to_string(),
        ),
        other => Err(format!("GitHub answered with HTTP {other}.")),
    }
}

fn split_status(stdout: &str) -> Option<(&str, u16)> {
    let at = stdout.rfind(STATUS_MARKER)?;
    let status = stdout[at + STATUS_MARKER.len()..].trim().parse().ok()?;
    Some((&stdout[..at], status))
}

/// The `*.app` this executable runs from, or `None` under `cargo run` (then there is nothing
/// to replace and the UI can only link to the release page).
pub fn current_bundle() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let exe = exe.canonicalize().unwrap_or(exe);
    bundle_of(&exe)
}

fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let shaped_like_bundle = macos.file_name()? == "MacOS"
        && contents.file_name()? == "Contents"
        && bundle.extension()? == "app";
    shaped_like_bundle.then(|| bundle.to_path_buf())
}

/// Downloads `release`, checks it and puts it where `bundle` is. The old bundle stays next to
/// it as `<bundle>.previous`. Touches only the bundle, `<bundle>.previous` and a hidden
/// staging folder beside them (same disk, so the final swap is two renames).
pub fn install(release: &Release, bundle: &Path) -> Result<(), String> {
    if !is_trusted_url(&release.zip_url) {
        return Err("The download link does not point to github.com.".to_string());
    }
    if bundle.extension().is_none_or(|ext| ext != "app") {
        return Err(format!("{} is not an .app bundle.", bundle.display()));
    }
    let parent = bundle
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| format!("{} has no parent folder.", bundle.display()))?;
    let staging = Staging::create(parent)?;

    let zip = staging.path.join("update.zip");
    download(&release.zip_url, &zip, 900)?;

    if let Some(sha_url) = &release.sha256_url {
        if !is_trusted_url(sha_url) {
            return Err("The checksum link does not point to github.com.".to_string());
        }
        let sha_file = staging.path.join("update.zip.sha256");
        download(sha_url, &sha_file, 60)?;
        let published = std::fs::read_to_string(&sha_file)
            .map_err(|e| format!("Could not read the downloaded checksum: {e}"))?;
        let expected = parse_sha256(&published)
            .ok_or("The published checksum file does not contain a SHA-256.")?;
        let actual = sha256_of(&zip)?;
        if actual != expected {
            return Err(format!(
                "The download is damaged or was tampered with: SHA-256 is {actual}, the release \
                 says {expected}. Nothing was installed."
            ));
        }
    }

    let unpacked = staging.path.join("unpacked");
    let output = run(
        Command::new("/usr/bin/ditto")
            .args(["-x", "-k"])
            .arg(&zip)
            .arg(&unpacked),
        "ditto",
    )?;
    if !output.status.success() {
        return Err(format!(
            "Could not unpack the update: {}",
            failure_text(&output, "ditto")
        ));
    }
    let new_bundle = find_bundle(&unpacked)?;
    check_bundle(&new_bundle)?;
    swap_bundle(&new_bundle, bundle)
}

/// Starts the bundle as a new instance. The caller quits right after.
pub fn relaunch(bundle: &Path) -> Result<(), String> {
    let output = run(Command::new("/usr/bin/open").arg("-n").arg(bundle), "open")?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not start {}: {}",
            bundle.display(),
            failure_text(&output, "open")
        ))
    }
}

/// A hidden folder next to the bundle, removed again when the install ends either way.
struct Staging {
    path: PathBuf,
}

impl Staging {
    fn create(parent: &Path) -> Result<Staging, String> {
        static COUNTER: AtomicU32 = AtomicU32::new(0);
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let name = format!(
            ".adaptaudio-update-{}-{nanos}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        );
        let path = parent.join(name);
        // `create_dir` (not `_all`) fails if the name exists, so the folder is always fresh.
        std::fs::create_dir(&path).map_err(|e| {
            format!(
                "Could not create a working folder in {} ({e}). Move Adaptaudio.app to a folder \
                 you can write to, such as Applications.",
                parent.display()
            )
        })?;
        Ok(Staging { path })
    }
}

impl Drop for Staging {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn download(url: &str, dest: &Path, max_secs: u32) -> Result<(), String> {
    let output = run(
        curl()
            .args(["--silent", "--show-error", "--fail", "--location"])
            .args(["--proto", "=https", "--proto-redir", "=https"])
            .args(["--max-redirs", "5", "--connect-timeout", "15"])
            .args(["--max-time", &max_secs.to_string()])
            .args(["--user-agent", USER_AGENT])
            .arg("--output")
            .arg(dest)
            .arg(url),
        "curl",
    )?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "Could not download the update: {}",
            failure_text(&output, "curl")
        ))
    }
}

/// The first word of a `shasum`-style file ("<hex>  <file name>"), lower-cased.
fn parse_sha256(text: &str) -> Option<String> {
    let word = text.split_whitespace().next()?;
    (word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit()))
        .then(|| word.to_ascii_lowercase())
}

fn sha256_of(file: &Path) -> Result<String, String> {
    let output = run(
        Command::new("/usr/bin/shasum")
            .args(["-a", "256"])
            .arg(file),
        "shasum",
    )?;
    if !output.status.success() {
        return Err(format!(
            "Could not compute the checksum: {}",
            failure_text(&output, "shasum")
        ));
    }
    parse_sha256(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| "shasum printed something unexpected.".to_string())
}

/// The single `*.app` folder at the top of the unpacked archive.
fn find_bundle(unpacked: &Path) -> Result<PathBuf, String> {
    let entries = std::fs::read_dir(unpacked)
        .map_err(|e| format!("Could not read the unpacked update: {e}"))?;
    let mut bundles = Vec::new();
    for entry in entries {
        let path = entry
            .map_err(|e| format!("Could not read the unpacked update: {e}"))?
            .path();
        if path.extension().is_some_and(|ext| ext == "app") {
            bundles.push(path);
        }
    }
    match bundles.len() {
        1 => Ok(bundles.remove(0)),
        0 => Err("The update archive has no .app inside.".to_string()),
        _ => Err("The update archive has more than one .app inside.".to_string()),
    }
}

fn check_bundle(bundle: &Path) -> Result<(), String> {
    check_bundle_layout(bundle)?;
    let output = run(
        Command::new("/usr/bin/codesign")
            .args(["--verify", "--deep"])
            .arg(bundle),
        "codesign",
    )?;
    if output.status.success() {
        Ok(())
    } else {
        Err(format!(
            "The update failed its code signature check: {}",
            failure_text(&output, "codesign")
        ))
    }
}

/// A real folder (not a link) with a real executable where macOS expects it.
fn check_bundle_layout(bundle: &Path) -> Result<(), String> {
    let is_real_dir = std::fs::symlink_metadata(bundle).is_ok_and(|meta| meta.is_dir());
    let exe = bundle.join(EXECUTABLE_IN_BUNDLE);
    let has_exe = std::fs::symlink_metadata(&exe).is_ok_and(|meta| meta.is_file());
    if is_real_dir && has_exe {
        Ok(())
    } else {
        Err(format!(
            "The update is not an Adaptaudio app ({EXECUTABLE_IN_BUNDLE} is missing)."
        ))
    }
}

/// `<bundle>.previous`, e.g. `Adaptaudio.app.previous`.
fn previous_path(bundle: &Path) -> Option<PathBuf> {
    let mut name: OsString = bundle.file_name()?.to_os_string();
    name.push(".previous");
    Some(bundle.with_file_name(name))
}

/// Puts `new_bundle` where `bundle` is and keeps the old one as `<bundle>.previous`.
fn swap_bundle(new_bundle: &Path, bundle: &Path) -> Result<(), String> {
    swap_bundle_with(new_bundle, bundle, |from, to| std::fs::rename(from, to))
}

/// The swap with the move step passed in, so tests can make it fail.
fn swap_bundle_with(
    new_bundle: &Path,
    bundle: &Path,
    move_dir: impl Fn(&Path, &Path) -> std::io::Result<()>,
) -> Result<(), String> {
    let previous = previous_path(bundle)
        .ok_or_else(|| format!("{} is not a bundle path.", bundle.display()))?;
    // Check everything that can be checked before the first change on disk.
    if !new_bundle.is_dir() {
        return Err("The unpacked update disappeared before it could be installed.".to_string());
    }
    if !bundle.is_dir() {
        return Err(format!("{} is not there any more.", bundle.display()));
    }
    match std::fs::symlink_metadata(&previous) {
        Ok(meta) if meta.is_dir() => std::fs::remove_dir_all(&previous),
        Ok(_) => std::fs::remove_file(&previous),
        Err(_) => Ok(()),
    }
    .map_err(|e| format!("Could not remove the old {}: {e}", previous.display()))?;

    move_dir(bundle, &previous).map_err(|e| {
        format!(
            "Could not move the current app aside ({e}). Nothing was changed. Check that you \
             may write to {}.",
            bundle.parent().unwrap_or(bundle).display()
        )
    })?;
    if let Err(e) = move_dir(new_bundle, bundle) {
        return match move_dir(&previous, bundle) {
            Ok(()) => Err(format!(
                "Could not put the new version in place ({e}). The current version was kept."
            )),
            Err(rollback) => Err(format!(
                "Could not put the new version in place ({e}), and could not restore the \
                 current one ({rollback}). It is at {}: rename it back to {}.",
                previous.display(),
                bundle.display()
            )),
        };
    }
    Ok(())
}

fn curl() -> Command {
    let program = if Path::new("/usr/bin/curl").exists() {
        "/usr/bin/curl"
    } else {
        "curl"
    };
    Command::new(program)
}

fn run(command: &mut Command, tool: &str) -> Result<Output, String> {
    command
        .output()
        .map_err(|e| format!("Could not start {tool}: {e}"))
}

fn failure_text(output: &Output, tool: &str) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    if stderr.is_empty() {
        format!("{tool} exited with {}", output.status)
    } else {
        stderr
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::fs;

    const LATEST: &str = r###"{
      "url": "https://api.github.com/repos/martycow/adaptaudio/releases/204815162",
      "html_url": "https://github.com/martycow/adaptaudio/releases/tag/v0.2.0",
      "id": 204815162,
      "tag_name": "v0.2.0",
      "target_commitish": "main",
      "name": "Adaptaudio 0.2.0",
      "draft": false,
      "prerelease": false,
      "created_at": "2026-11-20T10:02:11Z",
      "published_at": "2026-11-20T10:15:40Z",
      "author": {"login": "martycow", "id": 1, "type": "User"},
      "assets": [
        {
          "url": "https://api.github.com/repos/martycow/adaptaudio/releases/assets/1001",
          "name": "Adaptaudio-0.2.0-macos-arm64.zip.sha256",
          "content_type": "application/octet-stream",
          "state": "uploaded",
          "size": 98,
          "browser_download_url": "https://github.com/martycow/adaptaudio/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip.sha256"
        },
        {
          "url": "https://api.github.com/repos/martycow/adaptaudio/releases/assets/1002",
          "name": "Adaptaudio-0.2.0-macos-arm64.zip",
          "content_type": "application/zip",
          "state": "uploaded",
          "size": 9423811,
          "browser_download_url": "https://github.com/martycow/adaptaudio/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip"
        },
        {
          "url": "https://api.github.com/repos/martycow/adaptaudio/releases/assets/1003",
          "name": "fmod-import.zip",
          "content_type": "application/zip",
          "state": "uploaded",
          "size": 4096,
          "browser_download_url": "https://github.com/martycow/adaptaudio/releases/download/v0.2.0/fmod-import.zip"
        }
      ],
      "tarball_url": "https://api.github.com/repos/martycow/adaptaudio/tarball/v0.2.0",
      "zipball_url": "https://api.github.com/repos/martycow/adaptaudio/zipball/v0.2.0",
      "body": "## Что нового\n- Самообновление\n- Правки в Симуляторе"
    }"###;

    #[test]
    fn repo_accepts_plain_and_url_forms() {
        let expected = Some("martycow/adaptaudio".to_string());
        assert_eq!(parse_repo("martycow/adaptaudio"), expected);
        assert_eq!(parse_repo("  martycow/adaptaudio/ "), expected);
        assert_eq!(
            parse_repo("https://github.com/martycow/adaptaudio"),
            expected
        );
        assert_eq!(
            parse_repo("https://GitHub.com/martycow/adaptaudio.git"),
            expected
        );
        assert_eq!(
            parse_repo("https://github.com/martycow/adaptaudio/releases/tag/v1"),
            expected
        );
        assert_eq!(parse_repo("github.com/martycow/adaptaudio"), expected);
        assert_eq!(
            parse_repo("git@github.com:martycow/adaptaudio.git"),
            expected
        );
        assert_eq!(
            parse_repo("anomaly-co/rune_ui.rs"),
            Some("anomaly-co/rune_ui.rs".to_string())
        );
    }

    #[test]
    fn repo_rejects_everything_else() {
        for bad in [
            "",
            "adaptaudio",
            "owner/",
            "/name",
            "owner/name/extra",
            "owner/na me",
            "owner/name?x=1",
            "owner/name#frag",
            "owner/..",
            "ow_ner/name",
            "owner/имя",
            "owner/a%2Fb",
            "https://example.com/owner/name",
            "https://github.com.evil.io/owner/name",
            "https://github.com/owner",
            "owner/name@v1",
        ] {
            assert_eq!(parse_repo(bad), None, "{bad:?}");
        }
        // Trailing newline is whitespace and gets trimmed; one in the middle does not.
        assert_eq!(parse_repo("owner/na\nme"), None);
    }

    #[test]
    fn newer_compares_numbers_not_text() {
        assert!(is_newer("0.2.0", "0.1.0"));
        assert!(is_newer("v0.1.1", "0.1.0"));
        assert!(is_newer("1.10.0", "1.9.3"));
        assert!(is_newer("V2", "1.9.9"));
        assert!(is_newer("1.0.0.1", "1.0.0"));
        assert!(is_newer("1.2.0", "1.2.0-beta.1"));
        assert!(is_newer("0.2.0+build5", "0.1.0"));

        assert!(!is_newer("0.1.0", "0.1.0"));
        assert!(!is_newer("v0.1.0", "0.1.0"));
        assert!(!is_newer("1.0", "1.0.0"));
        assert!(!is_newer("0.0.9", "0.1.0"));
        assert!(!is_newer("1.9.3", "1.10.0"));
        assert!(!is_newer("1.2.0-beta.1", "1.2.0"));
        assert!(!is_newer("1.2.0-beta.2", "1.2.0-beta.1"));
        assert!(!is_newer("nightly", "0.1.0"));
        assert!(!is_newer("", "0.1.0"));
        assert!(!is_newer("1..2", "0.1.0"));
        assert!(!is_newer("-1.0", "0.1.0"));
    }

    #[test]
    fn latest_picks_the_arm64_zip_and_its_checksum() {
        let release = parse_latest(LATEST).unwrap();
        assert_eq!(release.version, "0.2.0");
        assert!(release.notes.contains("Самообновление"));
        assert_eq!(
            release.zip_url,
            "https://github.com/martycow/adaptaudio/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip"
        );
        assert_eq!(
            release.sha256_url.as_deref(),
            Some(
                "https://github.com/martycow/adaptaudio/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip.sha256"
            )
        );
        assert_eq!(
            release.page_url,
            "https://github.com/martycow/adaptaudio/releases/tag/v0.2.0"
        );
        assert!(is_newer(&release.version, "0.1.0"));
    }

    fn release_json(assets: &str) -> String {
        format!(
            r#"{{"tag_name":"v0.2.0","html_url":"https://github.com/o/n/releases/tag/v0.2.0","body":null,"assets":[{assets}]}}"#
        )
    }

    fn asset(name: &str, url: &str) -> String {
        format!(r#"{{"name":"{name}","browser_download_url":"{url}"}}"#)
    }

    #[test]
    fn latest_without_checksum_or_notes_still_parses() {
        let json = release_json(&asset(
            "Adaptaudio-0.2.0-macos-arm64.zip",
            "https://github.com/o/n/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip",
        ));
        let release = parse_latest(&json).unwrap();
        assert_eq!(release.sha256_url, None);
        assert_eq!(release.notes, "");
    }

    #[test]
    fn latest_rejects_missing_or_foreign_assets() {
        assert!(parse_latest("<html>rate limited</html>").is_err());
        assert!(parse_latest(r#"{"message":"Not Found"}"#).is_err());

        let no_mac = release_json(&asset(
            "Adaptaudio-0.2.0-windows-x64.zip",
            "https://github.com/o/n/releases/download/v0.2.0/Adaptaudio-0.2.0-windows-x64.zip",
        ));
        assert!(
            parse_latest(&no_mac)
                .unwrap_err()
                .contains("no macOS build")
        );

        for foreign in [
            "https://evil.example/Adaptaudio-0.2.0-macos-arm64.zip",
            "http://github.com/o/n/Adaptaudio-0.2.0-macos-arm64.zip",
            "https://github.com.evil.example/Adaptaudio-0.2.0-macos-arm64.zip",
            "https://github.com/o/n/a b-macos-arm64.zip",
        ] {
            let json = release_json(&asset("Adaptaudio-0.2.0-macos-arm64.zip", foreign));
            assert!(parse_latest(&json).is_err(), "{foreign}");
        }

        let foreign_sha = release_json(&format!(
            "{},{}",
            asset(
                "Adaptaudio-0.2.0-macos-arm64.zip",
                "https://github.com/o/n/releases/download/v0.2.0/Adaptaudio-0.2.0-macos-arm64.zip",
            ),
            asset(
                "Adaptaudio-0.2.0-macos-arm64.zip.sha256",
                "https://evil.example/sum.sha256"
            )
        ));
        assert!(parse_latest(&foreign_sha).is_err());

        let foreign_page = r#"{"tag_name":"v1","html_url":"https://evil.example/","assets":[]}"#;
        assert!(parse_latest(foreign_page).is_err());
    }

    #[test]
    fn status_marker_is_split_from_the_body() {
        let out = format!("{{\"a\":1}}{STATUS_MARKER}404");
        assert_eq!(split_status(&out), Some(("{\"a\":1}", 404)));
        assert_eq!(split_status("curl said nothing"), None);
    }

    #[test]
    fn checksum_file_formats() {
        let hex = "9F86D081884C7D659A2FEAA0C55AD015A3BF4F1B2B0B822CD15D6C15B0F00A08";
        let lower = hex.to_ascii_lowercase();
        assert_eq!(
            parse_sha256(&format!("{hex}  Adaptaudio-0.2.0-macos-arm64.zip\n")),
            Some(lower.clone())
        );
        assert_eq!(parse_sha256(&lower), Some(lower));
        assert_eq!(parse_sha256(""), None);
        assert_eq!(parse_sha256("deadbeef  file.zip"), None);
        assert_eq!(parse_sha256("<html>Not Found</html>"), None);
    }

    #[test]
    fn shasum_matches_a_known_digest() {
        let dir = TempDir::new("shasum");
        let file = dir.path.join("test.bin");
        fs::write(&file, "test").unwrap();
        assert_eq!(
            sha256_of(&file).unwrap(),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }

    #[test]
    fn bundle_is_found_only_around_a_real_app_layout() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Adaptaudio.app/Contents/MacOS/adaptaudio"
            )),
            Some(PathBuf::from("/Applications/Adaptaudio.app"))
        );
        assert_eq!(
            bundle_of(Path::new("/Users/m/Adaptaudio/target/debug/adaptaudio")),
            None
        );
        assert_eq!(
            bundle_of(Path::new("/x/Adaptaudio.app/Contents/Helpers/adaptaudio")),
            None
        );
        assert_eq!(bundle_of(Path::new("/adaptaudio")), None);
    }

    /// A scratch folder under the system temp dir, removed on drop.
    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(label: &str) -> TempDir {
            let staging = Staging::create(&std::env::temp_dir()).unwrap();
            let path = staging.path.with_extension(label);
            fs::rename(&staging.path, &path).unwrap();
            TempDir { path }
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.path);
        }
    }

    fn fake_bundle(path: &Path, marker: &str) {
        fs::create_dir_all(path.join("Contents/MacOS")).unwrap();
        fs::write(path.join(EXECUTABLE_IN_BUNDLE), marker).unwrap();
    }

    fn marker(bundle: &Path) -> String {
        fs::read_to_string(bundle.join(EXECUTABLE_IN_BUNDLE)).unwrap()
    }

    #[test]
    fn swap_installs_the_new_bundle_and_keeps_the_old_one() {
        let dir = TempDir::new("swap");
        let bundle = dir.path.join("Adaptaudio.app");
        let previous = dir.path.join("Adaptaudio.app.previous");
        let new_bundle = dir.path.join("staging/Adaptaudio.app");
        let project = dir.path.join("Song.aaproj");
        fake_bundle(&bundle, "current");
        fake_bundle(&previous, "ancient");
        fake_bundle(&new_bundle, "new");
        fs::write(&project, "user data").unwrap();

        swap_bundle(&new_bundle, &bundle).unwrap();

        assert_eq!(marker(&bundle), "new");
        assert_eq!(marker(&previous), "current", "older .previous is replaced");
        assert!(!new_bundle.exists());
        assert_eq!(fs::read_to_string(&project).unwrap(), "user data");
    }

    #[test]
    fn swap_rolls_back_when_the_new_bundle_cannot_be_moved_in() {
        let dir = TempDir::new("rollback");
        let bundle = dir.path.join("Adaptaudio.app");
        let previous = dir.path.join("Adaptaudio.app.previous");
        let new_bundle = dir.path.join("staging/Adaptaudio.app");
        fake_bundle(&bundle, "current");
        fake_bundle(&new_bundle, "new");

        let calls = Cell::new(0);
        let err = swap_bundle_with(&new_bundle, &bundle, |from, to| {
            calls.set(calls.get() + 1);
            if calls.get() == 2 {
                Err(std::io::Error::other("disk full"))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();

        assert!(err.contains("disk full"), "{err}");
        assert!(err.contains("current version was kept"), "{err}");
        assert_eq!(calls.get(), 3, "aside, failed move in, move back");
        assert_eq!(marker(&bundle), "current");
        assert!(!previous.exists());
        assert_eq!(marker(&new_bundle), "new");
    }

    #[test]
    fn swap_changes_nothing_when_the_first_move_fails_or_the_update_is_gone() {
        let dir = TempDir::new("untouched");
        let bundle = dir.path.join("Adaptaudio.app");
        let previous = dir.path.join("Adaptaudio.app.previous");
        let new_bundle = dir.path.join("staging/Adaptaudio.app");
        fake_bundle(&bundle, "current");
        fake_bundle(&previous, "ancient");

        assert!(swap_bundle(&new_bundle, &bundle).is_err());
        assert_eq!(marker(&bundle), "current");
        assert_eq!(marker(&previous), "ancient", "kept: nothing was started");

        fake_bundle(&new_bundle, "new");
        let err = swap_bundle_with(&new_bundle, &bundle, |_, _| {
            Err(std::io::Error::other("read-only"))
        })
        .unwrap_err();
        assert!(err.contains("Nothing was changed"), "{err}");
        assert_eq!(marker(&bundle), "current");
        assert_eq!(marker(&new_bundle), "new");
    }

    #[test]
    fn layout_check_wants_the_executable_and_no_links() {
        let dir = TempDir::new("layout");
        let good = dir.path.join("Adaptaudio.app");
        fake_bundle(&good, "x");
        assert!(check_bundle_layout(&good).is_ok());

        let empty = dir.path.join("Empty.app");
        fs::create_dir_all(empty.join("Contents/MacOS")).unwrap();
        assert!(check_bundle_layout(&empty).is_err());

        let link = dir.path.join("Link.app");
        std::os::unix::fs::symlink(&good, &link).unwrap();
        assert!(check_bundle_layout(&link).is_err());

        let unpacked = dir.path.join("unpacked");
        fake_bundle(&unpacked.join("Adaptaudio.app"), "x");
        fs::write(unpacked.join("README.txt"), "").unwrap();
        assert_eq!(
            find_bundle(&unpacked).unwrap(),
            unpacked.join("Adaptaudio.app")
        );
        fake_bundle(&unpacked.join("Other.app"), "x");
        assert!(find_bundle(&unpacked).is_err());
    }

    #[test]
    fn install_refuses_foreign_links_before_touching_anything() {
        let dir = TempDir::new("refuse");
        let bundle = dir.path.join("Adaptaudio.app");
        fake_bundle(&bundle, "current");
        let release = Release {
            version: "9.9.9".to_string(),
            notes: String::new(),
            zip_url: "https://evil.example/Adaptaudio-9.9.9-macos-arm64.zip".to_string(),
            sha256_url: None,
            page_url: "https://github.com/o/n/releases/tag/v9.9.9".to_string(),
        };
        assert!(install(&release, &bundle).is_err());
        assert_eq!(marker(&bundle), "current");
        assert_eq!(fs::read_dir(&dir.path).unwrap().count(), 1);
    }
}
