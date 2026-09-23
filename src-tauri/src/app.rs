use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::platform::windows::portable;

/// Mutex to protect settings.json read/write operations (RD-M1-013).
/// Prevents data corruption from concurrent access.
static SETTINGS_MUTEX: Mutex<()> = Mutex::new(());

/// Minimum allowed window dimensions to ensure the window remains visible.
const MIN_WINDOW_WIDTH: f64 = 400.0;
const MIN_WINDOW_HEIGHT: f64 = 300.0;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppSettings {
    pub last_workspace: Option<String>,
    pub window_state: Option<WindowState>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowState {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
    pub maximized: bool,
}

/// Load settings from `Data/settings.json`.
/// Returns `AppSettings::default()` if the file doesn't exist or can't be parsed.
/// Protected by `SETTINGS_MUTEX` to prevent concurrent read/write corruption.
pub fn load_settings() -> AppSettings {
    let _guard = SETTINGS_MUTEX.lock().unwrap_or_else(|e| e.into_inner());
    load_settings_inner()
}

/// Inner implementation of settings loading (caller holds the lock).
fn load_settings_inner() -> AppSettings {
    let settings_path = match portable::resolve_settings_path() {
        Ok(p) => p,
        Err(_) => return AppSettings::default(),
    };

    if !settings_path.exists() {
        return AppSettings::default();
    }

    match std::fs::read_to_string(&settings_path) {
        Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

/// Save settings to `Data/settings.json`.
/// Creates the Data directory if it doesn't exist.
/// Protected by `SETTINGS_MUTEX` to prevent concurrent read/write corruption.
#[allow(dead_code)] // Public API, used by future features
pub fn save_settings(settings: &AppSettings) -> Result<(), String> {
    let _guard = SETTINGS_MUTEX
        .lock()
        .map_err(|e| format!("Mutex poisoned: {e}"))?;
    save_settings_inner(settings)
}

/// Inner implementation of settings saving (caller holds the lock).
fn save_settings_inner(settings: &AppSettings) -> Result<(), String> {
    let settings_path = portable::resolve_settings_path().map_err(|e| e.to_string())?;

    if let Some(parent) = settings_path.parent() {
        if !parent.exists() {
            std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
        }
    }

    let content = serde_json::to_string_pretty(settings).map_err(|e| e.to_string())?;
    std::fs::write(&settings_path, content).map_err(|e| e.to_string())?;
    Ok(())
}

/// Save window state from a Tauri window event.
/// Protected by `SETTINGS_MUTEX` to prevent concurrent read/write corruption.
pub fn save_window_state(window: &tauri::WebviewWindow) {
    let _guard = match SETTINGS_MUTEX.lock() {
        Ok(g) => g,
        Err(e) => e.into_inner(),
    };

    let Ok(settings_path) = portable::resolve_settings_path() else {
        return;
    };

    // Load existing settings or start fresh
    let mut settings = if settings_path.exists() {
        std::fs::read_to_string(&settings_path)
            .ok()
            .and_then(|c| serde_json::from_str(&c).ok())
            .unwrap_or_default()
    } else {
        AppSettings::default()
    };

    // Get current window state
    let Ok(outer_position) = window.outer_position() else {
        return;
    };
    let Ok(outer_size) = window.outer_size() else {
        return;
    };
    let maximized = window.is_maximized().unwrap_or(false);

    settings.window_state = Some(WindowState {
        x: outer_position.x as f64,
        y: outer_position.y as f64,
        width: outer_size.width as f64,
        height: outer_size.height as f64,
        maximized,
    });

    // Ensure Data directory exists
    if let Some(parent) = settings_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    if let Ok(content) = serde_json::to_string_pretty(&settings) {
        let _ = std::fs::write(&settings_path, content);
    }
}

/// Restore window state from settings, if available.
///
/// Performs boundary validation to ensure the window is visible on a connected
/// display. If the saved position is off-screen (e.g. disconnected monitor),
/// falls back to default position and size. If the window was maximized, only
/// restores the maximized state without setting specific coordinates.
pub fn restore_window_state(window: &tauri::WebviewWindow) {
    let settings = load_settings();
    if let Some(state) = settings.window_state {
        use tauri::{PhysicalPosition, PhysicalSize};

        // If the window was maximized, just maximize — no need to restore coordinates
        if state.maximized {
            let _ = window.maximize();
            return;
        }

        // Validate dimensions are reasonable
        let width = if state.width >= MIN_WINDOW_WIDTH {
            state.width as u32
        } else {
            1200 // fallback default from tauri.conf.json
        };
        let height = if state.height >= MIN_WINDOW_HEIGHT {
            state.height as u32
        } else {
            800 // fallback default from tauri.conf.json
        };

        // Validate position: ensure the restored window stays fully inside a
        // connected display's work area (saved state may come from a monitor
        // that is no longer attached or from a different resolution).
        let x = if state.x.is_finite() && state.x > -10000.0 && state.x < 10000.0 {
            state.x as i32
        } else {
            // Invalid coordinate, let the OS pick a default
            let _ = window.set_size(PhysicalSize::new(width, height));
            return;
        };
        let y = if state.y.is_finite() && state.y > -10000.0 && state.y < 10000.0 {
            state.y as i32
        } else {
            let _ = window.set_size(PhysicalSize::new(width, height));
            return;
        };

        let (x, y) = clamp_to_work_area(window, x, y, width, height);
        let _ = window.set_position(PhysicalPosition::new(x, y));
        let _ = window.set_size(PhysicalSize::new(width, height));
    }
}

/// Clamp a window rectangle into the work area of the monitor containing its
/// top-left corner, so a restored window can never end up (partly) off-screen.
fn clamp_to_work_area(window: &tauri::WebviewWindow, x: i32, y: i32, width: u32, height: u32) -> (i32, i32) {
    let Ok(monitors) = window.available_monitors() else {
        return (x, y);
    };
    let Some(monitor) = monitors
        .iter()
        .find(|m| {
            let area = m.work_area();
            let (ax, ay) = (area.position.x, area.position.y);
            let (aw, ah) = (area.size.width as i32, area.size.height as i32);
            x >= ax && x < ax + aw && y >= ay && y < ay + ah
        })
        .or_else(|| monitors.first())
    else {
        return (x, y);
    };

    let area = monitor.work_area();
    let (ax, ay) = (area.position.x, area.position.y);
    let (aw, ah) = (area.size.width as i32, area.size.height as i32);
    let cx = if width as i32 >= aw { ax } else { x.clamp(ax, ax + aw - width as i32) };
    let cy = if height as i32 >= ah { ay } else { y.clamp(ay, ay + ah - height as i32) };
    (cx, cy)
}

/// Configure WebView2 user data directory for portable mode (RD-M1-009).
///
/// Sets the `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS` environment variable to
/// include `--user-data-dir=<Data/webview2>`, ensuring WebView2 data is stored
/// in the portable Data directory instead of `%LOCALAPPDATA%`.
///
/// Must be called BEFORE any WebView2 environment is created (i.e. before
/// `tauri::Builder::run()`).
pub fn configure_webview2_user_data_dir() {
    let Ok(udf_path) = portable::resolve_webview2_udf() else {
        return;
    };

    // Ensure the target directory exists
    if let Err(e) = std::fs::create_dir_all(&udf_path) {
        eprintln!("Failed to create WebView2 user data directory: {e}");
        return;
    }

    let user_data_dir = udf_path.to_string_lossy().to_string();
    let args_flag = format!("--user-data-dir={user_data_dir}");

    // Append to any existing arguments rather than overwriting
    let value = if let Ok(existing) = std::env::var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS") {
        if existing.is_empty() {
            args_flag
        } else {
            format!("{existing} {args_flag}")
        }
    } else {
        args_flag
    };

    std::env::set_var("WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS", &value);
}

/// Detect whether WebView2 Runtime is available on this system.
///
/// Checks:
/// 1. Bundled fixed version in `Data/webview2/`
/// 2. Evergreen runtime via `reg query` command
///
/// Returns `true` if the runtime is found.
pub fn detect_webview2_runtime() -> bool {
    // 1. Check if we have a bundled fixed version
    if let Ok(data_dir) = portable::resolve_data_dir() {
        let fixed_path = data_dir.join("webview2").join("msedgewebview2.exe");
        if fixed_path.exists() {
            return true;
        }
    }

    // 2. Check registry for Evergreen runtime via `reg query`
    let wv2_guid = "{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}";
    let reg_paths = [
        format!(r"HKLM\SOFTWARE\Microsoft\EdgeUpdate\Clients\{wv2_guid}"),
        format!(r"HKLM\SOFTWARE\WOW6432Node\Microsoft\EdgeUpdate\Clients\{wv2_guid}"),
        format!(r"HKCU\SOFTWARE\Microsoft\EdgeUpdate\Clients\{wv2_guid}"),
    ];

    for reg_path in &reg_paths {
        if let Ok(output) = std::process::Command::new("reg")
            .args(["query", reg_path])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            if output.success() {
                return true;
            }
        }
    }

    false
}

/// Set up the global panic hook to write crash logs to `Data/logs/crash.log`.
pub fn setup_panic_hook() {
    let logs_dir = match portable::resolve_logs_dir() {
        Ok(dir) => dir,
        Err(_) => return,
    };

    // Ensure logs directory exists
    let _ = std::fs::create_dir_all(&logs_dir);

    let logs_dir_clone = logs_dir.clone();
    let default_hook = std::panic::take_hook();

    std::panic::set_hook(Box::new(move |info| {
        let timestamp = timestamp_now();
        let crash_log_path = logs_dir_clone.join("crash.log");

        let panic_info = format!(
            "[{timestamp}] PANIC: {info}\nLocation: {}\n---\n",
            info.location()
                .map(|l| format!("{}:{}:{}", l.file(), l.line(), l.column()))
                .unwrap_or_else(|| "unknown".to_string()),
        );

        // Append to crash log
        use std::io::Write;
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&crash_log_path)
        {
            let _ = file.write_all(panic_info.as_bytes());
        }

        // Also call the default hook
        default_hook(info);
    }));
}

/// Simple Unix-timestamp-based time string (avoids chrono dependency).
fn timestamp_now() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format!("{}.{:03}", now.as_secs(), now.subsec_millis())
}
