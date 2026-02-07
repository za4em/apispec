# apispec

`apispec` is a Rust CLI/TUI for read-only exploration of OpenAPI specs.

It accepts:
- Local spec file path
- Direct OpenAPI URL
- Base API URL (with automatic spec discovery)

Only OpenAPI `3.1.0` is supported.

## Build

```bash
cargo build
```

## Usage

```bash
apispec <source>
```

`<source>` can be:
- `./openapi.yaml`
- `https://api.example.com/openapi.json`
- `https://api.example.com` (discovery mode)

### Examples

Run interactive TUI:

```bash
apispec ./openapi.yaml
apispec https://petstore3.swagger.io/api/v3/openapi.json
apispec https://petstore3.swagger.io/api/v3
```

Run in non-interactive mode for diagnostics/CI:

```bash
apispec --no-tui https://api.example.com/openapi.json
```

Disable alternate screen (or use `APISPEC_NO_ALT_SCREEN=1`):

```bash
apispec --no-alt-screen https://api.example.com/openapi.json
```

Disable TUI with env var:

```bash
APISPEC_NO_TUI=1 apispec https://api.example.com/openapi.json
```

## Discovery Behavior

When a base URL is provided, `apispec` tries discovery in this order:
1. `Link` headers with `rel=service-desc`
2. HTML `<link rel="service-desc" ...>` hints
3. Common Swagger/ReDoc script URL hints
4. Known endpoint probes:
   - `/openapi.json`
   - `/openapi.yaml`
   - `/openapi.yml`
   - `/v3/api-docs`
   - `/v3/api-docs.yaml`
   - `/swagger/v1/swagger.json`
   - `/swagger.json`
   - `/swagger.yaml`

The first valid OpenAPI `3.1.0` document is used.

## Cache and Offline Behavior

`apispec` caches specs under the platform cache directory and attempts network refresh on every launch.

Remote sources:
- `200`: cache is replaced (`Source: fresh`)
- `304`: cached bytes are reused (`Source: cached (not modified)`)
- Network unavailable + cached copy exists: runs offline with cache (`Source: offline, using cached copy ...`)
- Network unavailable + no cache: exits with explicit error

Local file sources are re-read and cache-refreshed on each launch.

## Strict Version Gate

Any version other than `3.1.0` is rejected.

Example:

```text
Unsupported OpenAPI version `3.1.1`. This tool currently supports only `3.1.0`.
```

## Endpoint Tree and Grouping

The left panel is a tree:
- Top-level rows are groups.
- Child rows are endpoints (`METHOD path` with optional summary text).

Grouping rules:
- First non-empty operation tag.
- If no tags, first meaningful path segment (skips `/` and `{param}`-only segments).
- If still unavailable, `Untagged`.

Ordering:
- Groups are alphabetical (case-insensitive) with `Untagged` forced last.
- Endpoints are ordered by path, then method rank (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `HEAD`, `TRACE`).

Filtering:
- Search is case-insensitive and token-based.
- Matches include group name, method, path, title/summary, operationId, description, and tags.
- While filtering is active, only matching groups/endpoints are shown and matching groups auto-expand.

## Keyboard Controls

Normal mode global:
- `q` or `Ctrl+c`: quit
- `/` or `Ctrl+s`: enter search mode
- `Ctrl+u`: clear search filter

Tree focus (left panel):
- `j` / `k` or Down / Up: move row selection
- `g` / `G`: first / last visible row
- `PageUp` / `PageDown`: jump by page
- Left / Right: collapse/expand selected group (disabled while filtering)
- `Enter`:
  - On group row: toggle expand/collapse
  - On endpoint row: open endpoint details and switch focus to details panel

Details focus (right panel):
- `j` / `k` or Down / Up: scroll details
- `h` / `l` or Left / Right: alternate detail scroll keys
- `PageUp` / `PageDown`: scroll by page
- `Tab`: jump to next section (`Description`, `Parameters`, `Request Body`, `Responses`, `Security`)
- `Enter`: toggle nearest expandable detail row
- `Esc`: return focus to tree panel

Search mode:
- Type to update filter immediately
- `Backspace`: delete one character
- `Ctrl+u`: clear query
- `Enter` or `Esc`: return to normal mode

## Details and Expansion Behavior

The details panel is sectioned and styled for scanability:
- Header: `METHOD path` (method-colored)
- Sections: Description, Parameters, Request Body, Responses, Security
- Parameters are rendered in a table-like row format: name, location, required, type, description
- Response rows style status codes by class (`2xx`, `4xx`, `5xx`, etc.)

Expandable rows in details:
- Request body block
- Request body media types (for example `application/json`)
- Each response status block (for example `200`)
- Response media types
- Nested schema nodes that have children

Schema rendering includes:
- Type labels and required markers
- Enum values and compact examples when available
- Reference hints (`[ref:Name]`)
- Safe placeholders for cycles and unresolved refs
- Breadcrumb line at the top of details when focused

## Error Behavior

Errors are actionable and include context.

Examples:

```text
Could not discover an OpenAPI spec from base URL `https://api.example.com`.
Tried: https://api.example.com/openapi.json, https://api.example.com/v3/api-docs.
If your spec lives elsewhere, pass it directly: apispec <full-spec-url>.
```

```text
Network unavailable while fetching `https://api.example.com/openapi.json`, and no cached spec is available (...).
```

```text
Could not parse OpenAPI document from `...` as JSON or YAML.
JSON error: ...
YAML error: ...
```
