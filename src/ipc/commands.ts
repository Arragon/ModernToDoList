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
} as const;
