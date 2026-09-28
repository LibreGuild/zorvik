//! Updates from GitHub Releases (docs/architecture.md, "Updates").
//!
//! The app reads the release's `latest.json` on GitHub a little after it starts and then every
//! few hours. That request is the only thing sent: nothing about the user, the computer or
//! their work. A newer version is downloaded in the background (Settings → Updates →
//! Automatic), checked against the project's signing key, and installed when the user restarts
//! or quits. Installs the app can't update itself (the portable zip, .deb and .rpm, a copy
//! running from a disk image) only say that a new version is out. So does any update that
//! couldn't be downloaded or installed (a network filter, a signature that doesn't match):
//! the user gets a link to the release instead of an error.

use std::sync::Mutex;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_updater::{RemoteRelease, Update, UpdaterExt};
use zorvik_api::Api;

const STABLE_URL: &str = "https://github.com/LibreGuild/zorvik/releases/latest/download/latest.json";
const NIGHTLY_URL: &str = "https://github.com/LibreGuild/zorvik/releases/download/nightly/latest.json";
/// Where people download Zorvik by hand.
const RELEASES_PAGE: &str = "https://github.com/LibreGuild/zorvik/releases";
/// The first check waits until the app has settled.
const FIRST_CHECK: Duration = Duration::from_secs(20);
const CHECK_EVERY: Duration = Duration::from_secs(6 * 60 * 60);
const TIMEOUT: Duration = Duration::from_secs(30);
/// Emitted to the UI whenever the state changes.
const EVENT: &str = "zv:update";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    Automatic,
    Notify,
    Off,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Channel {
    Stable,
    Nightly,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(tag = "state", rename_all = "camelCase", rename_all_fields = "camelCase")]
pub enum Status {
    Idle,
    Checking,
    UpToDate {
        checked_at: i64,
    },
    /// A newer version, not downloaded (notify-only, or this install can't update itself).
    Available {
        version: String,
        notes: Option<String>,
        date: Option<String>,
        /// Installing it automatically didn't work (the download failed, or its signature
        /// didn't match): download it from GitHub instead.
        by_hand: bool,
    },
    Downloading {
        version: String,
        percent: Option<u8>,
    },
    /// Downloaded and verified: installed on restart or quit.
    Ready {
        version: String,
        notes: Option<String>,
    },
    Installing,
    Failed {
        message: String,
    },
}

/// What the UI shows (Settings → Updates, the "ready" notice).
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    current: String,
    status: Status,
    /// This install can replace itself.
    can_install: bool,
    /// Why not, in a few words (e.g. "installed with a package manager").
    why_not: Option<String>,
    /// Where to download by hand.
    download_page: String,
}

#[derive(Default)]
pub struct Updates {
    status: Mutex<Option<Status>>,
    /// The newer version found by the last check.
    found: Mutex<Option<Update>>,
    /// Its downloaded, verified package.
    ready: Mutex<Option<(Update, Vec<u8>)>>,
    /// A version that couldn't be downloaded or installed: not tried again until the next start.
    by_hand: Mutex<Option<String>>,
    /// One check or download at a time.
    busy: tokio::sync::Mutex<()>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Whether this build knows the project's update signing key (release builds from CI do;
/// without it nothing could be verified, so there are no checks).
fn configured(app: &AppHandle) -> bool {
    app.config()
        .plugins
        .0
        .get("updater")
        .and_then(|u| u.get("pubkey"))
        .and_then(|k| k.as_str())
        .is_some_and(|k| !k.trim().is_empty())
}

/// Whether this copy of the app can install an update over itself.
fn install_support() -> Result<(), &'static str> {
    if cfg!(debug_assertions) {
        return Err("a development build");
    }
    let exe = std::env::current_exe().map_err(|_| "unknown location")?;
    #[cfg(target_os = "windows")]
    {
        // The installer puts its uninstaller next to the app; the portable zip has none.
        use tauri::utils::config::BundleType;
        let installed = tauri::utils::platform::bundle_type() == Some(BundleType::Nsis)
            && exe.parent().is_some_and(|d| d.join("uninstall.exe").exists());
        if !installed {
            return Err("the portable version");
        }
    }
    #[cfg(target_os = "macos")]
    {
        let path = exe.to_string_lossy();
        if !path.contains(".app/Contents/MacOS/") {
            return Err("not an app bundle");
        }
        if path.starts_with("/Volumes/") || path.contains("/AppTranslocation/") {
            return Err("running from the disk image");
        }
        // The .app is replaced in its folder (e.g. /Applications): it must be writable for us.
        let bundle = tauri_plugin_updater::extract_path_from_executable(&exe).map_err(|_| "unknown location")?;
        let folder = bundle.parent().ok_or("unknown location")?;
        let probe = folder.join(format!(".zorvik-update-{}", std::process::id()));
        std::fs::write(&probe, b"").map_err(|_| "its folder is read-only for this user")?;
        let _ = std::fs::remove_file(probe);
    }
    #[cfg(target_os = "linux")]
    {
        let _ = &exe;
        if std::env::var_os("APPIMAGE").is_none() {
            return Err("installed with a package manager");
        }
    }
    #[cfg(not(any(target_os = "windows", target_os = "macos", target_os = "linux")))]
    {
        let _ = &exe;
        return Err("not supported on this system");
    }
    Ok(())
}

/// When a nightly was built (the release tool sets it; local builds have none).
fn built_at() -> Option<time::OffsetDateTime> {
    let raw = option_env!("ZORVIK_BUILT_AT")?;
    time::OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).ok()
}

/// Whether `remote` is newer than this build. Nightlies carry the version of the code they
/// were built from, so a newer version always wins and, for the same version, a nightly built
/// later does (only on the nightly channel: a stable user never gets a nightly of their version).
fn is_newer(
    channel: Channel,
    current: &semver::Version,
    built: Option<time::OffsetDateTime>,
    remote_version: &semver::Version,
    remote_date: Option<time::OffsetDateTime>,
) -> bool {
    if remote_version != current {
        return remote_version > current;
    }
    channel == Channel::Nightly
        && match (remote_date, built) {
            (Some(remote), Some(built)) => remote > built,
            // A stable build of this version, and a nightly of it: the nightly is newer.
            (Some(_), None) => true,
            _ => false,
        }
}

impl Updates {
    fn status(&self) -> Status {
        lock(&self.status).clone().unwrap_or(Status::Idle)
    }

    fn set(&self, app: &AppHandle, status: Status) {
        *lock(&self.status) = Some(status);
        let _ = app.emit(EVENT, info(app));
    }

    /// Automatic install of this version didn't work: offer the download page instead.
    fn by_hand(&self, app: &AppHandle, update: &Update) {
        *lock(&self.by_hand) = Some(update.version.clone());
        self.set(app, available(update, true));
    }
}

fn available(update: &Update, by_hand: bool) -> Status {
    Status::Available {
        version: update.version.clone(),
        notes: update.body.clone(),
        date: update.date.and_then(|d| d.format(&time::format_description::well_known::Rfc3339).ok()),
        by_hand,
    }
}

pub fn info(app: &AppHandle) -> UpdateInfo {
    let why_not = if configured(app) { install_support().err() } else { Some("a build without update signing") };
    let why_not = why_not.map(str::to_string);
    UpdateInfo {
        current: app.package_info().version.to_string(),
        status: app.state::<Updates>().status(),
        can_install: why_not.is_none(),
        why_not,
        download_page: RELEASES_PAGE.into(),
    }
}

/// The update settings (and the proxy) the user chose.
async fn preferences(app: &AppHandle) -> (Mode, Channel, Option<url::Url>) {
    let api = app.state::<Api>().inner().clone();
    let settings = api.call("settings.get", serde_json::Value::Null).await.unwrap_or_default();
    let mode = match settings.pointer("/updates/mode").and_then(|v| v.as_str()) {
        Some("notify") => Mode::Notify,
        Some("off") => Mode::Off,
        _ => Mode::Automatic,
    };
    let channel = match settings.pointer("/updates/channel").and_then(|v| v.as_str()) {
        Some("nightly") => Channel::Nightly,
        _ => Channel::Stable,
    };
    (mode, channel, proxy_url(&settings))
}

/// The proxy the app's own requests use for github.com, if any.
fn proxy_url(settings: &serde_json::Value) -> Option<url::Url> {
    let mode: zorvik_engine::ProxyMode = serde_json::from_value(settings.get("proxy")?.clone()).ok()?;
    let proxy = zorvik_engine::ProxySettings::from_mode(&mode).ok()?;
    let endpoint = proxy.for_target("github.com", true)?;
    let mut url = url::Url::parse(&format!("http://{}:{}", endpoint.host, endpoint.port)).ok()?;
    if let Some(user) = &endpoint.username {
        let _ = url.set_username(user);
        let _ = url.set_password(endpoint.password.as_deref());
    }
    Some(url)
}

/// Look for a newer version; in automatic mode (or when asked), download it too.
pub async fn check(app: AppHandle, manual: bool) {
    let updates = app.state::<Updates>();
    let Ok(_one) = updates.busy.try_lock() else { return };
    if !configured(&app) {
        if manual {
            updates.set(
                &app,
                Status::Failed {
                    message: "This build can't check for updates (it was made without the project's update key)."
                        .into(),
                },
            );
        }
        return;
    }
    let (mode, channel, proxy) = preferences(&app).await;
    if mode == Mode::Off && !manual {
        return;
    }
    // A downloaded update waits for the restart; nothing new to fetch until then.
    if matches!(updates.status(), Status::Ready { .. }) && !manual {
        return;
    }
    if manual {
        updates.set(&app, Status::Checking);
    }
    let found = match find(&app, channel, proxy).await {
        Ok(found) => found,
        Err(e) => {
            tracing::info!("update check failed: {e}");
            // Background checks fail quietly (offline, a proxy); asking shows why.
            let status = if manual { Status::Failed { message: plain(&e) } } else { Status::Idle };
            updates.set(&app, status);
            return;
        }
    };
    let Some(update) = found else {
        updates.set(&app, Status::UpToDate { checked_at: now_ms() });
        return;
    };
    tracing::info!("Zorvik {} is available", update.version);
    let by_hand = lock(&updates.by_hand).as_deref() == Some(update.version.as_str());
    updates.set(&app, available(&update, by_hand));
    *lock(&updates.found) = Some(update);
    if mode == Mode::Automatic && !by_hand && install_support().is_ok() {
        download_found(&app).await;
    }
}

/// Download the version the last check found (Settings → "Download and install").
pub async fn download(app: AppHandle) {
    let updates = app.state::<Updates>();
    let Ok(_one) = updates.busy.try_lock() else { return };
    download_found(&app).await;
}

async fn download_found(app: &AppHandle) {
    let updates = app.state::<Updates>();
    let Some(update) = lock(&updates.found).clone() else { return };
    if install_support().is_err() {
        return;
    }
    let version = update.version.clone();
    updates.set(app, Status::Downloading { version: version.clone(), percent: Some(0) });
    let mut received = 0usize;
    let mut shown = 0u8;
    let progress = |chunk: usize, total: Option<u64>| {
        received += chunk;
        let percent = total.filter(|t| *t > 0).map(|t| ((received as u64 * 100) / t).min(100) as u8);
        // A few events, not one per chunk.
        if let Some(p) = percent
            && p >= shown.saturating_add(5)
        {
            shown = p;
            updates.set(app, Status::Downloading { version: version.clone(), percent: Some(p) });
        }
    };
    match update.download(progress, || {}).await {
        Ok(bytes) => {
            let notes = update.body.clone();
            *lock(&updates.ready) = Some((update, bytes));
            updates.set(app, Status::Ready { version, notes });
        }
        Err(e) => {
            // Offline, a network filter, or a signature that doesn't match: never installed.
            tracing::warn!("update download failed: {e}");
            updates.by_hand(app, &update);
        }
    }
}

async fn find(
    app: &AppHandle,
    channel: Channel,
    proxy: Option<url::Url>,
) -> Result<Option<Update>, tauri_plugin_updater::Error> {
    let url = std::env::var("ZORVIK_UPDATE_URL")
        .ok()
        .filter(|u| !u.is_empty())
        .unwrap_or_else(|| (if channel == Channel::Nightly { NIGHTLY_URL } else { STABLE_URL }).to_string());
    let endpoint = url::Url::parse(&url).map_err(|e| tauri_plugin_updater::Error::Io(std::io::Error::other(e)))?;
    let built = built_at();
    let mut builder = app.updater_builder().endpoints(vec![endpoint])?.timeout(TIMEOUT).version_comparator(
        move |current, remote: RemoteRelease| is_newer(channel, &current, built, &remote.version, remote.pub_date),
    );
    if let Some(proxy) = proxy {
        builder = builder.proxy(proxy);
    }
    builder.build()?.check().await
}

/// Install the downloaded update now and start the new version. On Windows the installer
/// takes over (a small progress window) and opens the app again.
pub fn install_now(app: &AppHandle) -> Result<(), String> {
    let updates = app.state::<Updates>();
    let Some((update, bytes)) = lock(&updates.ready).take() else {
        return Err("No update is ready".into());
    };
    updates.set(app, Status::Installing);
    // The Windows installer ends this process itself: stop things first.
    #[cfg(windows)]
    before_exit(app);
    if let Err(e) = update.install(&bytes) {
        // The app keeps running as it was; the notice links to the download instead.
        tracing::warn!("installing the update failed: {e}");
        updates.by_hand(app, &update);
        return Ok(());
    }
    // macOS and Linux: the new version is in place; start it.
    #[cfg(not(windows))]
    before_exit(app);
    app.restart();
}

/// Called as the app quits: a downloaded update is installed so the next start is the new
/// version (without starting it now).
pub fn install_on_quit(app: &AppHandle) {
    let Some(updates) = app.try_state::<Updates>() else { return };
    let Some((update, bytes)) = lock(&updates.ready).take() else { return };
    let update = update.restart_after_install(false);
    if let Err(e) = update.install(&bytes) {
        tracing::warn!("installing the update on quit failed: {e}");
    }
}

/// What quitting does anyway, before an installer takes over (it ends the process itself on
/// Windows): stop servers and load tests, close the agents' listener.
fn before_exit(app: &AppHandle) {
    if let Some(api) = app.try_state::<Api>() {
        api.stop_all_servers();
    }
    if let Some(listener) = app.try_state::<zorvik_mcp::listener::AgentListener>() {
        listener.shutdown();
    }
}

/// The background checks: soon after start, then every few hours.
pub fn start(app: &AppHandle) {
    let app = app.clone();
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(FIRST_CHECK).await;
        loop {
            check(app.clone(), false).await;
            tokio::time::sleep(CHECK_EVERY).await;
        }
    });
}

/// An error in a sentence for the UI.
fn plain(e: &tauri_plugin_updater::Error) -> String {
    use tauri_plugin_updater::Error as E;
    match e {
        E::ReleaseNotFound => "No release was found on GitHub.".into(),
        E::Reqwest(_) | E::Network(_) => "Could not reach GitHub. Check your connection or proxy.".into(),
        other => other.to_string(),
    }
}

fn now_ms() -> i64 {
    (time::OffsetDateTime::now_utc().unix_timestamp_nanos() / 1_000_000) as i64
}

// ---- commands ----------------------------------------------------------------------

#[tauri::command]
pub fn update_info(app: AppHandle) -> UpdateInfo {
    info(&app)
}

#[tauri::command]
pub async fn update_check(app: AppHandle) {
    check(app, true).await;
}

#[tauri::command]
pub async fn update_download(app: AppHandle) {
    download(app).await;
}

/// The UI asked the user first when servers, a load test or unsaved work would be lost.
#[tauri::command]
pub fn update_install(app: AppHandle) -> Result<(), String> {
    install_now(&app)
}

#[cfg(test)]
mod tests {
    use time::macros::datetime;

    use super::*;

    fn v(s: &str) -> semver::Version {
        semver::Version::parse(s).unwrap()
    }

    #[test]
    fn newer_versions_and_nightlies() {
        let (a, b) = (datetime!(2026-09-01 03:17 UTC), datetime!(2026-09-02 03:17 UTC));
        // A newer version always wins; an older never does.
        assert!(is_newer(Channel::Stable, &v("0.1.2"), None, &v("0.1.3"), None));
        assert!(!is_newer(Channel::Nightly, &v("0.1.3"), Some(b), &v("0.1.2"), Some(b)));
        // Same version on stable: nothing to do, even from a nightly.
        assert!(!is_newer(Channel::Stable, &v("0.1.2"), None, &v("0.1.2"), Some(b)));
        assert!(!is_newer(Channel::Stable, &v("0.1.2"), Some(a), &v("0.1.2"), Some(b)));
        // Nightly channel: a later build of the same version.
        assert!(is_newer(Channel::Nightly, &v("0.1.2"), Some(a), &v("0.1.2"), Some(b)));
        assert!(!is_newer(Channel::Nightly, &v("0.1.2"), Some(b), &v("0.1.2"), Some(b)), "the same build");
        assert!(!is_newer(Channel::Nightly, &v("0.1.2"), Some(b), &v("0.1.2"), Some(a)));
        // A stable build moving to nightly gets the nightly of its version.
        assert!(is_newer(Channel::Nightly, &v("0.1.2"), None, &v("0.1.2"), Some(a)));
        assert!(!is_newer(Channel::Nightly, &v("0.1.2"), None, &v("0.1.2"), None));
    }

    #[test]
    fn statuses_serialize_for_the_ui() {
        let s = serde_json::to_value(Status::Downloading { version: "1.0.0".into(), percent: Some(40) }).unwrap();
        assert_eq!(s, serde_json::json!({ "state": "downloading", "version": "1.0.0", "percent": 40 }));
        let s = serde_json::to_value(Status::UpToDate { checked_at: 5 }).unwrap();
        assert_eq!(s, serde_json::json!({ "state": "upToDate", "checkedAt": 5 }));
        let s =
            serde_json::to_value(Status::Available { version: "1.0.0".into(), notes: None, date: None, by_hand: true })
                .unwrap();
        assert_eq!(
            s,
            serde_json::json!({ "state": "available", "version": "1.0.0", "notes": null, "date": null, "byHand": true })
        );
    }
}
