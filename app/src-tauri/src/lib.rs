//! Tauri shell: one `rpc` command forwards to `zorvik_api::Api`, and API events
//! are emitted to the webview as `zv:event`. AI agents reach the same `Api`
//! through `zorvik mcp` and the listener of `zorvik_mcp` (docs/architecture.md, "AI agents").

#[cfg(target_os = "macos")]
mod traffic_lights;
mod updates;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Emitter, Manager, RunEvent, WindowEvent};
use tauri_plugin_opener::OpenerExt;
use zorvik_api::{Api, ApiError, BatchingSink, EventSink, StreamEvent};

struct TauriSink(AppHandle);

impl EventSink for TauriSink {
    fn emit(&self, event: StreamEvent) {
        if let Err(e) = self.0.emit("zv:event", event) {
            tracing::warn!("failed to emit event: {e}");
        }
    }

    fn open_url(&self, url: &str) -> Result<(), String> {
        self.0.opener().open_url(url, None::<&str>).map_err(|e| e.to_string())
    }
}

/// Set once the user confirmed quitting (servers are stopped by then).
static QUITTING: AtomicBool = AtomicBool::new(false);

/// Menu item id of "Quit Zorvik" (⌘Q).
const QUIT_MENU_ID: &str = "zv-quit";

/// Longest wait on quit for a stopped load test to end (requests in flight
/// get up to 5 s), so its result is saved to the history.
const LOAD_TEST_STOP_WAIT: std::time::Duration = std::time::Duration::from_secs(7);

/// Quit after the UI confirmed it: stop every server and the load test first.
#[tauri::command]
async fn quit(app: AppHandle) {
    QUITTING.store(true, Ordering::SeqCst);
    let api = app.state::<Api>().inner().clone();
    api.stop_all_servers();
    api.stop_load_test_and_wait(LOAD_TEST_STOP_WAIT).await;
    app.exit(0);
}

/// Closing the window (or ⌘Q) while servers run asks the UI first.
fn must_confirm_quit(app: &AppHandle) -> bool {
    if QUITTING.load(Ordering::SeqCst) {
        return false;
    }
    let Some(api) = app.try_state::<Api>() else { return false };
    if api.running_server_count() == 0 && !api.load_test_running() {
        return false;
    }
    api.request_quit();
    true
}

#[tauri::command]
async fn rpc(
    api: tauri::State<'_, Api>,
    method: String,
    params: serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    api.call(&method, params).await
}

fn init_logging(dir: &std::path::Path) -> Option<tracing_appender::non_blocking::WorkerGuard> {
    let _ = std::fs::create_dir_all(dir);
    let appender = tracing_appender::rolling::Builder::new()
        .rotation(tracing_appender::rolling::Rotation::DAILY)
        .max_log_files(7)
        .filename_prefix("zorvik")
        .filename_suffix("log")
        .build(dir)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_writer(writer)
        .with_ansi(false)
        .try_init()
        .ok()?;
    Some(guard)
}

/// macOS menu without "Close Window" on ⌘W (the UI uses ⌘W to close a tab).
/// The Edit menu is required for copy/paste shortcuts in WKWebView.
/// Quit is our own item: the predefined one terminates the app right away
/// (no `ExitRequested`), so running servers would stop without asking.
#[cfg(target_os = "macos")]
fn app_menu(app: &tauri::AppHandle) -> tauri::Result<tauri::menu::Menu<tauri::Wry>> {
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};
    let app_menu = Submenu::with_items(
        app,
        "Zorvik",
        true,
        &[
            &PredefinedMenuItem::about(app, None, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::hide(app, None)?,
            &PredefinedMenuItem::hide_others(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &MenuItem::with_id(app, QUIT_MENU_ID, "Quit Zorvik", true, Some("CmdOrCtrl+Q"))?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &PredefinedMenuItem::separator(app)?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;
    Menu::with_items(app, &[&app_menu, &edit, &window])
}

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(target_os = "macos")]
    let builder = builder.menu(app_menu);
    builder
        // Registered first, as the plugin requires.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.unminimize();
                let _ = window.show();
                let _ = window.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .manage(updates::Updates::default())
        .setup(|app| {
            let handle = app.handle().clone();
            if let Ok(log_dir) = handle.path().app_log_dir()
                && let Some(guard) = init_logging(&log_dir)
            {
                app.manage(guard);
            }
            let data_dir = handle.path().app_data_dir().expect("app data dir");
            tracing::info!("Zorvik {} starting, data dir {}", env!("CARGO_PKG_VERSION"), data_dir.display());
            let api = Api::new(data_dir.clone(), Arc::new(BatchingSink::new(Arc::new(TauriSink(handle)))));
            api.restore_last_workspace();
            match tauri::async_runtime::block_on(zorvik_mcp::listener::start(api.clone(), &data_dir)) {
                Ok(listener) => {
                    app.manage(listener);
                }
                Err(e) => tracing::warn!("AI agents can't connect: {e}"),
            }
            app.manage(api);
            updates::start(app.handle());
            #[cfg(target_os = "macos")]
            for window in app.webview_windows().values() {
                traffic_lights::center(&window.as_ref().window());
            }
            Ok(())
        })
        // Goes through `ExitRequested` below, which asks first while servers run.
        .on_menu_event(|app, event| {
            if event.id() == QUIT_MENU_ID {
                app.exit(0);
            }
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event
                && must_confirm_quit(window.app_handle())
            {
                api.prevent_close();
            }
            #[cfg(target_os = "macos")]
            if matches!(
                event,
                WindowEvent::Resized(_)
                    | WindowEvent::Focused(_)
                    | WindowEvent::ThemeChanged(_)
                    | WindowEvent::ScaleFactorChanged { .. }
            ) {
                traffic_lights::center(window);
            }
        })
        .invoke_handler(tauri::generate_handler![
            rpc,
            quit,
            updates::update_info,
            updates::update_check,
            updates::update_download,
            updates::update_install
        ])
        .build(tauri::generate_context!())
        .expect("error while building Zorvik")
        .run(|app, event| match event {
            RunEvent::ExitRequested { api, .. } if must_confirm_quit(app) => api.prevent_exit(),
            RunEvent::Exit => {
                if let Some(listener) = app.try_state::<zorvik_mcp::listener::AgentListener>() {
                    listener.shutdown();
                }
                // A downloaded update goes in now, so the next start is the new version.
                updates::install_on_quit(app);
            }
            _ => {}
        });
}
