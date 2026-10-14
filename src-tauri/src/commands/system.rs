use serde::Serialize;

use crate::platform::windows::portable;

#[derive(Debug, Clone, Serialize)]
pub struct RuntimeInfo {
    pub version: String,
    pub data_dir: String,
    pub webview2_udf: String,
    pub portable_root: String,
}

#[tauri::command]
pub fn ping() -> String {
    "pong".to_string()
}

#[tauri::command]
pub fn get_runtime_info() -> RuntimeInfo {
    let data_dir = portable::resolve_data_dir()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let webview2_udf = portable::resolve_webview2_udf()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();
    let portable_root = portable::resolve_portable_root()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    RuntimeInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        data_dir,
        webview2_udf,
        portable_root,
    }
}
