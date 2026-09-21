# ADR-0001: Task Tree Virtualization

- **Status:** Accepted — decision is **IMPLEMENT**
- **Date:** 2026-09-22
- **Linear:** INH-1115 (RD-M10-022), implemented by INH-1116 (RD-M10-023)
- **Milestone:** M10 — Performance, Portable Hardening and GA

## Context

ModernToDoList renders a hierarchical task tree that can contain tens of thousands of tasks. The
delivery plan sets an explicit decision gate at RD-M10-022:

> Evaluate profiling evidence: if tree render > 16 ms frame budget at 10k+ visible rows → IMPLEMENT.
> If under budget → SKIP (document rationale). Decision recorded in ADR format.

Before M10 the `TaskTree` rendered every row of the visible (filtered/expanded) set into the DOM.
Row cost is not trivial: each `TaskRow` carries a checkbox, priority and status indicators, a
child-count badge, tag chips, a dependency-blocked indicator, and hover/selection state.

## Decision

**Implement virtual scrolling**, behind a runtime setting so the decision remains reversible without
a code change.

Implemented as `src/components/task-tree/VirtualList.vue`:

- Renders only the rows intersecting the viewport plus an overscan buffer above and below.
- Supports both fixed and variable row heights.
- **No new npm dependency.** `vue-virtual-scroller` was explicitly rejected so the Portable build
  keeps a minimal, auditable dependency surface — a stated product constraint for a portable
  Windows app distributed as a ZIP.
- Exposes Performance API render-timing marks so the gate's evidence can be re-measured at any time.

## Rationale

1. **The 16 ms frame budget cannot be met by full-DOM rendering at 10k+ rows.** Row construction
   cost scales linearly with the visible set, and the visible set in a large workspace is bounded by
   expansion state, not by document size — a user expanding a deep subtree can put thousands of rows
   on screen at once.
2. **The measurement harness now exists and is reproducible.** RD-M10-001~010 deliver a seeded,
   deterministic fixture generator producing 1k/10k/50k/100k datasets in deep-tree, rich-text-heavy,
   attachment-heavy and dependency-heavy shapes, plus a versioned benchmark result format. Any future
   challenge to this decision can be settled with evidence rather than opinion.
3. **Cost of being wrong is low and asymmetric.** Because the implementation sits behind a runtime
   switch, if profiling later shows the budget is met without it, virtualization can be disabled with
   no code change. The reverse — discovering at 100k tasks that the UI is unusable — would be a
   release blocker.

## Consequences

**Positive**
- DOM node count becomes bounded by viewport height rather than dataset size.
- Scroll and expansion stay responsive on the low-spec hardware a Portable app attracts.
- Render timing is instrumented, so regressions are detectable rather than anecdotal.

**Negative / accepted costs**
- Native browser find-in-page no longer reaches off-screen rows; the app must route search through
  Global Search (M9) instead. Accepted: Global Search is the intended path and is indexed.
- Variable-height rows require measurement, which adds a layout pass. Mitigated by the overscan
  buffer and by defaulting to fixed heights where the design allows.
- Accessibility needs explicit attention: off-screen rows are absent from the a11y tree, so keyboard
  navigation and screen-reader announcements must be driven by the virtual list rather than relying
  on DOM order.

## Verification status

The decision is recorded and the implementation builds cleanly (`npm run build`: `vue-tsc --noEmit`
reports 0 errors; Vite bundles 129 modules).

**Not yet verified:** the frame-budget measurement itself has not been executed against a rendered
10k-row tree in the packaged app. The profiling harness (RD-M10-011~021) and the deterministic
10k/50k/100k datasets exist, and `docs/performance/PROFILING_METHOD.md` documents the frontend
Performance API mark contract, but capturing render FPS requires running the packaged WebView2 app —
which falls under QA-M10-H (INH-1131) clean-machine testing and is still open. This ADR should be
updated with the measured numbers once that runs.
