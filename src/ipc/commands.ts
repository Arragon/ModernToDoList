export const Commands = {
  // -- M1: System --
  PING: "ping",
  GET_RUNTIME_INFO: "get_runtime_info",

  // -- M2: Document --
  READ_AND_PARSE_DOCUMENT: "read_and_parse_document",
  SERIALIZE_AND_WRITE_DOCUMENT: "serialize_and_write_document",
  GET_DOCUMENT_METADATA: "get_document_metadata",
  VALIDATE_DOCUMENT_CMD: "validate_document_cmd",
  ALLOCATE_TASK_ID: "allocate_task_id",

  // -- M3: Session --
  OPEN_DOCUMENT_SESSION: "open_document_session",
  CLOSE_DOCUMENT_SESSION: "close_document_session",
  SAVE_DOCUMENT_ATOMIC: "save_document_atomic",
  GET_SESSION_STATUS: "get_session_status",
  UNDO_LAST_COMMAND: "undo_last_command",
  REDO_LAST_COMMAND: "redo_last_command",

  // -- M4: Workspace --
  CREATE_WORKSPACE: "create_workspace",
  OPEN_WORKSPACE: "open_workspace",
  CLOSE_WORKSPACE: "close_workspace",
  GET_WORKSPACE_STATUS: "get_workspace_status",
  LIST_DOCUMENTS: "list_documents",
  SCAN_AND_INDEX: "scan_and_index",
  REBUILD_INDEX: "rebuild_index",
  GET_DB_STATUS: "get_db_status",

  // -- M5: Task Query --
  QUERY_TASKS: "query_tasks",
  GET_TASK_TAGS: "get_task_tags",

  // -- M6: Task Mutation --
  UPDATE_TASK_FIELD: "update_task_field",
  DELETE_TASK: "delete_task",
  ADD_TASK: "add_task",
  SET_TASK_TAGS: "set_task_tags",

  // -- M6: Participants --
  GET_PARTICIPANTS: "get_participants",
  LIST_PARTICIPANTS: "list_participants",
  LIST_TASK_PARTICIPANTS: "list_task_participants",
  ADD_PARTICIPANT: "add_participant",
  REMOVE_PARTICIPANT: "remove_participant",

  // -- M6: Dependencies --
  GET_DEPENDENCIES: "get_dependencies",
  LIST_DEPENDENCIES: "list_dependencies",
  ADD_DEPENDENCY: "add_dependency",
  REMOVE_DEPENDENCY: "remove_dependency",

  // -- M6: Progress Links --
  LIST_PROGRESS_LINKS: "list_progress_links",
  ADD_PROGRESS_LINK: "add_progress_link",
  UPDATE_PROGRESS_LINK: "update_progress_link",
  REMOVE_PROGRESS_LINK: "remove_progress_link",

  // -- M6: Attachments --
  LIST_ATTACHMENTS: "list_attachments",
  ADD_MANAGED_ATTACHMENT: "add_managed_attachment",
  LINK_LOCAL_ATTACHMENT: "link_local_attachment",
  ADD_URL_ATTACHMENT: "add_url_attachment",
  UPDATE_ATTACHMENT: "update_attachment",
  REMOVE_ATTACHMENT: "remove_attachment",
  OPEN_ATTACHMENT: "open_attachment",
  REVEAL_ATTACHMENT: "reveal_attachment",

  // -- M7: Task comments / description --
  GET_TASK_COMMENTS: "get_task_comments",

  // -- M9: Productivity --
  GLOBAL_SEARCH: "global_search",
  GET_SMART_VIEW: "get_smart_view",
  LIST_SAVED_VIEWS: "list_saved_views",
  CREATE_SAVED_VIEW: "create_saved_view",
  RENAME_SAVED_VIEW: "rename_saved_view",
  UPDATE_SAVED_VIEW: "update_saved_view",
  DELETE_SAVED_VIEW: "delete_saved_view",
  REORDER_SAVED_VIEWS: "reorder_saved_views",
  QUICK_ADD_TASK: "quick_add_task",
  GET_GLOBAL_SHORTCUT_SETTINGS: "get_global_shortcut_settings",
  SET_GLOBAL_QUICK_ADD_SHORTCUT: "set_global_quick_add_shortcut",
  GET_SHORTCUT_CONFLICTS: "get_shortcut_conflicts",

  // -- M8: Cross-document transfer --
  LIST_TRANSFER_TARGETS: "list_transfer_targets",
  PREVIEW_TRANSFER: "preview_transfer",
  COPY_TASK_BETWEEN_DOCUMENTS: "copy_task_between_documents",
  MOVE_TASK_BETWEEN_DOCUMENTS: "move_task_between_documents",
  UNDO_TRANSFER: "undo_transfer",
  RECOVER_TRANSFERS: "recover_transfers",
  COMPLETE_INTERRUPTED_MOVE: "complete_interrupted_move",

  // -- Platform plugins (already registered in src-tauri) --
  SHELL_OPEN: "plugin:shell|open",
} as const;
