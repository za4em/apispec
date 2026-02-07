# apispec TUI Scalability and Readability Plan

## 1. Problem Statement and Goals

`apispec` already loads OpenAPI 3.1.0 specs and renders a functional flat endpoint list + text details panel. The current UX breaks down for large APIs (200+ endpoints) because:

- the left panel is flat and hard to navigate,
- there is no explicit panel focus model,
- details are mostly a long wrapped text block,
- request/response schemas are summarized into compact strings instead of navigable structures.

This plan defines a production-grade architecture and phased implementation to deliver:

1. Hierarchical endpoint tree with expandable groups and search-aware auto-expansion.
2. Explicit focus and keyboard interaction model for tree vs details.
3. Structured details UI with sectioning, table-like parameter layout, and response/status styling.
4. Expandable request/response bodies with readable, navigable schema tree rendering.

Non-goals remain unchanged:

- no API calling/auth/write operations,
- no support beyond OpenAPI 3.1.0.

---

## 2. Current Architecture Baseline (Codebase Mapping)

Current responsibilities:

- `src/spec/index.rs`
  - builds flat `Vec<EndpointSummary>` sorted by path/method,
  - resolves parameters/request body/responses into summary-level structs,
  - computes basic search text.
- `src/spec/render.rs`
  - summarizes schemas into short strings (depth/node capped).
- `src/tui/state.rs`
  - holds app state, flat filter state, selected endpoint index,
  - renders endpoint details as wrapped plain text lines.
- `src/tui/keymap.rs`
  - maps keys for a single "normal/search" mode model.
- `src/tui/event.rs`
  - event loop dispatches actions to state.
- `src/tui/view.rs`
  - renders search box, flat endpoint list, plain text details, help line.

Implication: we need a state and rendering refactor, but no changes are required in source fetch/cache/version-validation paths.

---

## 3. Architecture Principles

Implementation will follow these principles:

1. Separation of concerns:
   - spec indexing/grouping logic in `spec/*`,
   - UI interaction state machine in `tui/state.rs`,
   - terminal rendering in `tui/view.rs`,
   - key -> action mapping in `tui/keymap.rs`.
2. Stable identity everywhere:
   - endpoint IDs, group IDs, detail node IDs must be deterministic to preserve selection and expansion state.
3. Predictable state transitions:
   - explicit focus enum and contextual actions to avoid ambiguous key behavior.
4. Incremental computation:
   - precompute searchable tokens and grouping once,
   - rebuild visible tree rows only when filter/expand state changes,
   - cache expensive detail/schema rendering by width and endpoint identity.
5. Graceful degradation:
   - unresolved refs, missing tags/schemas, and uncommon schema forms never crash; always render a safe placeholder.

---

## 4. Target Data Model Changes

### 4.1 Extend endpoint index model

In `src/spec/index.rs`, extend `EndpointSummary` with fields needed for grouping and search:

- `tags: Vec<String>` (normalized operation tags),
- `group_key: String` (resolved display group),
- `group_sort_key: String` (lowercased normalized key),
- `search_text` expanded to include tags and operation metadata.

Grouping derivation rules:

1. Primary: first non-empty operation tag.
2. Fallback: first path segment (`/users/{id}` -> `users`).
3. Final fallback: `"Untagged"`.

Sorting rules:

- groups alphabetical (case-insensitive), `"Untagged"` forced last,
- endpoints within group sorted by `path`, then method rank (`GET, POST, ...`), then method lexicographically.

### 4.2 Introduce tree view-model types

Add a tree model module (recommended: `src/tui/tree.rs`) to keep tree logic out of render code.

Core structures:

- `GroupNode { id, label, endpoint_ids }`
- `TreeRow { kind: Group|Endpoint, group_id, endpoint_id, depth, is_expanded, is_match }`
- `TreeModel { groups, rows_visible, expanded_groups, manual_expanded_groups }`

Notes:

- `manual_expanded_groups` preserves user toggles when no filter is active.
- during active filter, matching groups are auto-expanded without mutating manual preferences.

### 4.3 Introduce structured details model

Replace ad hoc string generation with structured rows (recommended: `src/tui/details.rs`):

- `DetailSection` enum: `Overview`, `Parameters`, `RequestBody`, `Responses`, `Security`
- `DetailRow`:
  - text spans/styling payload,
  - optional `row_id`,
  - optional `toggle_target` (expand/collapse target),
  - optional section association.
- `DetailsDocument`:
  - ordered rows,
  - row index map for section navigation and focus.

### 4.4 Schema tree model

Add `src/spec/schema_tree.rs` for full schema traversal/resolution:

- `SchemaNode { id, label, type_label, required, enum_values, description, example, ref_name, children }`
- cycle-safe resolver with `visited_ref_stack`,
- support for object/array/primitive/null and common composed forms (`oneOf`, `anyOf`, `allOf`) with safe fallback text when details are unavailable.

Node IDs should be deterministic from traversal path (for stable expand state).

---

## 5. Focus and Interaction State Machine

Add explicit focus and richer details navigation to `AppState`:

- `FocusPanel { Tree, Details }`
- keep `InputMode { Normal, Search }` for search capture behavior.

Rules:

1. Tree focus
   - `Up/Down`: move visible tree row selection.
   - `Right/Enter` on group: expand/collapse group.
   - `Enter` on endpoint: set selected endpoint and switch focus to details.
2. Details focus
   - `Up/Down`: scroll details or move selected detail row (for toggle targets).
   - `Tab`: cycle `DetailSection`.
   - `Enter`: toggle currently selected expandable item (request body, response code, content type, optional nested schema node).
   - `Esc`: return focus to tree.
3. Search mode
   - remains text-entry overlay mode,
   - filter applies to group label, endpoint path, operationId, tags, summary/title,
   - matching groups auto-expand while filter is non-empty.

Add transition tests to guarantee no invalid state combinations.

---

## 6. Rendering Plan (ratatui)

### 6.1 Left panel tree

In `src/tui/view.rs`:

- replace flat endpoint list with tree rows:
  - group rows prefixed with fold indicators (`[-]`, `[+]`),
  - endpoint rows indented under group.
- apply focused panel styling:
  - focused panel border/header uses accent style,
  - unfocused panel uses muted style.
- preserve item count in title:
  - total endpoints + filtered visible endpoints.

### 6.2 Right panel structured details

Render using section headers and separators:

- prominent endpoint header (`METHOD PATH`) with method color,
- summary + description,
- sections: Parameters, Request Body, Responses, Security.

Parameters layout:

- fixed columns with width allocation by available area:
  - `name`, `in`, `required`, `type`, `description`.
- truncate and wrap description safely.

Responses:

- status code badges with semantic colors:
  - success (2xx) green-ish,
  - redirect/info/warn/error distinct subdued palette.
- show status + description + content types.

Request/response expansion:

- collapsed row shows compact summary,
- expanded row inserts schema tree rows below with indentation guides.

Orientation aids:

- breadcrumb line for active schema node path in details focus,
- highlighted current detail row when details panel is focused.

---

## 7. Search and Filtering Behavior

Filtering strategy:

1. Normalize query to lowercase, split by whitespace tokens.
2. Endpoint matches if all tokens appear across combined searchable text:
   - path, method, title, operationId, tags, group label.
3. Group matches if:
   - group label matches tokens, or
   - any child endpoint matches.

Display strategy:

- no filter: show all groups using manual expansion state.
- filter active:
  - show only groups with matches,
  - show only matching endpoints in each shown group,
  - auto-expand shown groups.

Selection strategy:

- preserve previously selected endpoint when still visible,
- otherwise select first visible row,
- reset detail scroll when selected endpoint changes.

---

## 8. Expand/Collapse and Schema Navigation Details

Expandable entities:

1. Request Body section.
2. Each response status block (e.g., `200`, `400`).
3. Each response content-type row.
4. Optional: nested schema object/array nodes.

State storage:

- `HashSet<ToggleId>` inside `DetailsState`.
- `ToggleId` deterministic key examples:
  - `request_body`,
  - `response:200`,
  - `response:200:application/json`,
  - `schema:request_body:application/json:/properties/user`.

Schema rendering behavior:

- resolve internal refs where possible, include reference name label,
- show required markers,
- show type and enum values (compact),
- wrap descriptions and compact examples,
- detect cycles and render `[cycle: RefName]`,
- unresolved refs render `[unresolved: #/components/... ]`.

---

## 9. Performance Plan

Target: interactive feel for 200+ endpoints and deep schemas.

Controls:

1. One-time index + grouping build on startup.
2. Tree visible rows rebuilt only when:
   - search query changes,
   - group expansion changes.
3. Detail document cached by:
   - endpoint ID,
   - width bucket,
   - expansion state fingerprint.
4. Schema expansion is lazy:
   - do not flatten entire schema tree when collapsed.
5. String allocations minimized:
   - precompute lowercase search strings once.

Validation:

- add micro-benchmark-like tests for tree rebuild on synthetic large specs (unit-level timing assertions should be coarse and deterministic).

---

## 10. Robustness and Edge Cases

Must handle without panic or broken UI:

- missing tags and empty tag strings,
- paths with no meaningful segment (`/`, `/{id}`),
- unresolved parameter/request/response/schema refs,
- responses lacking descriptions/content,
- non-JSON content types,
- endpoints with very long descriptions/enum/example text,
- mixed nullable/union/composed schema forms,
- empty specs (`paths: {}`) and filter-no-match states.

---

## 11. File-Level Implementation Plan

### Phase 1: Data/index foundation

Files:

- `src/spec/index.rs`
  - extend endpoint metadata and search payload,
  - implement group key derivation and deterministic sorting helpers.
- `src/spec/mod.rs`
  - export any new modules as needed.

Deliverable: grouped-ready endpoint index with tests.

### Phase 2: Tree model and state machine

Files:

- `src/tui/state.rs`
  - add `FocusPanel`,
  - replace flat filtered list state with tree-aware selection + expansion state,
  - preserve endpoint selection across filter changes.
- `src/tui/tree.rs` (new)
  - tree row generation, filtering projection, expansion logic.
- `src/tui/keymap.rs`
  - add contextual actions for tree/details focus, tab section navigation, toggle detail items.
- `src/tui/event.rs`
  - dispatch new actions and clamp selection/scroll safely.

Deliverable: keyboard behavior contract implemented and unit-tested.

Status: Done (implemented in this session).

### Phase 3: Structured details and schema engine

Files:

- `src/spec/schema_tree.rs` (new)
  - schema resolver/traverser + node model + cycle handling.
- `src/tui/details.rs` (new)
  - details document builder, row IDs, toggle targets, section index map.
- `src/tui/state.rs`
  - details focus state, expanded toggle storage, breadcrumb tracking.

Deliverable: expandable request/response/schema content with deterministic row IDs.

### Phase 4: Visual rendering integration

Files:

- `src/tui/view.rs`
  - render tree UI (indents, fold indicators, focus styling),
  - render structured detail sections with table-like parameter rows and response badges,
  - render breadcrumb + focused row visuals.

Deliverable: final UX with explicit focus indicators and scannable details.

### Phase 5: Docs and polish

Files:

- `README.md`
  - document grouping rules,
  - document tree vs details keybindings,
  - explain expand/collapse behavior for request/response bodies and schema nodes.

Deliverable: docs aligned with final keyboard model and behavior.

---

## 12. Test Strategy

### Unit tests

- grouping fallback behavior (tag -> first path segment -> `Untagged`),
- group/order stability and `Untagged` last,
- filter matching coverage (group name, path, operationId, tags),
- focus transition tests (tree/details/search),
- toggle state behavior for request/response/schema nodes,
- schema resolver tests for cycles, unresolved refs, composed schema cases.

### State/model tests

- selection preservation across filter changes,
- auto-expand behavior only while filter is active,
- detail section tab navigation order,
- scroll clamping after resize and expansion collapse.

### Snapshot tests

- detail rendering at multiple width buckets,
- parameter table alignment and wrapping,
- response status styling labels and expanded schema tree layout.

### Regression tests

- no endpoints,
- huge endpoint list synthetic fixture,
- deeply nested schema fixture.

---

## 13. Definition of Done

All are required:

1. All prompt feature requirements implemented end-to-end.
2. No crashes on missing tags/schemas/uncommon refs.
3. Keyboard controls are deterministic and documented.
4. Tree filtering/expansion is responsive for 200+ endpoints.
5. Details panel is visibly structured with expandable bodies and readable schema tree.
6. `cargo test` passes and README update is complete.

---

## 14. Execution Order and Risk Management

Recommended execution order:

1. Implement data model + grouping first.
2. Implement tree state and keymap state machine.
3. Implement structured details model and schema engine.
4. Integrate rendering and styling.
5. Harden edge cases and finalize docs/tests.

Main risks and mitigation:

- Risk: state complexity explosion across search/focus/expand.
  - Mitigation: centralized action reducer pattern in `AppState` plus transition tests.
- Risk: schema traversal regressions and cycles.
  - Mitigation: dedicated `schema_tree` module with strict cycle detection and fixtures.
- Risk: performance regressions.
  - Mitigation: cached render documents and incremental row rebuild boundaries.

This plan keeps responsibilities modular and testable, and is intentionally aligned with the existing crate structure to minimize migration risk.

---

## 15. Session Restore Checklist

Use this as a resumable execution checklist after clearing context.

### Pre-flight

- [ ] Re-read `prompt.txt` requirements and non-goals.
- [ ] Re-read this `plan.md` and confirm scope is unchanged.
- [ ] Run `git status --short` and note unrelated local changes.
- [ ] Run `cargo test` to capture baseline failures (if any) before edits.

### Phase 1: Data/index foundation

- [ ] Extend `EndpointSummary` with `tags`, `group_key`, `group_sort_key`.
- [ ] Implement group-key derivation: first tag -> first path segment -> `Untagged`.
- [ ] Expand search text to include tags/group metadata.
- [ ] Add deterministic group and endpoint ordering helpers.
- [ ] Add/refresh unit tests in `src/spec/index.rs` for grouping fallback and ordering.
- [ ] Run `cargo test` and fix regressions before moving on.

### Phase 2: Tree model and state machine

- [x] Create `src/tui/tree.rs` with group nodes, visible rows, expansion logic.
- [x] Replace flat filtered list state in `src/tui/state.rs` with tree-aware state.
- [x] Add `FocusPanel { Tree, Details }` and transition-safe reducers.
- [x] Preserve endpoint selection across filter updates where possible.
- [x] Update `src/tui/keymap.rs` with tree/details contextual actions.
- [x] Update `src/tui/event.rs` dispatch to new actions and clamping logic.
- [x] Add tests for selection persistence, auto-expand-on-filter, focus transitions.
- [x] Run `cargo test`.

### Phase 3: Structured details and schema engine

- [ ] Add `src/spec/schema_tree.rs` for resolved, cycle-safe schema traversal.
- [ ] Add `src/tui/details.rs` for sectioned detail document and row/toggle IDs.
- [ ] Add expand/collapse state for request body, responses, content types, schema nodes.
- [ ] Add breadcrumb/active-node tracking in details focus.
- [ ] Add tests for cycles, unresolved refs, composed schema fallback behavior.
- [ ] Run `cargo test`.

### Phase 4: Rendering integration

- [ ] Replace flat list render with tree render in `src/tui/view.rs`.
- [ ] Add focused vs unfocused panel visual styling.
- [ ] Render details as structured sections instead of plain wrapped text dump.
- [ ] Implement parameter table-like layout and response status badge styling.
- [ ] Render expandable schema blocks and active detail-row highlight.
- [ ] Validate behavior manually with a large spec (200+ endpoints).
- [ ] Run `cargo test`.

### Phase 5: Docs and polish

- [ ] Update `README.md` grouping behavior docs.
- [ ] Update `README.md` keybindings for tree focus vs details focus.
- [ ] Update `README.md` expand/collapse behavior for bodies/schemas.
- [ ] Add any missing regression tests discovered during manual QA.
- [ ] Run `cargo test` and ensure green.

### Final acceptance gate

- [ ] No crashes on missing tags/schemas/unresolved refs.
- [ ] Tree filtering and expansion remains responsive on large specs.
- [ ] Keyboard navigation is consistent and matches README.
- [ ] Details are visually scannable and schema navigation is usable.
- [ ] `git diff` only contains intentional changes.
