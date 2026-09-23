import {
  BarChart,
  Callout,
  ChartContainer,
  Divider,
  Grid,
  H1,
  H3,
  MetricsGrid,
  ReportSection,
  ReportShell,
  Stack,
  Table,
  Text,
  Timeline,
} from "qoder/canvas";

const testBreakdown = [
  { label: "Unit Tests", value: 207 },
  { label: "M4 QA", value: 16 },
  { label: "M5 QA", value: 16 },
  { label: "Round-trip", value: 17 },
  { label: "Doc-tests", value: 1 },
];

const newFiles = [
  { file: "src-tauri/src/commands/task_query.rs", desc: "Task query IPC (query_tasks, get_task_tags)" },
  { file: "src-tauri/tests/m5_qa_tests.rs", desc: "16 M5 QA integration tests" },
  { file: "src/stores/task-store.ts", desc: "Task state: tree flattening, visible rows, navigation" },
  { file: "src/stores/filter-state.ts", desc: "Filter state model + multi-select state" },
  { file: "src/app/shortcuts.ts", desc: "Keyboard shortcut registration (11 bindings)" },
  { file: "src/components/task-tree/TaskTree.vue", desc: "Task tree list with loading/empty states" },
  { file: "src/components/task-tree/TaskRow.vue", desc: "Task row: checkbox, priority, dates, expand" },
  { file: "src/components/task-tree/TaskFilter.vue", desc: "Filter bar: keyword, status, due date" },
  { file: "src/components/inspector/Inspector.vue", desc: "Inspector shell with all editors" },
  { file: "src/components/inspector/TitleEditor.vue", desc: "Title input with 400ms debounce" },
  { file: "src/components/inspector/StatusEditor.vue", desc: "Status dropdown (4 states)" },
  { file: "src/components/inspector/PriorityEditor.vue", desc: "Priority dropdown (5 levels)" },
  { file: "src/components/inspector/DateEditor.vue", desc: "Date picker for start/due dates" },
  { file: "src/components/inspector/TagsEditor.vue", desc: "Multi-value chip input with Enter" },
  { file: "src/components/inspector/MoreProperties.vue", desc: "Collapsible secondary properties" },
];

const fileColumns = [
  { key: "file", title: "File Path", role: "label" as const },
  { key: "desc", title: "Description", role: "description" as const },
];

const modifiedFiles = [
  { file: "src/ipc/commands.ts", change: "Added QUERY_TASKS, GET_TASK_TAGS commands" },
  { file: "src/ipc/types.ts", change: "Added TaskSummary, TaskQueryResult types" },
  { file: "src/components/layout/TaskTreePanel.vue", change: "Integrated TaskTree + TaskFilter" },
  { file: "src/components/layout/InspectorPanel.vue", change: "Integrated Inspector component" },
  { file: "src/App.vue", change: "Added initKeyboardShortcuts() on mount" },
];

const changeColumns = [
  { key: "file", title: "File Path", role: "label" as const },
  { key: "change", title: "Change", role: "description" as const },
];

const qaResults = [
  { result: "207 / 207", suite: "Unit tests", coverage: "All domain, infrastructure, platform tests pass" },
  { result: "16 / 16", suite: "M4 QA tests", coverage: "SQLite, workspace, indexing, file watch, recovery" },
  { result: "16 / 16", suite: "M5 QA tests", coverage: "Task query, hierarchy, tags, filter, GATE-M5" },
  { result: "17 / 17", suite: "Round-trip tests", coverage: "XML serialize/decode round-trip integrity" },
  { result: "1 / 1", suite: "Doc-tests", coverage: "Encoding detection doc-test" },
];

const qaColumns = [
  { key: "result", title: "Result", role: "label" as const },
  { key: "suite", title: "Suite", role: "label" as const },
  { key: "coverage", title: "Coverage", role: "description" as const },
];

const phaseTimeline = [
  { id: "p0", timestamp: "Phase 0", title: "Linear Sync & Build Verification", description: "Synced M2/M3 QA+GATE to Done, verified 172 tests passing", state: "completed" as const },
  { id: "p1", timestamp: "Phase 1", title: "M4 SQLite Infrastructure", description: "rusqlite bundled, WAL mode, migration runner, 18 RD tasks", state: "completed" as const },
  { id: "p2", timestamp: "Phase 2", title: "M4 Workspace Domain", description: "Workspace model, document scanning, indexing, 18 RD tasks", state: "completed" as const },
  { id: "p3", timestamp: "Phase 3", title: "M4 File Watch", description: "notify crate, debounce, conflict detection, 10 RD tasks", state: "completed" as const },
  { id: "p4", timestamp: "Phase 4", title: "M4 IPC + QA + Exit Gate", description: "8 workspace IPC commands, 16 M4 QA integration tests", state: "completed" as const },
  { id: "p5", timestamp: "Phase 5", title: "M5 UI Infrastructure", description: "Design tokens, 3-column layout, 8 common components, app state", state: "completed" as const },
  { id: "p6", timestamp: "Phase 6", title: "M5 Task Tree", description: "Task store with tree flattening, TaskTree/TaskRow, query IPC", state: "completed" as const },
  { id: "p7", timestamp: "Phase 7", title: "M5 Inspector", description: "Title/Status/Priority/Date/Tags editors, MoreProperties", state: "completed" as const },
  { id: "p8", timestamp: "Phase 8", title: "M5 Keyboard & Undo", description: "11 keyboard shortcuts: Arrow nav, Enter, Space, Delete, Ctrl+Z/Y/S/N", state: "completed" as const },
  { id: "p9", timestamp: "Phase 9", title: "M5 Multi-select & Filter", description: "Filter state, TaskFilter bar, Ctrl/Shift multi-select", state: "completed" as const },
  { id: "p10", timestamp: "Phase 10", title: "M5 QA + Exit Gate", description: "16 M5 QA tests, GATE-M5 verification, 257 total tests passing", state: "completed" as const },
];

export default function M4M5CompletionReport() {
  return (
    <ReportShell width="wide" ariaLabel="M4-M5 Development Plan Completion Report">
      <Stack gap="section">
        <Stack gap="component">
          <H1>M4-M5 Development Plan — Completion Report</H1>
          <Text tone="secondary">
            ModernToDoList 2.0 · Tauri + Vue 3 + Rust · All 10 phases implemented and verified
          </Text>
          <MetricsGrid
            variant="header"
            columns={4}
            items={[
              { label: "Phases Completed", value: "10 / 10", trend: "up" },
              { label: "Total Tests Passing", value: "257", trend: "up" },
              { label: "New Files Created", value: "15" },
              { label: "Files Modified", value: "5" },
            ]}
          />
        </Stack>

        <Callout tone="success">
          <Text>
            <strong>All phases complete.</strong> M4 (SQLite platform) and M5 (UI layer) fully implemented.
            Rust cargo check passes, frontend npm run build succeeds, all 257 tests green.
          </Text>
        </Callout>

        <ReportSection title="Phase Timeline" divided>
          <Timeline events={phaseTimeline} />
        </ReportSection>

        <ReportSection title="Test Suite Summary" divided>
          <ChartContainer ariaLabel="Test breakdown by suite">
            <BarChart
              categories={testBreakdown.map((t) => t.label)}
              series={[
                {
                  name: "Tests",
                  data: testBreakdown.map((t) => t.value),
                },
              ]}
              colorByCategory
            />
          </ChartContainer>
          <Table
            columns={qaColumns}
            rows={qaResults}
            density="compact"
          />
        </ReportSection>

        <ReportSection title="New Files (15)" divided>
          <Table
            columns={fileColumns}
            rows={newFiles}
            density="compact"
          />
        </ReportSection>

        <ReportSection title="Modified Files (5)" divided>
          <Table
            columns={changeColumns}
            rows={modifiedFiles}
            density="compact"
          />
        </ReportSection>

        <ReportSection title="Architecture Summary" divided>
          <Grid columns={2} gap={16}>
            <Stack gap={8}>
              <H3>Rust Backend (src-tauri)</H3>
              <Text size="small">
                SQLite (rusqlite bundled) with WAL mode, schema migrations, workspace domain model,
                file watcher (notify crate), IPC command layer for workspace and task queries.
                207 unit tests + 33 integration tests.
              </Text>
            </Stack>
            <Stack gap={8}>
              <H3>Vue 3 Frontend (src)</H3>
              <Text size="small">
                Design tokens CSS, 3-column resizable layout, task tree with expand/collapse and
                filter, inspector with property editors, keyboard shortcuts, multi-select.
                76 modules, 92KB JS + 97KB CSS production build.
              </Text>
            </Stack>
          </Grid>
        </ReportSection>

        <Divider />
        <Text tone="secondary" size="small">
          Generated for ModernToDoList 2.0 M4-M5 milestone completion.
        </Text>
      </Stack>
    </ReportShell>
  );
}
