//! Infrastructure layer for ModernToDoList 2.0
//!
//! Contains database, file watching, and other infrastructure concerns.

// M4: SQLite Database
pub mod db;
pub mod indexer;
pub mod migration;
pub mod schema;
pub mod watcher;

// Re-exports
#[allow(unused_imports)]
pub use db::{DatabaseManager, DatabaseMode, DatabaseError};
#[allow(unused_imports)]
pub use migration::{run_migrations, get_current_version, MigrationError};
#[allow(unused_imports)]
pub use indexer::{index_document, rebuild_index, clear_workspace_index, IndexProgress, IndexError};
#[allow(unused_imports)]
pub use watcher::{WorkspaceWatcher, WatcherConfig, FileChangeEvent, ChangeType, ExternalConflict, FingerprintChecker};
