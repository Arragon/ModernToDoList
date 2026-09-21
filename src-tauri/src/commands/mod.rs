pub mod document;
pub mod session;
pub mod system;
pub mod task_query;
pub mod workspace;

pub mod quick_add;
pub mod search;
pub mod views;

// M6/M9 IPC bridge (task editing, relations, search/quick-add, attachments).
pub mod bridge;
pub mod relations;
pub mod task_edit;
