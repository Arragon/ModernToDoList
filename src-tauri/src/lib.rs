mod app;
mod application;
mod commands;
pub mod domain;
pub mod infrastructure;
mod platform;

pub mod benchmark;
pub mod release;
pub mod diagnostics;

use commands::document::{
    allocate_task_id, get_document_metadata, read_and_parse_document,
    serialize_and_write_document, validate_document_cmd,
};
use commands::session::{
    close_document_session, get_session_status, open_document_session,
    redo_last_command, save_document_atomic, undo_last_command, AppState,
};
use commands::system::{get_runtime_info, ping};
use commands::workspace::{
    WorkspaceState, create_workspace, open_workspace, close_workspace,
    get_workspace_status, list_documents, scan_and_index, rebuild_index, get_db_status,
};
use commands::task_query::{query_tasks, get_task_tags};
use commands::search::{
    get_shortcut_conflicts, list_app_commands, palette_query, rebuild_search_index_cmd,
    resolve_task_jump, search_tasks,
};
use commands::views::{
    create_saved_view, delete_saved_view, evaluate_saved_view, get_smart_view, list_saved_views,
    rename_saved_view, reorder_saved_views, update_saved_view,
};
use commands::quick_add::{
    get_global_shortcut_settings, parse_quick_add, quick_add_build_task,
    set_global_quick_add_shortcut,
};
// M6/M9 IPC bridge: task editing, relations, search/quick-add, attachments.
use commands::task_edit::{
    add_task, delete_task, global_search, quick_add_task, set_task_tags, update_task_field,
};
use commands::relations::{
    add_dependency, add_managed_attachment, add_participant, add_progress_link,
    add_url_attachment, get_dependencies, get_participants, link_local_attachment,
    list_attachments, list_dependencies, list_participants, list_progress_links,
    list_task_participants, remove_attachment, remove_dependency, remove_participant,
    remove_progress_link, update_attachment, update_progress_link,
};
use commands::bridge::{open_attachment, reveal_attachment};
// Re-exported so `tests/ipc_bridge_tests.rs` (an external integration test) can
// drive the pure `*_core` bridge logic against the public domain types; the
// `commands` module itself stays crate-private.
pub use commands::{bridge, relations, task_edit};
use tauri::Manager;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Set up panic hook for crash logging (RD-M1-020)
    app::setup_panic_hook();

    // Ensure Data directory exists (RD-M1-008)
    if let Err(e) = platform::windows::portable::ensure_data_dir() {
        eprintln!("Failed to create Data directory: {e}");
        // In a real scenario, we'd show a dialog here. For now, log and continue.
    }

    // Set WebView2 user data directory to portable Data/webview2/ (RD-M1-009)
    // Must be done BEFORE any WebView2 environment is created.
    app::configure_webview2_user_data_dir();

    // WebView2 Runtime detection (RD-M1-016)
    if !app::detect_webview2_runtime() {
        eprintln!(
            "WebView2 Runtime not detected. The application may not function correctly.\n\
             Please install WebView2 Runtime from Microsoft."
        );
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            // Focus the first window when a second instance is launched
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_focus();
            }
        }))
        .manage(AppState::new())
        .manage({
            let data_dir = platform::windows::portable::resolve_data_dir()
                .unwrap_or_else(|_| std::path::PathBuf::from("Data"));
            WorkspaceState::new(&data_dir)
        })
        .invoke_handler(tauri::generate_handler![
            // System commands
            ping,
            get_runtime_info,
            // M2: Document commands
            read_and_parse_document,
            serialize_and_write_document,
            get_document_metadata,
            validate_document_cmd,
            allocate_task_id,
            // M3: Session commands
            open_document_session,
            close_document_session,
            save_document_atomic,
            get_session_status,
            undo_last_command,
            redo_last_command,
            // M4: Workspace commands
            create_workspace,
            open_workspace,
            close_workspace,
            get_workspace_status,
            list_documents,
            scan_and_index,
            rebuild_index,
            get_db_status,
            // M5: Task query commands
            query_tasks,
            get_task_tags,
            // M9: Global search, command palette and shortcut conflicts
            search_tasks,
            rebuild_search_index_cmd,
            palette_query,
            list_app_commands,
            get_shortcut_conflicts,
            resolve_task_jump,
            // M9: Smart views and saved views
            get_smart_view,
            create_saved_view,
            list_saved_views,
            rename_saved_view,
            update_saved_view,
            delete_saved_view,
            reorder_saved_views,
            evaluate_saved_view,
            // M9: Quick Add and global shortcut
            parse_quick_add,
            quick_add_build_task,
            get_global_shortcut_settings,
            set_global_quick_add_shortcut,
            // M6 bridge: task mutation
            update_task_field,
            add_task,
            delete_task,
            set_task_tags,
            // M6 bridge: participants
            get_participants,
            list_participants,
            list_task_participants,
            add_participant,
            remove_participant,
            // M6 bridge: dependencies
            get_dependencies,
            list_dependencies,
            add_dependency,
            remove_dependency,
            // M6 bridge: progress links
            list_progress_links,
            add_progress_link,
            update_progress_link,
            remove_progress_link,
            // M6 bridge: attachments
            list_attachments,
            add_managed_attachment,
            link_local_attachment,
            add_url_attachment,
            update_attachment,
            remove_attachment,
            open_attachment,
            reveal_attachment,
            // M9 bridge: global search + quick add insertion
            global_search,
            quick_add_task,
        ])
        .setup(|app| {
            // Restore window state on startup (RD-M1-013)
            if let Some(window) = app.get_webview_window("main") {
                app::restore_window_state(&window);
            }
            Ok(())
        })
        .on_window_event(|window, event| {
            // Save window state on close (RD-M1-013)
            if let tauri::WindowEvent::CloseRequested { .. } = event {
                // Get the webview window from the window reference
                let label = window.label().to_string();
                if let Some(webview_window) = window.get_webview_window(&label) {
                    app::save_window_state(&webview_window);
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
