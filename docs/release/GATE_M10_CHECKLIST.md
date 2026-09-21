# GATE-M10 — ModernToDoList 2.0 GA Exit Gate Checklist

**Linear:** INH-1133 · **Milestone:** M10 · **Priority:** P1
**Predecessor:** RC-M10 (INH-1132) blocker review → `RC_M10_BLOCKER_REVIEW.md`

Every criterion below is taken verbatim from the delivery plan
(`.qoder/specs/M6-M10_Full_Delivery_60d4cd8c.md`, section 5.6). The gate passes only when **all**
rows are `MET` with real evidence attached. A green build, a passing unit-test suite, or a
substantial implementation effort is **not** by itself evidence that a row is met — each row names
the specific artifact that proves it.

Status values: `MET` · `NOT MET` · `PARTIAL` · `CANNOT VERIFY HERE`.

---

## 1. Blocker and data-safety criteria

| # | Criterion | Status | Required evidence |
|---|-----------|--------|-------------------|
| 1.1 | All Blockers = 0 | | `RC_M10_BLOCKER_REVIEW.md` regenerated from live Linear state showing 0 open M6–M10 issues |
| 1.2 | All P0/P1 data-safety issues = 0 | | Linear query filtered to priority Urgent/High with a data-safety label, returning empty |
| 1.3 | No issue closed while its dependencies are open | | Confirm no GATE/RC issue is `Done` while its milestone's RD/QA issues are still Backlog |

> Criterion 1.3 exists because this project already had one violation: GATE-M10 itself was marked
> `Done` while all 93 M6–M10 issues sat in Backlog. That was corrected on 2026-09-22 (returned to
> Backlog with an evidence comment). Re-check it at every gate review.

## 2. Automated test criteria

| # | Criterion | Status | Required evidence |
|---|-----------|--------|-------------------|
| 2.1 | M0–M9 automated tests all pass | | Full `cargo test` transcript with per-suite counts, zero failures, zero ignored |
| 2.2 | Frontend type-checks and builds | | `npm run build` (`vue-tsc --noEmit && vite build`) output with zero TS errors |
| 2.3 | XML round-trip compatibility intact | | All `round_trip_test` cases pass — encodings, unknown elements/attributes, comments, dependencies, FileLink, real-world fixture |
| 2.4 | Index remains disposable | | A test that deletes `index.db`, rebuilds from XML, and asserts identical query results |

## 3. RC full matrix (QA-M10 A–H)

| Matrix | Cases | Linear | Status | Required evidence |
|--------|-------|--------|--------|-------------------|
| A — XML compatibility | A01~A13 | INH-1124 | | `m10_qa_a_xml_compat.rs` — 13 named tests, all pass |
| B — Atomic save / recovery | B01~B10 | INH-1125 | | `m10_qa_b_atomic_save.rs` — 10 named tests, all pass |
| C — Workspace / SQLite | C01~C10 | INH-1126 | | `m10_qa_c_workspace_sqlite.rs` — 10 named tests, all pass |
| D — Core task UX | D01~D10 | INH-1127 | | `m10_qa_d_core_ux.rs` — 10 named tests, all pass |
| E — Relations / attachments / rich content | E01~E12 | INH-1128 | | Requires M6 + M7 implemented; 12 named tests |
| F — Cross-document transactions | F01~F10 | INH-1129 | | Requires M8 implemented; 10 named tests incl. crash recovery |
| G — Productivity | G01~G10 | INH-1130 | | Requires M9 implemented; 10 named tests incl. Chinese search |
| H — Windows Portable / clean machine | H01~H15 | INH-1131 | | **Human execution required** — `docs/qa/QA_M10_H_PORTABLE_PROTOCOL.md` result table, filled in for both Win10 and Win11 |

**Matrix H cannot be automated.** It requires clean Windows 10 and Windows 11 VMs with no network,
no administrator rights, and no Node/Rust/Python/Git toolchain installed. Until a human executes it
and records results, criterion 3.H is `CANNOT VERIFY HERE` and **the gate cannot pass**. This is a
hard structural limit, not a matter of effort.

## 4. Explicit GATE-M10 verification items (from spec section 5.6)

| # | Criterion | Status | Required evidence |
|---|-----------|--------|-------------------|
| 4.1 | Delete `index.db` → rebuild → full function | | Test or manual run showing search, smart views, saved views, filters and counts all identical after rebuild |
| 4.2 | Cross-file Move kill recovery | | Matrix F crash-recovery tests: kill at each phase boundary, assert no task lost (duplicate-not-loss) |
| 4.3 | Win10 + Win11 clean VM | | Matrix H rows H01–H15 on both OSes |
| 4.4 | No-network Portable run | | Matrix H case H03 with the vNIC disabled and zero outbound connections observed |

## 5. Release artifact criteria

| # | Criterion | Status | Required evidence |
|---|-----------|--------|-------------------|
| 5.1 | Release ZIP hash fixed | | Published SHA-256 of `ModernToDoList-2.0.0-Portable-win-x64.zip`, reproducible: building twice from the same inputs yields the same hash |
| 5.2 | Migration test passes | | Versioned Data/settings/index migration tests across ≥3 versions, plus the failure→index-rebuild fallback |
| 5.3 | Release notes complete | | `docs/release/RELEASE_NOTES_2.0.0.md` with the 2.0.0 changelog |
| 5.4 | Licenses collected | | `LICENSES/` containing Rust crate licences (from `Cargo.lock`) and npm licences (from `package-lock.json`) |
| 5.5 | Version metadata embedded | | EXE file version / product version / company present and matching 2.0.0 |
| 5.6 | Manual update path documented | | `docs/release/MANUAL_UPDATE.md`, validated by matrix H case H15 |
| 5.7 | WebView2 policy documented | | `docs/release/WEBVIEW2_POLICY.md`, validated by matrix H case H02 |
| 5.8 | Commit and version frozen | | The RC commit SHA, version string and ZIP hash recorded in RC-M10 and immutable thereafter |

## 6. Security criteria

| # | Criterion | Status | Required evidence |
|---|-----------|--------|-------------------|
| 6.1 | No secret in the repository or its history | | `git grep` for the API-key pattern across all reachable commits returns empty; `.env` gitignored |
| 6.2 | HTML sanitizer resists XSS | | Matrix E / QA-M7-007~009 adversarial cases: script, iframe, `on*` handlers, `javascript:`, `data:`, entity-encoded and mutation-XSS payloads all neutralised |
| 6.3 | Attachment paths cannot traverse the asset root | | Tests asserting `../` and absolute-path escapes are rejected |
| 6.4 | Crash dumps are privacy-safe | | Matrix H case H13: exported bundle contains no task content and no verbatim file paths (hashes only) |

---

## Sign-off

The gate may be moved to `Done` only when every row above is `MET` and the RC commit SHA, version
string and ZIP SHA-256 are recorded. Any row that is `NOT MET`, `PARTIAL` or `CANNOT VERIFY HERE`
means GATE-M10 stays open, and the specific unmet rows must be listed in the Linear comment so the
gap is visible rather than implied.

**Do not close this gate on the strength of passing unit tests alone.** Criteria 3.H, 4.3 and 4.4
require physical clean-machine execution that no automated suite can substitute for.
