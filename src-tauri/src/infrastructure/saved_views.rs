//! M9: Saved Views — structured predicates, versioned persistence, evaluator
//! (RD-M9-022~030).
//!
//! Saved views are DISPOSABLE application state (like the rest of the SQLite
//! index): losing them never loses business data. They are stored in the
//! pre-existing `saved_views` table with a VERSIONED JSON payload so future
//! predicate-model changes can be migrated.
//!
//! Model:
//! - [`ViewPredicate`] `{ field, operator, value }` — one leaf condition.
//! - [`PredicateNode`] — `Leaf` / `And` / `Or` composition tree.
//! - [`compile_sql`] turns a node into a parameterized WHERE clause over
//!   `task_index` (tags/participants via EXISTS subqueries).
//! - [`PredicateNode::matches`] is the in-memory evaluator used for
//!   client-side filtering and cross-checks.
//! - CRUD helpers persist views per workspace; [`evaluate_saved_view`]
//!   returns the matching task rows.

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::domain::smart_view::{naive_date_to_ole, parse_index_date};
use crate::domain::types::TaskKey;

/// Bump when the JSON payload layout changes incompatibly.
pub const PREDICATE_SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Error)]
pub enum SavedViewError {
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Unknown predicate field: {0}")]
    UnknownField(String),

    #[error("Unsupported value for field {field}: {value}")]
    InvalidValue { field: String, value: String },

    #[error("Saved view not found: {0}")]
    NotFound(String),

    #[error("Unsupported saved-view payload version {found} (max {supported})")]
    UnsupportedVersion { found: u32, supported: u32 },

    #[error("View name must not be empty")]
    EmptyName,
}

pub type SavedViewResult<T> = Result<T, SavedViewError>;

/// Fields a predicate can reference.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateField {
    Title,
    Status,
    Priority,
    PercentDone,
    Risk,
    StartDate,
    DueDate,
    CompletedDate,
    DocumentId,
    Tag,
    Participant,
}

/// Comparison operators.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateOperator {
    Equals,
    NotEquals,
    Contains,
    NotContains,
    StartsWith,
    GreaterThan,
    GreaterOrEqual,
    LessThan,
    LessOrEqual,
    In,
    NotIn,
    IsNull,
    NotNull,
}

/// One leaf condition.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewPredicate {
    pub field: PredicateField,
    pub operator: PredicateOperator,
    /// JSON value: string / number / bool / array (for In/NotIn) / null.
    pub value: serde_json::Value,
}

impl ViewPredicate {
    pub fn new(
        field: PredicateField,
        operator: PredicateOperator,
        value: serde_json::Value,
    ) -> Self {
        Self { field, operator, value }
    }
}

/// AND/OR composition tree.
///
/// Serialization is externally tagged (`{"and":[...]}` / `{"or":[...]}` /
/// `{"leaf":{...}}`) so And and Or nodes remain distinguishable on load.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateNode {
    And(Vec<PredicateNode>),
    Or(Vec<PredicateNode>),
    Leaf(ViewPredicate),
}

impl PredicateNode {
    /// AND-compose a flat list of conditions (canonical filter form).
    pub fn all_of(nodes: Vec<PredicateNode>) -> Self {
        PredicateNode::And(nodes)
    }

    /// OR-compose a flat list of conditions.
    pub fn any_of(nodes: Vec<PredicateNode>) -> Self {
        PredicateNode::Or(nodes)
    }

    /// In-memory evaluation against a task row (with its tags/participants).
    pub fn matches(&self, row: &ViewRow) -> bool {
        match self {
            PredicateNode::And(children) => children.iter().all(|c| c.matches(row)),
            PredicateNode::Or(children) => children.iter().any(|c| c.matches(row)),
            PredicateNode::Leaf(p) => match p.field {
                PredicateField::Title => compare_text(&row.title, p.operator, &p.value),
                PredicateField::Status => compare_text(&row.status, p.operator, &p.value),
                PredicateField::DocumentId => {
                    compare_text(&row.document_id, p.operator, &p.value)
                }
                PredicateField::Priority => compare_num(f64::from(row.priority), p.operator, &p.value),
                PredicateField::PercentDone => {
                    compare_num(f64::from(row.percent_done), p.operator, &p.value)
                }
                PredicateField::Risk => compare_num(f64::from(row.risk), p.operator, &p.value),
                PredicateField::StartDate => {
                    compare_date(row.start_date.as_deref(), p.operator, &p.value)
                }
                PredicateField::DueDate => {
                    compare_date(row.due_date.as_deref(), p.operator, &p.value)
                }
                PredicateField::CompletedDate => {
                    compare_date(row.completed_date.as_deref(), p.operator, &p.value)
                }
                PredicateField::Tag => compare_multi(&row.tags, p.operator, &p.value),
                PredicateField::Participant => {
                    compare_multi(&row.participants, p.operator, &p.value)
                }
            },
        }
    }
}

// ── Versioned serialization (RD-M9-023~024) ─────────────────────────────────

/// The versioned envelope stored in `saved_views.predicates_json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionedPredicates {
    pub version: u32,
    pub root: PredicateNode,
}

/// Serialize a predicate tree with the current schema version.
pub fn serialize_predicates(root: &PredicateNode) -> SavedViewResult<String> {
    let envelope = VersionedPredicates {
        version: PREDICATE_SCHEMA_VERSION,
        root: root.clone(),
    };
    Ok(serde_json::to_string(&envelope)?)
}

/// Deserialize predicates; handles the table default `'{}'` (match-all) and
/// rejects unknown future versions (version is checked BEFORE the payload so
/// newer files report UnsupportedVersion instead of a parse error).
pub fn deserialize_predicates(json: &str) -> SavedViewResult<PredicateNode> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "{}" {
        // Legacy/default payload → match everything.
        return Ok(PredicateNode::And(Vec::new()));
    }
    #[derive(Deserialize)]
    struct RawEnvelope {
        version: u32,
        root: serde_json::Value,
    }
    let raw: RawEnvelope = serde_json::from_str(trimmed)?;
    if raw.version == 0 || raw.version > PREDICATE_SCHEMA_VERSION {
        return Err(SavedViewError::UnsupportedVersion {
            found: raw.version,
            supported: PREDICATE_SCHEMA_VERSION,
        });
    }
    // Future: migrate older versions here (v1 → v2 → ...).
    Ok(serde_json::from_value(raw.root)?)
}

// ── SQL compilation (RD-M9-025~027) ─────────────────────────────────────────

/// A bindable SQL parameter value.
#[derive(Debug, Clone, PartialEq)]
pub enum SqlValue {
    Text(String),
    Real(f64),
    Null,
}

impl rusqlite::types::ToSql for SqlValue {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(match self {
            SqlValue::Text(s) => rusqlite::types::ToSqlOutput::from(s.as_str()),
            SqlValue::Real(f) => rusqlite::types::ToSqlOutput::from(*f),
            SqlValue::Null => rusqlite::types::ToSqlOutput::from(rusqlite::types::Null),
        })
    }
}

/// Compile a predicate tree into a WHERE clause over `task_index` (alias `t`)
/// plus the ordered list of bind parameters. Placeholders are numbered
/// `?1..?N` within the compiled fragment.
pub fn compile_sql(node: &PredicateNode) -> SavedViewResult<(String, Vec<SqlValue>)> {
    compile_sql_with_offset(node, 0)
}

/// Like [`compile_sql`], but shifts every placeholder by `offset` so the
/// fragment can be embedded in a statement that already binds parameters
/// (e.g. `evaluate` binds the workspace id as `?1`).
pub fn compile_sql_with_offset(
    node: &PredicateNode,
    offset: usize,
) -> SavedViewResult<(String, Vec<SqlValue>)> {
    let mut params = Vec::new();
    let sql = compile_node(node, &mut params, offset)?;
    Ok((sql, params))
}

fn compile_node(
    node: &PredicateNode,
    params: &mut Vec<SqlValue>,
    offset: usize,
) -> SavedViewResult<String> {
    match node {
        PredicateNode::And(children) if children.is_empty() => Ok("1=1".to_string()),
        PredicateNode::Or(children) if children.is_empty() => Ok("1=0".to_string()),
        PredicateNode::And(children) => {
            let parts = children
                .iter()
                .map(|c| compile_node(c, params, offset))
                .collect::<SavedViewResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" AND ")))
        }
        PredicateNode::Or(children) => {
            let parts = children
                .iter()
                .map(|c| compile_node(c, params, offset))
                .collect::<SavedViewResult<Vec<_>>>()?;
            Ok(format!("({})", parts.join(" OR ")))
        }
        PredicateNode::Leaf(p) => compile_leaf(p, params, offset),
    }
}

fn compile_leaf(
    p: &ViewPredicate,
    params: &mut Vec<SqlValue>,
    offset: usize,
) -> SavedViewResult<String> {
    use PredicateField as F;
    use PredicateOperator as O;

    // NULL checks first — they take no value.
    match p.operator {
        O::IsNull | O::NotNull => {
            let col = scalar_column(p.field)?;
            let null_sql = match p.field {
                F::StartDate | F::DueDate | F::CompletedDate => {
                    format!("({col} IS NULL OR {col} = '')")
                }
                _ => format!("{col} IS NULL"),
            };
            return Ok(match p.operator {
                O::IsNull => null_sql,
                _ => format!("NOT {null_sql}"),
            });
        }
        _ => {}
    }

    match p.field {
        F::Tag => compile_membership("task_tags", "tag", p, params, offset),
        F::Participant => {
            compile_membership("task_participants", "participant", p, params, offset)
        }
        F::StartDate | F::DueDate | F::CompletedDate => {
            let col = scalar_column(p.field)?;
            let ole = value_as_date_ole(p)?;
            match p.operator {
                O::Equals => numeric(format!("CAST({col} AS REAL) = {ole}")),
                O::NotEquals => numeric(format!("NOT ({col} IS NULL OR {col} = '') AND CAST({col} AS REAL) <> {ole}")),
                O::GreaterThan => numeric(format!("CAST({col} AS REAL) > {ole}")),
                O::GreaterOrEqual => numeric(format!("CAST({col} AS REAL) >= {ole}")),
                O::LessThan => numeric(format!("NOT ({col} IS NULL OR {col} = '') AND CAST({col} AS REAL) < {ole}")),
                O::LessOrEqual => numeric(format!("NOT ({col} IS NULL OR {col} = '') AND CAST({col} AS REAL) <= {ole}")),
                other => Err(SavedViewError::InvalidValue {
                    field: "date".into(),
                    value: format!("operator {:?} not supported for dates", other),
                }),
            }
        }
        F::Priority | F::PercentDone | F::Risk => {
            let col = scalar_column(p.field)?;
            let num = value_as_f64(p)?;
            params.push(SqlValue::Real(num));
            let pos = params.len() + offset;
            let cmp = match p.operator {
                O::Equals => "=",
                O::NotEquals => "<>",
                O::GreaterThan => ">",
                O::GreaterOrEqual => ">=",
                O::LessThan => "<",
                O::LessOrEqual => "<=",
                other => {
                    return Err(SavedViewError::InvalidValue {
                        field: format!("{:?}", p.field),
                        value: format!("operator {:?} not supported for numbers", other),
                    })
                }
            };
            Ok(format!("CAST({col} AS REAL) {cmp} ?{pos}"))
        }
        F::Title | F::Status | F::DocumentId => {
            let col = scalar_column(p.field)?;
            match p.operator {
                O::Equals => {
                    let text = value_as_str(p)?;
                    params.push(SqlValue::Text(text));
                    Ok(format!("{col} = ?{}", params.len() + offset))
                }
                O::NotEquals => {
                    let text = value_as_str(p)?;
                    params.push(SqlValue::Text(text));
                    Ok(format!("{col} <> ?{}", params.len() + offset))
                }
                O::Contains | O::NotContains => {
                    let text = value_as_str(p)?;
                    params.push(SqlValue::Text(format!("%{}%", escape_like(&text))));
                    let sql = format!("{col} LIKE ?{} ESCAPE '\\'", params.len() + offset);
                    Ok(if p.operator == O::NotContains {
                        format!("NOT {sql}")
                    } else {
                        sql
                    })
                }
                O::StartsWith => {
                    let text = value_as_str(p)?;
                    params.push(SqlValue::Text(format!("{}%", escape_like(&text))));
                    Ok(format!("{col} LIKE ?{} ESCAPE '\\'", params.len() + offset))
                }
                O::In | O::NotIn => {
                    let values = value_as_str_list(p)?;
                    if values.is_empty() {
                        return Ok(if p.operator == O::In { "1=0".into() } else { "1=1".into() });
                    }
                    let mut marks = Vec::new();
                    for v in values {
                        params.push(SqlValue::Text(v));
                        marks.push(format!("?{}", params.len() + offset));
                    }
                    let sql = format!("{col} IN ({})", marks.join(", "));
                    Ok(if p.operator == O::NotIn { format!("NOT {sql}") } else { sql })
                }
                _ => Err(SavedViewError::InvalidValue {
                    field: format!("{:?}", p.field),
                    value: format!("operator {:?} not supported for text", p.operator),
                }),
            }
        }
    }
}

fn compile_membership(
    table: &str,
    column: &str,
    p: &ViewPredicate,
    params: &mut Vec<SqlValue>,
    offset: usize,
) -> SavedViewResult<String> {
    let text = value_as_str(p)?;
    let cond = match p.operator {
        PredicateOperator::Equals => {
            params.push(SqlValue::Text(text));
            format!("{table}.{column} = ?{}", params.len() + offset)
        }
        PredicateOperator::NotEquals => {
            params.push(SqlValue::Text(text));
            format!("{table}.{column} <> ?{}", params.len() + offset)
        }
        PredicateOperator::Contains | PredicateOperator::NotContains => {
            params.push(SqlValue::Text(format!("%{}%", escape_like(&text))));
            format!(
                "{table}.{column} LIKE ?{} ESCAPE '\\'",
                params.len() + offset
            )
        }
        other => {
            return Err(SavedViewError::InvalidValue {
                field: format!("{:?}", p.field),
                value: format!("operator {:?} not supported for tag/participant", other),
            })
        }
    };
    let exists = format!(
        "EXISTS (SELECT 1 FROM {table} WHERE {table}.task_key = t.task_key \
         AND {table}.document_id = t.document_id AND {cond})"
    );
    Ok(match p.operator {
        PredicateOperator::NotEquals | PredicateOperator::NotContains => format!("NOT {exists}"),
        _ => exists,
    })
}

fn numeric(s: String) -> SavedViewResult<String> {
    Ok(s)
}

fn scalar_column(field: PredicateField) -> SavedViewResult<&'static str> {
    Ok(match field {
        PredicateField::Title => "t.title",
        PredicateField::Status => "t.status",
        PredicateField::Priority => "t.priority",
        PredicateField::PercentDone => "t.percent_done",
        PredicateField::Risk => "t.risk",
        PredicateField::StartDate => "t.start_date",
        PredicateField::DueDate => "t.due_date",
        PredicateField::CompletedDate => "t.completed_date",
        PredicateField::DocumentId => "t.document_id",
        PredicateField::Tag | PredicateField::Participant => {
            return Err(SavedViewError::UnknownField(format!("{field:?}")))
        }
    })
}

fn escape_like(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_")
}

fn value_as_str(p: &ViewPredicate) -> SavedViewResult<String> {
    match &p.value {
        serde_json::Value::String(s) => Ok(s.clone()),
        serde_json::Value::Number(n) => Ok(n.to_string()),
        serde_json::Value::Bool(b) => Ok(b.to_string()),
        other => Err(SavedViewError::InvalidValue {
            field: format!("{:?}", p.field),
            value: other.to_string(),
        }),
    }
}

fn value_as_str_list(p: &ViewPredicate) -> SavedViewResult<Vec<String>> {
    match &p.value {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|v| match v {
                serde_json::Value::String(s) => Ok(s.clone()),
                serde_json::Value::Number(n) => Ok(n.to_string()),
                other => Err(SavedViewError::InvalidValue {
                    field: format!("{:?}", p.field),
                    value: other.to_string(),
                }),
            })
            .collect(),
        single => Ok(vec![value_as_str(&ViewPredicate {
            field: p.field,
            operator: p.operator,
            value: single.clone(),
        })?]),
    }
}

fn value_as_f64(p: &ViewPredicate) -> SavedViewResult<f64> {
    match &p.value {
        serde_json::Value::Number(n) => n.as_f64().ok_or(SavedViewError::InvalidValue {
            field: format!("{:?}", p.field),
            value: n.to_string(),
        }),
        serde_json::Value::String(s) => s.parse::<f64>().map_err(|_| {
            SavedViewError::InvalidValue {
                field: format!("{:?}", p.field),
                value: s.clone(),
            }
        }),
        other => Err(SavedViewError::InvalidValue {
            field: format!("{:?}", p.field),
            value: other.to_string(),
        }),
    }
}

/// Date values are given as ISO strings ("2024-01-15") or OLE numbers;
/// compiled to the OLE float stored in the index.
fn value_as_date_ole(p: &ViewPredicate) -> SavedViewResult<f64> {
    match &p.value {
        serde_json::Value::Number(n) => n.as_f64().ok_or(SavedViewError::InvalidValue {
            field: format!("{:?}", p.field),
            value: n.to_string(),
        }),
        serde_json::Value::String(s) => {
            let date = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").map_err(|_| {
                SavedViewError::InvalidValue {
                    field: format!("{:?}", p.field),
                    value: s.clone(),
                }
            })?;
            Ok(naive_date_to_ole(date))
        }
        other => Err(SavedViewError::InvalidValue {
            field: format!("{:?}", p.field),
            value: other.to_string(),
        }),
    }
}

// ── In-memory comparison helpers ─────────────────────────────────────────────

fn compare_text(actual: &str, op: PredicateOperator, value: &serde_json::Value) -> bool {
    let want = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    match op {
        PredicateOperator::Equals => actual == want,
        PredicateOperator::NotEquals => actual != want,
        PredicateOperator::Contains => actual.to_lowercase().contains(&want.to_lowercase()),
        PredicateOperator::NotContains => !actual.to_lowercase().contains(&want.to_lowercase()),
        PredicateOperator::StartsWith => actual.to_lowercase().starts_with(&want.to_lowercase()),
        PredicateOperator::GreaterThan => actual > want.as_str(),
        PredicateOperator::GreaterOrEqual => actual >= want.as_str(),
        PredicateOperator::LessThan => actual < want.as_str(),
        PredicateOperator::LessOrEqual => actual <= want.as_str(),
        PredicateOperator::In | PredicateOperator::NotIn => value
            .as_array()
            .map(|items| {
                let hit = items.iter().any(|v| v.as_str() == Some(actual));
                if op == PredicateOperator::In { hit } else { !hit }
            })
            .unwrap_or(false),
        PredicateOperator::IsNull => actual.is_empty(),
        PredicateOperator::NotNull => !actual.is_empty(),
    }
}

fn compare_num(actual: f64, op: PredicateOperator, value: &serde_json::Value) -> bool {
    let Some(want) = value.as_f64().or_else(|| {
        value.as_str().and_then(|s| s.parse::<f64>().ok())
    }) else {
        return matches!(op, PredicateOperator::NotEquals);
    };
    match op {
        PredicateOperator::Equals => (actual - want).abs() < f64::EPSILON,
        PredicateOperator::NotEquals => (actual - want).abs() >= f64::EPSILON,
        PredicateOperator::GreaterThan => actual > want,
        PredicateOperator::GreaterOrEqual => actual >= want,
        PredicateOperator::LessThan => actual < want,
        PredicateOperator::LessOrEqual => actual <= want,
        _ => false,
    }
}

fn compare_date(actual_raw: Option<&str>, op: PredicateOperator, value: &serde_json::Value) -> bool {
    let actual = actual_raw.and_then(|s| parse_index_date(Some(s)));
    let want = match value {
        serde_json::Value::String(s) => chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d").ok(),
        serde_json::Value::Number(n) => n
            .as_f64()
            .and_then(crate::domain::smart_view::ole_to_naive_date),
        _ => None,
    };
    match op {
        PredicateOperator::IsNull => actual.is_none(),
        PredicateOperator::NotNull => actual.is_some(),
        _ => match (actual, want) {
            (Some(a), Some(w)) => match op {
                PredicateOperator::Equals => a == w,
                PredicateOperator::NotEquals => a != w,
                PredicateOperator::GreaterThan => a > w,
                PredicateOperator::GreaterOrEqual => a >= w,
                PredicateOperator::LessThan => a < w,
                PredicateOperator::LessOrEqual => a <= w,
                _ => false,
            },
            (None, _) => matches!(op, PredicateOperator::NotEquals),
            _ => false,
        },
    }
}

fn compare_multi(actual: &[String], op: PredicateOperator, value: &serde_json::Value) -> bool {
    let want = match value {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    let lower = want.to_lowercase();
    match op {
        PredicateOperator::Equals => actual.iter().any(|v| v == &want),
        PredicateOperator::NotEquals => !actual.iter().any(|v| v == &want),
        PredicateOperator::Contains => {
            actual.iter().any(|v| v.to_lowercase().contains(&lower))
        }
        PredicateOperator::NotContains => {
            !actual.iter().any(|v| v.to_lowercase().contains(&lower))
        }
        _ => false,
    }
}

// ── Rows and evaluation ──────────────────────────────────────────────────────

/// A task row as seen by saved-view evaluation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ViewRow {
    pub task_key: String,
    pub document_id: String,
    pub title: String,
    pub status: String,
    pub priority: i32,
    pub percent_done: i32,
    pub risk: i32,
    pub start_date: Option<String>,
    pub due_date: Option<String>,
    pub completed_date: Option<String>,
    pub tags: Vec<String>,
    pub participants: Vec<String>,
}

impl ViewRow {
    pub fn key(&self) -> TaskKey {
        TaskKey::new(
            crate::domain::types::DocumentId::new(&self.document_id),
            crate::domain::types::TaskId::new(&self.task_key),
        )
    }
}

/// A persisted saved view.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SavedView {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub predicates: PredicateNode,
    pub sort_order: i64,
    pub created_at: String,
}

/// Evaluate a predicate tree directly in SQLite over the whole workspace.
pub fn evaluate(
    conn: &Connection,
    workspace_id: &str,
    node: &PredicateNode,
) -> SavedViewResult<Vec<ViewRow>> {
    let (where_sql, values) = compile_sql_with_offset(node, 1)?;
    let sql = format!(
        "SELECT t.task_key, t.document_id, t.title, t.status, t.priority, t.percent_done, \
                t.risk, t.start_date, t.due_date, t.completed_date \
         FROM task_index t \
         JOIN documents d ON d.id = t.document_id \
         WHERE d.workspace_id = ?1 AND ({where_sql}) \
         ORDER BY t.document_id ASC, t.task_key ASC"
    );
    let mut stmt = conn.prepare(&sql)?;
    let mut params: Vec<Box<dyn rusqlite::types::ToSql>> =
        vec![Box::new(workspace_id.to_string())];
    for v in values {
        params.push(Box::new(v));
    }
    let params_ref: Vec<&dyn rusqlite::types::ToSql> = params.iter().map(|p| p.as_ref()).collect();

    let rows = stmt.query_map(params_ref.as_slice(), |row| {
        Ok(RawRow {
            task_key: row.get(0)?,
            document_id: row.get(1)?,
            title: row.get(2)?,
            status: row.get(3)?,
            priority: row.get(4)?,
            percent_done: row.get::<_, f64>(5)? as i32,
            risk: row.get(6)?,
            start_date: row.get(7)?,
            due_date: row.get(8)?,
            completed_date: row.get(9)?,
        })
    })?;

    let mut out = Vec::new();
    for raw in rows {
        let raw = raw?;
        out.push(ViewRow {
            tags: fetch_multi(
                conn,
                "SELECT tag FROM task_tags WHERE task_key = ?1 AND document_id = ?2 ORDER BY tag",
                &raw.task_key,
                &raw.document_id,
            ),
            participants: fetch_multi(
                conn,
                "SELECT DISTINCT participant FROM task_participants \
                 WHERE task_key = ?1 AND document_id = ?2 ORDER BY participant",
                &raw.task_key,
                &raw.document_id,
            ),
            task_key: raw.task_key,
            document_id: raw.document_id,
            title: raw.title,
            status: raw.status,
            priority: raw.priority,
            percent_done: raw.percent_done,
            risk: raw.risk,
            start_date: raw.start_date,
            due_date: raw.due_date,
            completed_date: raw.completed_date,
        });
    }
    Ok(out)
}

struct RawRow {
    task_key: String,
    document_id: String,
    title: String,
    status: String,
    priority: i32,
    percent_done: i32,
    risk: i32,
    start_date: Option<String>,
    due_date: Option<String>,
    completed_date: Option<String>,
}

fn fetch_multi(conn: &Connection, sql: &str, task_key: &str, document_id: &str) -> Vec<String> {
    let mut stmt = match conn.prepare(sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    stmt.query_map([task_key, document_id], |r| r.get::<_, String>(0))
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
}

// ── CRUD (RD-M9-028~030) ─────────────────────────────────────────────────────

/// Create a saved view from the current filter state (a predicate tree).
pub fn create_view(
    conn: &Connection,
    workspace_id: &str,
    name: &str,
    predicates: &PredicateNode,
) -> SavedViewResult<SavedView> {
    if name.trim().is_empty() {
        return Err(SavedViewError::EmptyName);
    }
    let id = uuid::Uuid::new_v4().to_string();
    let json = serialize_predicates(predicates)?;
    let next_order: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(sort_order), 0) + 1 FROM saved_views WHERE workspace_id = ?1",
            [workspace_id],
            |r| r.get(0),
        )
        .unwrap_or(1);
    conn.execute(
        "INSERT INTO saved_views (id, workspace_id, name, predicates_json, sort_order) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![id, workspace_id, name.trim(), json, next_order],
    )?;
    get_view(conn, &id)?.ok_or(SavedViewError::NotFound(id))
}

/// List a workspace's saved views in sidebar order.
pub fn list_views(conn: &Connection, workspace_id: &str) -> SavedViewResult<Vec<SavedView>> {
    let mut stmt = conn.prepare(
        "SELECT id, workspace_id, name, predicates_json, sort_order, created_at \
         FROM saved_views WHERE workspace_id = ?1 ORDER BY sort_order ASC, created_at ASC, id ASC",
    )?;
    let rows = stmt.query_map([workspace_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, String>(2)?,
            row.get::<_, String>(3)?,
            row.get::<_, i64>(4)?,
            row.get::<_, String>(5)?,
        ))
    })?;
    let mut views = Vec::new();
    for row in rows {
        let (id, ws, name, json, order, created) = row?;
        views.push(SavedView {
            id,
            workspace_id: ws,
            name,
            predicates: deserialize_predicates(&json)?,
            sort_order: order,
            created_at: created,
        });
    }
    Ok(views)
}

/// Fetch one saved view.
pub fn get_view(conn: &Connection, id: &str) -> SavedViewResult<Option<SavedView>> {
    let row: Option<(String, String, String, String, i64, String)> = conn
        .query_row(
            "SELECT id, workspace_id, name, predicates_json, sort_order, created_at \
             FROM saved_views WHERE id = ?1",
            [id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    match row {
        Some((id, workspace_id, name, json, sort_order, created_at)) => Ok(Some(SavedView {
            id,
            workspace_id,
            name,
            predicates: deserialize_predicates(&json)?,
            sort_order,
            created_at,
        })),
        None => Ok(None),
    }
}

/// Rename a saved view.
pub fn rename_view(conn: &Connection, id: &str, new_name: &str) -> SavedViewResult<()> {
    if new_name.trim().is_empty() {
        return Err(SavedViewError::EmptyName);
    }
    let n = conn.execute(
        "UPDATE saved_views SET name = ?2 WHERE id = ?1",
        rusqlite::params![id, new_name.trim()],
    )?;
    if n == 0 {
        return Err(SavedViewError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Replace a view's predicate tree (re-saving the current filter state).
pub fn update_view(
    conn: &Connection,
    id: &str,
    predicates: &PredicateNode,
) -> SavedViewResult<()> {
    let json = serialize_predicates(predicates)?;
    let n = conn.execute(
        "UPDATE saved_views SET predicates_json = ?2 WHERE id = ?1",
        rusqlite::params![id, json],
    )?;
    if n == 0 {
        return Err(SavedViewError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Delete a saved view.
pub fn delete_view(conn: &Connection, id: &str) -> SavedViewResult<()> {
    let n = conn.execute("DELETE FROM saved_views WHERE id = ?1", [id])?;
    if n == 0 {
        return Err(SavedViewError::NotFound(id.to_string()));
    }
    Ok(())
}

/// Persist a new sidebar ordering.
pub fn reorder_views(conn: &Connection, ordered_ids: &[String]) -> SavedViewResult<()> {
    for (i, id) in ordered_ids.iter().enumerate() {
        conn.execute(
            "UPDATE saved_views SET sort_order = ?2 WHERE id = ?1",
            rusqlite::params![id, (i + 1) as i64],
        )?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::infrastructure::migration;
    use chrono::NaiveDate;
    use serde_json::json;

    fn setup() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        migration::run_migrations(&conn).unwrap();
        conn.execute(
            "INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'WS', '/tmp')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO documents (id, workspace_id, file_path) VALUES ('doc1', 'ws1', 'a.tdl')",
            [],
        )
        .unwrap();
        // 2024-01-15 = OLE 45306; 2024-06-01 = OLE 45444
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, due_date) \
             VALUES ('1', 'doc1', 'Alpha task', 7, 'NotStarted', 0, '45306')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_index (task_key, document_id, title, priority, status, percent_done, due_date, completed_date) \
             VALUES ('2', 'doc1', 'Beta 任务', 2, 'Done', 100, '45444', '45444')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_tags (task_key, document_id, tag) VALUES ('1', 'doc1', 'work')",
            [],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task_participants (task_key, document_id, participant, role) \
             VALUES ('1', 'doc1', 'Alice', 'allocated_to')",
            [],
        )
        .unwrap();
        conn
    }

    fn leaf(field: PredicateField, op: PredicateOperator, v: serde_json::Value) -> PredicateNode {
        PredicateNode::Leaf(ViewPredicate::new(field, op, v))
    }

    #[test]
    fn versioned_roundtrip() {
        let node = PredicateNode::all_of(vec![
            leaf(PredicateField::Title, PredicateOperator::Contains, json!("alpha")),
            PredicateNode::any_of(vec![
                leaf(PredicateField::Priority, PredicateOperator::GreaterOrEqual, json!(7)),
                leaf(PredicateField::Tag, PredicateOperator::Equals, json!("work")),
            ]),
        ]);
        let json_str = serialize_predicates(&node).unwrap();
        assert!(json_str.contains("\"version\":1"));
        assert_eq!(deserialize_predicates(&json_str).unwrap(), node);
    }

    #[test]
    fn default_payload_is_match_all() {
        let node = deserialize_predicates("{}").unwrap();
        assert_eq!(node, PredicateNode::And(vec![]));
        let (sql, params) = compile_sql(&node).unwrap();
        assert_eq!(sql, "1=1");
        assert!(params.is_empty());
    }

    #[test]
    fn future_version_rejected() {
        let bad = r#"{"version":99,"root":{"field":"title","operator":"equals","value":"x"}}"#;
        assert!(matches!(
            deserialize_predicates(bad),
            Err(SavedViewError::UnsupportedVersion { found: 99, .. })
        ));
    }

    #[test]
    fn sql_evaluation_matches_rows() {
        let conn = setup();
        let node = PredicateNode::all_of(vec![
            leaf(PredicateField::Status, PredicateOperator::Equals, json!("NotStarted")),
            leaf(PredicateField::Priority, PredicateOperator::GreaterOrEqual, json!(7)),
        ]);
        let rows = evaluate(&conn, "ws1", &node).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_key, "1");
        assert_eq!(rows[0].tags, vec!["work"]);
        assert_eq!(rows[0].participants, vec!["Alice"]);
    }

    #[test]
    fn sql_and_memory_agree() {
        let conn = setup();
        let node = PredicateNode::any_of(vec![
            leaf(PredicateField::Tag, PredicateOperator::Equals, json!("work")),
            leaf(PredicateField::Title, PredicateOperator::Contains, json!("任务")),
        ]);
        let rows = evaluate(&conn, "ws1", &node).unwrap();
        assert_eq!(rows.len(), 2);
        // In-memory evaluator must agree with SQL on the same rows.
        for row in &rows {
            assert!(node.matches(row), "memory evaluator disagrees on {}", row.task_key);
        }
        let all = evaluate(&conn, "ws1", &PredicateNode::And(vec![])).unwrap();
        let mem_hits: Vec<&ViewRow> = all.iter().filter(|r| node.matches(r)).collect();
        assert_eq!(mem_hits.len(), rows.len());
    }

    #[test]
    fn date_predicate_compiles_to_ole() {
        let conn = setup();
        let node = leaf(
            PredicateField::DueDate,
            PredicateOperator::LessOrEqual,
            json!("2024-01-15"),
        );
        let rows = evaluate(&conn, "ws1", &node).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_key, "1");

        let (sql, _) = compile_sql(&node).unwrap();
        assert!(sql.contains("45306"), "compiled SQL was: {sql}");

        // In-memory agreement
        assert!(rows.iter().all(|r| node.matches(r)));
    }

    #[test]
    fn null_predicates() {
        let conn = setup();
        let node = leaf(PredicateField::CompletedDate, PredicateOperator::IsNull, json!(null));
        let rows = evaluate(&conn, "ws1", &node).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].task_key, "1");
    }

    #[test]
    fn participant_predicate() {
        let conn = setup();
        let node = leaf(PredicateField::Participant, PredicateOperator::Equals, json!("Alice"));
        let rows = evaluate(&conn, "ws1", &node).unwrap();
        assert_eq!(rows.len(), 1);
        let node2 = leaf(PredicateField::Participant, PredicateOperator::Equals, json!("Bob"));
        assert!(evaluate(&conn, "ws1", &node2).unwrap().is_empty());
    }

    #[test]
    fn crud_lifecycle_and_restart_survival() {
        let dir = std::env::temp_dir().join(format!("mtdl_sv_{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let db_path = dir.join("views.db");

        let node = leaf(PredicateField::Title, PredicateOperator::Contains, json!("alpha"));
        let id;
        {
            let conn = Connection::open(&db_path).unwrap();
            migration::run_migrations(&conn).unwrap();
            conn.execute(
                "INSERT INTO workspaces (id, name, root_path) VALUES ('ws1', 'WS', '/tmp')",
                [],
            )
            .unwrap();
            let view = create_view(&conn, "ws1", "My Filter", &node).unwrap();
            id = view.id.clone();
            rename_view(&conn, &id, "Renamed Filter").unwrap();
        }
        // "Restart": reopen the file database.
        {
            let conn = Connection::open(&db_path).unwrap();
            let views = list_views(&conn, "ws1").unwrap();
            assert_eq!(views.len(), 1);
            assert_eq!(views[0].name, "Renamed Filter");
            assert_eq!(views[0].predicates, node);
            delete_view(&conn, &id).unwrap();
            assert!(list_views(&conn, "ws1").unwrap().is_empty());
            assert!(matches!(
                delete_view(&conn, &id),
                Err(SavedViewError::NotFound(_))
            ));
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn empty_name_rejected_and_reorder() {
        let conn = setup();
        assert!(matches!(
            create_view(&conn, "ws1", "  ", &PredicateNode::And(vec![])),
            Err(SavedViewError::EmptyName)
        ));
        let a = create_view(&conn, "ws1", "A", &PredicateNode::And(vec![])).unwrap();
        let b = create_view(&conn, "ws1", "B", &PredicateNode::And(vec![])).unwrap();
        assert!(a.sort_order < b.sort_order);
        reorder_views(&conn, &[b.id.clone(), a.id.clone()]).unwrap();
        let views = list_views(&conn, "ws1").unwrap();
        assert_eq!(views[0].id, b.id);
    }

    #[test]
    fn invalid_values_reported() {
        let bad = leaf(PredicateField::Priority, PredicateOperator::Equals, json!("high"));
        // "high" is not numeric → compile fails cleanly, no SQL injection risk.
        assert!(matches!(
            compile_sql(&bad),
            Err(SavedViewError::InvalidValue { .. })
        ));
        let bad_date = leaf(PredicateField::DueDate, PredicateOperator::Equals, json!("nope"));
        assert!(compile_sql(&bad_date).is_err());
    }

    #[test]
    fn in_operator_and_text_ops() {
        let conn = setup();
        let node = leaf(
            PredicateField::Status,
            PredicateOperator::In,
            json!(["NotStarted", "InProgress"]),
        );
        assert_eq!(evaluate(&conn, "ws1", &node).unwrap().len(), 1);
        let node = leaf(PredicateField::Title, PredicateOperator::StartsWith, json!("Beta"));
        assert_eq!(evaluate(&conn, "ws1", &node).unwrap().len(), 1);
    }

    #[test]
    fn memory_date_boundaries() {
        let row = ViewRow {
            task_key: "1".into(),
            document_id: "d".into(),
            title: "t".into(),
            status: "NotStarted".into(),
            priority: 5,
            percent_done: 0,
            risk: 0,
            start_date: None,
            due_date: Some(
                naive_date_to_ole(NaiveDate::from_ymd_opt(2024, 1, 15).unwrap()).to_string(),
            ),
            completed_date: None,
            tags: vec![],
            participants: vec![],
        };
        let before = leaf(PredicateField::DueDate, PredicateOperator::LessThan, json!("2024-01-16"));
        let same = leaf(PredicateField::DueDate, PredicateOperator::Equals, json!("2024-01-15"));
        let after = leaf(PredicateField::DueDate, PredicateOperator::GreaterThan, json!("2024-01-14"));
        assert!(before.matches(&row));
        assert!(same.matches(&row));
        assert!(after.matches(&row));
        assert!(!leaf(PredicateField::DueDate, PredicateOperator::LessThan, json!("2024-01-15")).matches(&row));
    }
}
