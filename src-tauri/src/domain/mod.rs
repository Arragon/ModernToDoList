//! Domain layer for ModernToDoList 2.0
//!
//! This module contains the core domain types for the lossless XML processing pipeline,
//! data safety infrastructure, and undo/redo command system.

// M2: Lossless XML Core
pub mod encoding;
pub mod id_allocator;
pub mod mappers;
pub mod task;
pub mod types;
pub mod validator;
pub mod xml_parser;
pub mod xml_serializer;
pub mod xml_tree;

// M3: Data Safety Core
pub mod command;
pub mod fingerprint;
pub mod persistence;
pub mod recovery;
pub mod session;

// M4: Workspace Data Platform
pub mod workspace;

// Encoding layer
#[allow(unused_imports)]
pub use encoding::{detect_bom, BomDetection, LineEnding, XmlEncoding, XmlEncodingMeta};

// Domain IDs
#[allow(unused_imports)]
pub use types::{DocumentId, TaskId, TaskKey, WorkspaceId};

// XML infrastructure
#[allow(unused_imports)]
pub use xml_parser::{parse_xml, unescape_xml, escape_xml, XmlParseError};
#[allow(unused_imports)]
pub use xml_serializer::serialize_xml;
#[allow(unused_imports)]
pub use xml_tree::{XmlAttribute, XmlDocument, XmlElement, XmlNode};

// Task domain
#[allow(unused_imports)]
pub use task::{
    Task, TaskStatus, TaskPriority, TaskDependency, TaskComment, CommentType,
    TaskFileLink, TaskCategory, TaskMetadata, DocumentMetadata, TaskTree,
};

// Mappers
#[allow(unused_imports)]
pub use mappers::{read_task, write_task, read_document_metadata};

// ID allocator
#[allow(unused_imports)]
pub use id_allocator::TaskIdAllocator;

// Validator
#[allow(unused_imports)]
pub use validator::{validate_document, validate_task_tree, ValidationError, XmlCoreError};

// Data Safety
#[allow(unused_imports)]
pub use fingerprint::{FileFingerprint, FingerprintError};
#[allow(unused_imports)]
pub use session::{DocumentSession, SaveState, SaveErrorCode, StaleRevisionError};
#[allow(unused_imports)]
pub use persistence::{atomic_save, SaveConfig, SaveResult, AutosaveCoordinator};
#[allow(unused_imports)]
pub use recovery::{RecoveryJournal, RecoveryJournalEntry, RecoveryPhase, RecoveryAction, SafetyEventLog};

// Undo/Redo
#[allow(unused_imports)]
pub use command::{UndoableCommand, UndoRedoManager, FieldUpdateCommand, TaskField, FieldValue};

// Workspace
#[allow(unused_imports)]
pub use workspace::{Workspace, WorkspaceMetadata, WorkspaceError, DocumentEntry, DocumentType, RecentWorkspace};

// M6: Task Relations (TEMPORARY validation wiring — remove before handoff)
pub mod attachment;
pub mod dependency;
pub mod participant;
pub mod progress_link;

pub mod comments;
pub mod quick_add;
pub mod sanitizer;
pub mod search;
pub mod smart_view;

pub mod file_library;
pub mod rich_text;
pub mod transfer;
pub mod transfer_journal;
pub mod trash;
