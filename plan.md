# apispec Architecture and Implementation Plan

## 1. Product Contract (Locked)

`apispec` is a Rust CLI + TUI tool for read-only exploration of OpenAPI specs.

Non-negotiable requirements from `prompt.txt`:

1. Command is `apispec <source>`.
2. `<source>` can be:
   1. Local file.
   2. Direct OpenAPI URL.
   3. Base API URL where spec must be discovered automatically.
3. Must cache locally and attempt refresh on every launch.
4. If network is down, must use cached spec when available and clearly indicate offline mode.
5. Only OpenAPI `3.1.0` is accepted. Everything else fails clearly.
6. Interactive TUI:
   1. Left: searchable endpoint list.
   2. Right: selected endpoint details.
   3. Keyboard-only navigation (`h/j/k/l` and arrows; plus search shortcuts).
7. Show endpoint method/path, summary/description, grouped parameters, request body formats/schemas, responses/schemas.
8. Prioritize startup speed, responsiveness, sensible defaults, clear errors.

Out of scope:

1. API calling.
2. Auth flow execution.
3. Support for OpenAPI versions other than 3.1.0.

---

## 2. Research-Based Tool Choices

### 2.1 Core crates

1. `clap` (CLI parsing): stable and ergonomic.
2. `reqwest` + `rustls` (HTTP): robust HTTP client and conditional requests.
3. `oas3` (OpenAPI 3.1 parser/model):
   1. Explicitly targets OpenAPI 3.1.x.
   2. Provides `Spec` with helper methods (`operations`, `operation_by_id`, `validate_version`).
   3. Provides `ObjectOrReference::resolve` for `$ref` handling.
4. `ratatui` + `crossterm` (TUI and keyboard events): proven stack for responsive terminal apps.
5. `directories` (cross-platform cache dir).
6. `serde`, `serde_json`, `serde_yaml` (metadata and tolerant format handling).
7. `sha2` (stable cache keying from source identity).
8. `thiserror` + `anyhow` (typed errors + context).

### 2.2 Why `oas3` over `openapiv3_1`

1. `openapiv3_1` is explicitly marked as under active development.
2. `oas3` has stronger navigation primitives (operations iterator + reference resolution APIs).
3. `oas3` cleanly models OpenAPI 3.1 structures needed for endpoint extraction and schema rendering.

### 2.3 Discovery standards and ecosystem defaults used

1. `service-desc` link relation (RFC 8631 / IANA) for machine-readable service description discovery.
2. Common framework defaults:
   1. FastAPI: `/openapi.json`.
   2. Springdoc: `/v3/api-docs`, `/v3/api-docs.yaml`.
   3. Swashbuckle/ASP.NET: `/swagger/v1/swagger.json`.

---

## 3. System Architecture (Final)

### 3.1 High-level flow

1. Parse CLI input.
2. Classify source (`LocalFile | DirectUrl | BaseUrl`).
3. Resolve to concrete spec location:
   1. Local file directly.
   2. Direct URL directly.
   3. Base URL via discovery strategy.
4. Load/refresh cache.
5. Parse and validate spec (`openapi == "3.1.0"` strict).
6. Build UI-ready index from `Spec`.
7. Start TUI loop.

### 3.2 Module layout

```text
src/
  main.rs
  cli.rs
  app.rs
  error.rs
  source/
    mod.rs
    classify.rs
    discover.rs
    fetch.rs
  cache/
    mod.rs
    store.rs
    metadata.rs
  spec/
    mod.rs
    load.rs
    validate.rs
    index.rs
    render.rs
  tui/
    mod.rs
    state.rs
    event.rs
    view.rs
    keymap.rs
```

### 3.3 Core data models

```text
SourceInput { raw: String, kind: SourceKind, normalized_key: String }

ResolvedSpecLocation {
  canonical_source: String,      // for cache identity
  spec_url: Option<Url>,         // if remote
  local_path: Option<PathBuf>,   // if local
  discovery_trace: Vec<String>,  // for diagnostics
}

CacheMetadata {
  canonical_source: String,
  resolved_spec_url: Option<String>,
  etag: Option<String>,
  last_modified: Option<String>,
  fetched_at_utc: String,
  last_success_at_utc: String,
  openapi_version: String,
  content_sha256: String,
}

LoadedSpec {
  spec: oas3::Spec,
  source_label: String,
  cache_state: CacheState,       // Fresh | Revalidated304 | OfflineStale | NoCache
}

EndpointSummary {
  id: usize,
  method: String,
  path: String,
  title: String,                 // summary fallback method+path
  description: Option<String>,
  operation_id: Option<String>,
  grouped_parameters: GroupedParameters,
  request_body: Option<RequestBodyView>,
  responses: Vec<ResponseView>,
}
```

---

## 4. Source Handling and Discovery

### 4.1 Source classification

1. If argument parses as URL:
   1. If path suggests spec (`.json`, `.yaml`, `.yml`, `openapi`, `swagger`, `api-docs`) => `DirectUrl`.
   2. Else => `BaseUrl`.
2. Else treat as local path:
   1. Must exist and be a file.
   2. Else error with remediation hint.

### 4.2 Base URL discovery algorithm (ordered)

1. Normalize base URL (trim trailing slash).
2. Request base URL (`GET`, short timeout).
3. Extract discovery hints in this order:
   1. HTTP `Link` headers with `rel=service-desc`.
   2. HTML `<link rel="service-desc" href="...">`.
   3. Swagger/ReDoc script hints (`url`, `urls` in known bootstrap blocks).
4. Probe known candidates (first success wins):
   1. `/openapi.json`
   2. `/openapi.yaml`
   3. `/openapi.yml`
   4. `/v3/api-docs`
   5. `/v3/api-docs.yaml`
   6. `/swagger/v1/swagger.json`
   7. `/swagger.json`
   8. `/swagger.yaml`
5. For each candidate:
   1. Fetch.
   2. Parse as OpenAPI.
   3. Validate strict version.
   4. Accept on first valid match.
6. If none resolve: return deterministic error listing attempted endpoints.

### 4.3 HTTP policy

1. Build one reusable blocking client.
2. Defaults:
   1. Connect timeout: 5s.
   2. Total timeout: 15s.
   3. Redirects: enabled (safe default).
3. Headers:
   1. `Accept: application/json, application/yaml, text/yaml, */*`.
   2. User-Agent: `apispec/<version>`.
4. Optional conditional headers from cache:
   1. `If-None-Match`.
   2. `If-Modified-Since`.

---

## 5. Cache Strategy

### 5.1 Location and keying

1. Root: `ProjectDirs::from("dev", "apispec", "apispec").cache_dir()/specs`.
2. Cache key: `sha256(canonical_source_string)`.
3. Per-key files:
   1. `spec.raw` (exact fetched/read bytes).
   2. `metadata.json`.

### 5.2 Launch behavior (always refresh attempt)

1. Local file source:
   1. Read local file every launch.
   2. Update cache copy every launch.
2. Remote source:
   1. Attempt network refresh every launch.
   2. Use conditional requests when metadata exists.
   3. `304`: reuse cached bytes, mark `Revalidated304`.
   4. `200`: replace cache bytes and metadata, mark `Fresh`.
3. Network failure:
   1. If cache exists: load cached bytes, mark `OfflineStale`.
   2. If cache missing: fail with clear "offline and no cache" error.

### 5.3 Offline indication

TUI status line must display one of:

1. `Source: fresh`.
2. `Source: cached (not modified)`.
3. `Source: offline, using cached copy from <timestamp>`.

---

## 6. Parsing, Validation, and Normalization

### 6.1 Parse strategy

1. Try JSON parse path first when content-type or extension indicates JSON.
2. Try YAML parse otherwise.
3. If ambiguous/failure, attempt both before failing.
4. Convert parse failures into concise actionable messages.

### 6.2 Strict version gate (prompt requirement)

1. Validate `spec.openapi == "3.1.0"` string exact match.
2. Reject `3.1.1`, `3.1`, `3.0.x`, `2.0`, and malformed values.
3. Error format:
   1. "Unsupported OpenAPI version `<value>`."
   2. "This tool currently supports only `3.1.0`."

### 6.3 Resilience policy

1. No silent failures.
2. Unknown/extra fields are tolerated by parser.
3. Unresolved refs do not crash UI; they degrade to readable placeholders.

---

## 7. Endpoint Indexing and Rendering

### 7.1 Building endpoint list

1. Iterate all `spec.paths`.
2. For each `PathItem`, collect operations by HTTP method.
3. Produce sorted `Vec<EndpointSummary>`:
   1. Primary sort: path.
   2. Secondary sort: method order (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `HEAD`, `TRACE`).

### 7.2 Parameters (grouped + override rules)

1. Collect path-level parameters.
2. Collect operation-level parameters.
3. Merge by `(name, in)` key where operation-level overrides path-level.
4. Group output into:
   1. Path
   2. Query
   3. Header
   4. Cookie

### 7.3 Request bodies and responses

1. Resolve `ObjectOrReference` where possible via `resolve(&spec)`.
2. Request body view:
   1. Show required flag.
   2. For each media type, show schema summary.
3. Response view:
   1. Show status code/default.
   2. Show description.
   3. Show media types with schema summaries.

### 7.4 Schema summarizer (concise, readable)

1. Output target: plain text lines optimized for terminal width.
2. Rules:
   1. Prefer type + format first (`object`, `array<string>`, `string(uuid)`).
   2. Show required properties first.
   3. Limit recursive expansion depth (`max_depth=2`) and node count per schema.
   4. Detect cycles via visited ref paths.
   5. On cutoff, append "..." with count hint.

---

## 8. TUI Architecture

### 8.1 Layout

1. Top status bar (source/cache/version/filter stats).
2. Main split:
   1. Left pane (search + endpoint list).
   2. Right pane (details for selected endpoint).
3. Bottom help bar (key hints).

### 8.2 Interaction model

1. Normal mode:
   1. `j/k` or `Down/Up`: move selection.
   2. `g` / `G`: first/last endpoint.
   3. `PageUp/PageDown`: jump.
   4. `q`: quit.
   5. `/` or `Ctrl+s`: enter search mode.
2. Search mode:
   1. Type to filter.
   2. `Esc`: exit search mode.
   3. `Backspace`: delete.
   4. `Enter`: keep current query and return to normal.

### 8.3 Search behavior

1. Case-insensitive substring search.
2. Search fields:
   1. Method.
   2. Path.
   3. Summary.
   4. Operation ID.
3. Incremental update on each keystroke.
4. Always keep UI responsive by filtering against precomputed lowercase index strings.

### 8.4 Performance controls

1. Precompute endpoint labels and search index tokens at load time.
2. Cache rendered detail lines per selected endpoint + width bucket.
3. Do not resolve/render every endpoint body at startup; do lazy detail rendering.

---

## 9. Error Model and UX Messages

### 9.1 Error categories

1. CLI/input errors.
2. Discovery errors.
3. HTTP/network errors.
4. Cache read/write errors.
5. Parse errors.
6. Version validation errors.
7. TUI runtime errors.

### 9.2 Message standard

Every surfaced error must include:

1. What failed.
2. Why (if known).
3. What user can do next.

Example:

`Could not discover an OpenAPI spec from base URL https://api.example.com.
Tried: /openapi.json, /v3/api-docs, /swagger/v1/swagger.json.
If your spec lives elsewhere, pass it directly: apispec <full-spec-url>.`

---

## 10. Implementation Phases

## Phase 1: Bootstrap and plumbing

1. Create crate and module skeleton.
2. Add dependencies and error scaffolding.
3. Implement CLI arg parsing.
4. Implement source classification.

Exit criteria:

1. `apispec <source>` validates inputs and prints resolved source kind.

## Phase 2: Fetch + cache + strict validation

1. Implement local load path.
2. Implement remote fetch with conditional requests.
3. Implement cache storage and metadata.
4. Implement parse + strict `3.1.0` validation.

Exit criteria:

1. Works for local and direct URL.
2. Fails cleanly for non-3.1.0.
3. Offline fallback works with existing cache.

## Phase 3: Discovery and indexing

1. Implement base URL discovery algorithm.
2. Implement endpoint extraction and parameter merge.
3. Implement request/response/schema summary renderer.

Exit criteria:

1. Base URL discovery succeeds on representative defaults.
2. Endpoint summaries are complete and readable.

## Phase 4: TUI

1. Implement app state and event loop.
2. Implement left pane searchable list.
3. Implement right pane detail rendering + scrolling.
4. Add cache/offline status indicators.

Exit criteria:

1. Keyboard-only navigation complete.
2. UI remains responsive on large specs.

## Phase 5: Hardening and release quality

1. Add unit and integration tests.
2. Add snapshot tests for schema summary output.
3. Run `fmt`, `clippy`, `test`.
4. Add README usage examples and error behavior doc.

Exit criteria:

1. Test suite green.
2. No clippy warnings in project code.
3. Clear docs for normal and offline usage.

---

## 11. Test Plan

### 11.1 Unit tests

1. Source classification matrix.
2. Discovery candidate URL generation.
3. Strict version validator.
4. Parameter merge rules.
5. Schema summarizer depth/cycle behavior.

### 11.2 Integration tests

1. Local JSON spec load.
2. Local YAML spec load.
3. Remote spec fetch 200.
4. Remote conditional fetch 304.
5. Offline + cache fallback.
6. Offline + no cache failure.
7. Base URL discovery from:
   1. `Link: rel=service-desc`.
   2. `/openapi.json`.
   3. `/v3/api-docs`.
8. Version mismatch rejection (`3.1.1`, `3.0.3`).

### 11.3 Manual QA

1. Very large spec (thousands of operations).
2. Narrow terminal width behavior.
3. Search and navigation ergonomics.
4. Clear offline banner visibility.

---

## 12. Performance Targets

1. Startup (cached or local): under 500ms for medium specs (~500 operations).
2. Startup (fresh network, excluding server latency): under 1.5s for medium specs.
3. Navigation input-to-render latency: below 50ms in normal usage.
4. Search update latency: below 100ms for ~5k endpoints.

---

## 13. Risks and Mitigations

1. Risk: Discovery false negatives on custom docs endpoints.
   1. Mitigation: deterministic candidate list + service-desc parsing + clear failure output.
2. Risk: Complex external `$ref` chains.
   1. Mitigation: resolve local refs robustly; degrade unresolved refs gracefully; never crash.
3. Risk: Terminal key differences across OSes.
   1. Mitigation: support both vim keys and arrow keys; avoid platform-specific key assumptions.
4. Risk: Cache corruption.
   1. Mitigation: atomic writes via temp file + rename; metadata/content hash checks.

---

## 14. Definition of Done

1. `apispec <local-file>` works end-to-end.
2. `apispec <direct-url>` works end-to-end.
3. `apispec <base-url>` discovers and loads spec on common defaults.
4. Offline fallback works exactly as specified.
5. Strict version gate enforces only `3.1.0`.
6. TUI displays required endpoint details and remains responsive.
7. Tests cover critical behaviors and pass in CI.

---

## 15. Primary References Used

1. OpenAPI specification 3.1.0: https://spec.openapis.org/oas/v3.1.0
2. OpenAPI patch release note (3.1.1 context): https://www.openapis.org/blog/2024/10/25/announcing-openapi-specification-patch-releases
3. `oas3` crate docs: https://docs.rs/oas3/latest/oas3/
4. `oas3::spec::Spec` docs: https://docs.rs/oas3/latest/oas3/spec/struct.Spec.html
5. `oas3::spec::Operation` docs: https://docs.rs/oas3/latest/oas3/spec/struct.Operation.html
6. `oas3::spec::ObjectOrReference` docs: https://docs.rs/oas3/latest/oas3/spec/enum.ObjectOrReference.html
7. `ratatui` list state docs: https://docs.rs/ratatui/latest/ratatui/widgets/struct.ListState.html
8. `crossterm` event docs: https://docs.rs/crossterm/latest/crossterm/event/index.html
9. `reqwest` blocking client docs: https://docs.rs/reqwest/latest/reqwest/blocking/
10. `directories::ProjectDirs` docs: https://docs.rs/directories/latest/directories/struct.ProjectDirs.html
11. RFC 8631 (`service-desc`): https://www.rfc-editor.org/rfc/rfc8631.html
12. IANA link relations registry: https://www.iana.org/assignments/link-relations
13. FastAPI default OpenAPI URL: https://fastapi.tiangolo.com/tutorial/metadata/
14. Springdoc default docs endpoints: https://springdoc.org/
15. Microsoft Swashbuckle sample endpoint convention: https://learn.microsoft.com/en-us/samples/dotnet/aspnetcore.docs/getstarted-swashbuckle-aspnetcore/

---

## 16. Execution Checklist

### Phase 1: Bootstrap and plumbing

- [x] Create Rust binary crate (`apispec`) and baseline project structure.
- [x] Add initial dependencies for CLI/input plumbing (`clap`, `thiserror`, `url`).
- [x] Implement `main` startup flow and centralized error output.
- [x] Implement CLI parsing for `apispec <source>`.
- [x] Implement source classification (`LocalFile | DirectUrl | BaseUrl`).
- [x] Validate local source paths (exists + file) with clear error types.
- [x] Add initial module skeleton matching architecture (`source`, `cache`, `spec`, `tui`).
- [x] Add unit tests for source classification paths.
- [x] Satisfy Phase 1 exit criterion: command resolves input and prints source kind.

### Phase 2: Fetch + cache + strict validation

- [x] Implement local/remote loading path with refresh-on-launch behavior.
- [x] Implement cache store and metadata model.
- [x] Implement conditional HTTP refresh (`ETag` / `Last-Modified`).
- [x] Implement offline fallback from cache.
- [x] Implement parse + strict OpenAPI `3.1.0` validation.

### Phase 3: Discovery and indexing

- [x] Implement base URL discovery algorithm and candidate probing.
- [x] Build endpoint index and parameter grouping/merge logic.
- [x] Implement request/response/schema summary rendering.

### Phase 4: TUI

- [ ] Implement interactive terminal app loop and key handling.
- [ ] Implement searchable endpoint list pane.
- [ ] Implement endpoint detail pane with concise schema display.
- [ ] Surface cache/offline status in UI chrome.

### Phase 5: Hardening and release quality

- [ ] Expand unit/integration coverage for cache/discovery/validation.
- [ ] Add snapshot-style tests for schema rendering.
- [ ] Run and clean `fmt`/`clippy`/`test`.
- [ ] Finalize user-facing docs and usage examples.

---

## 17. Session Handoff Notes (After Phase 3)

Use this section when resuming after context reset.

### 17.1 Current implementation status

1. Phase 1 is complete.
2. Phase 2 is complete.
3. Phase 3 is complete for:
   1. Base URL discovery (`service-desc` header, HTML link hints, script URL hints, candidate probing).
   2. Base URL loader integration with existing cache/offline behavior.
   3. Endpoint indexing with deterministic path/method sorting.
   4. Parameter merge and grouping with operation-level override.
   5. Request/response/media type rendering and schema summaries with depth/node limits, cycle detection, and unresolved-ref placeholders.

### 17.2 Key runtime behavior (as implemented)

1. `apispec <local-file>`:
   1. Reads file.
   2. Parses + validates strict `3.1.0`.
   3. Writes cached bytes + metadata.
2. `apispec <direct-spec-url>`:
   1. Reads cache metadata if present.
   2. Sends conditional request when metadata has validators.
   3. `200` -> parse/validate + overwrite cache.
   4. `304` -> load cached bytes.
   5. Network unavailable:
      1. With cache -> load cache (`OfflineStale`).
      2. Without cache -> fail with `OfflineNoCache`.
3. `apispec <base-url>`:
   1. Tries discovery via `service-desc` hints and common framework defaults.
   2. Validates discovered candidates as strict OpenAPI `3.1.0`.
   3. Uses discovered URL for fetch/conditional refresh.
   4. Falls back to cached copy on network failure when cache exists.
4. CLI currently prints:
   1. Resolved source kind.
   2. Resolved spec source.
   3. Loaded OpenAPI version.
   4. Cache state + timestamp (when available).
   5. Indexed endpoint count.

### 17.3 Important code locations

1. Loader orchestration + base URL discovery integration: `src/spec/load.rs`.
2. Discovery implementation: `src/source/discover.rs`.
3. Strict parse/validate logic: `src/spec/validate.rs`.
4. HTTP fetch + conditional headers: `src/source/fetch.rs`.
5. Endpoint indexing and grouping: `src/spec/index.rs`.
6. Schema and media rendering summaries: `src/spec/render.rs`.
7. Cache metadata/state model: `src/cache/metadata.rs`.
8. Cache store implementation: `src/cache/store.rs`.
9. Error surface/messages: `src/error.rs`.
10. CLI output integration: `src/app.rs`.

### 17.4 Cache and environment notes

1. Default cache root:
   `ProjectDirs::from("dev", "apispec", "apispec").cache_dir()/specs`.
2. Cache key:
   `sha256(canonical_source)`.
3. Override cache root for local testing:
   set `APISPEC_CACHE_DIR=<path>`.
4. Cache metadata parse errors are intentionally tolerated (metadata becomes `None`) so stale-but-valid cached spec bytes still load.

### 17.5 Validation status at handoff

Last successful checks:

1. `cargo fmt`
2. `cargo clippy --all-targets -- -D warnings`
3. `cargo test` (`30` passing tests)

### 17.6 Suggested immediate next work (Phase 4)

1. Implement TUI app state and event loop in `src/tui/state.rs` and `src/tui/event.rs`.
2. Implement keyboard mapping (`h/j/k/l`, arrows, search shortcuts) in `src/tui/keymap.rs`.
3. Implement split-pane rendering (searchable endpoint list + detail pane) in `src/tui/view.rs`.
4. Add status/help bars with cache/offline indicators and key hints.
5. Connect indexed endpoint/search data from `spec/index.rs` into TUI interaction state.

### 17.7 Guardrails to keep

1. Keep strict OpenAPI support gate at exact `3.1.0` unless product requirement changes.
2. Preserve offline behavior:
   1. never silently fail,
   2. never ignore network failure without explicit cached fallback state.
3. Keep cache writes atomic (temp file + rename).
