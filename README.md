# apispec

`apispec` is a Rust CLI/TUI for read-only inspection of OpenAPI specs.

It accepts:
- Local spec file path
- Direct spec URL
- Base API URL (with automatic discovery)

Only OpenAPI `3.1.0` is supported.

## Build

```bash
cargo build
```

Run directly with Cargo:

```bash
cargo run -- <source>
```

## Usage

```bash
apispec [OPTIONS] <source>
```

Arguments:
- `<source>`: local file path, direct spec URL, or base API URL

Options:
- `--no-tui`: disable interactive UI and print a plain endpoint summary
- `--no-alt-screen`: run TUI without entering alternate screen

Environment toggles:
- `APISPEC_NO_TUI=1`: same effect as `--no-tui`
- `APISPEC_NO_ALT_SCREEN=1`: same effect as `--no-alt-screen`

Note: alternate screen is also auto-disabled when `TERM_PROGRAM=ghostty`.

## Examples

Interactive TUI:

```bash
apispec ./openapi.yaml
apispec https://petstore3.swagger.io/api/v3/openapi.json
apispec https://petstore3.swagger.io/api/v3
```

Non-interactive mode (CI/diagnostics):

```bash
apispec --no-tui https://api.example.com/openapi.json
```

## Source Classification

`apispec` classifies input into one of three source kinds:
- `LocalFile`
- `DirectUrl`
- `BaseUrl`

URL inputs are treated as `DirectUrl` when the URL path or query looks spec-like (contains one of: `.json`, `.yaml`, `.yml`, `openapi`, `swagger`, `api-docs`).
Otherwise they are treated as `BaseUrl` and discovery is used.

## Discovery Behavior (Base URLs)

For base URLs, discovery works as follows:
1. Probe the base URL itself. If it already returns a valid OpenAPI `3.1.0` document, use it.
2. Parse `Link` headers with `rel=service-desc`.
3. Parse HTML `<link rel="service-desc" href="...">` hints.
4. Parse script/url hints in page content.
5. Probe known candidate paths.

Known candidate paths:
- `/openapi.json`
- `/openapi.yaml`
- `/openapi.yml`
- `/v3/api-docs`
- `/v3/api-docs.yaml`
- `/swagger/v1/swagger.json`
- `/swagger.json`
- `/swagger.yaml`

Candidates are attempted for both the base path prefix and the site root (for example, both `/api/openapi.json` and `/openapi.json` when base is `https://host/api`).

## Caching and Offline Behavior

Specs are cached by canonical source key.

Cache location:
- Default: platform cache directory via `directories::ProjectDirs("dev", "apispec", "apispec")`, under `specs/`
- Override: set `APISPEC_CACHE_DIR`

Remote source behavior:
- HTTP `200`: cache updated, status `fresh`
- HTTP `304`: cached bytes reused, status `cached (not modified)`
- Network unavailable + cache exists: run from cache, status `offline, using cached copy`
- Network unavailable + no cache: exit with explicit error

Local file behavior:
- File is re-read every run
- Parsed content is written into cache as `fresh`

## Non-Interactive Output

In `--no-tui` mode, output includes:
- Resolved source kind
- Resolved spec source URL/path
- Loaded OpenAPI version
- Source/cache status (with cached timestamp when available)
- Indexed endpoint count
- Endpoint preview (up to 40 entries, with `operationId` when present)

## Endpoint Tree and Grouping

Tree groups are derived by:
1. First non-empty operation tag
2. Else first meaningful path segment
3. Else `Untagged`

Sorting:
- Groups: case-insensitive alphabetical, with `Untagged` forced last
- Endpoints: by path, then method order (`GET`, `POST`, `PUT`, `PATCH`, `DELETE`, `OPTIONS`, `HEAD`, `TRACE`)

## Filtering

Search is case-insensitive and token-based (split on whitespace).
All tokens must match (AND behavior).

Search text includes:
- Group name
- HTTP method
- Path
- Title/summary
- `operationId`
- Description
- Tags

While filtering:
- Only matching groups/endpoints are shown
- Matching groups auto-expand
- Manual group toggle is disabled

## Details Panel

Details are rendered in sections:
- Description
- Parameters
- Request Body
- Responses
- Security

Current limitation:
- Security section is currently a placeholder (`No security details indexed.`)

Expansion/toggles:
- Request body row
- Request body media type rows
- Response status rows
- Response media type rows
- Nested schema nodes

Schema rendering includes:
- Type labels and required markers
- Enum values
- Example values (pretty-printed JSON when applicable)
- Reference hints (`[ref:<name>]`)
- Safe placeholders for unresolved refs and cycles
- Breadcrumb path for active detail row

## Keyboard Controls

Normal mode (global):
- `q` or `Ctrl+c`: quit
- `/` or `Ctrl+s`: enter search mode
- `Ctrl+u`: clear search

Tree focus:
- `j` / `k` or Down / Up: move selection
- `g` / `G`: first / last row
- `PageUp` / `PageDown`: jump by page
- Left / Right: toggle selected group (disabled while filtering)
- `Enter`:
  - Group row: expand/collapse
  - Endpoint row: open details and focus details panel

Details focus:
- `j` / `k` or Down / Up: move detail row cursor
- `h` / `l` or Left / Right: scroll details
- `PageUp` / `PageDown`: scroll by page
- `Tab`: jump to next section
- `Enter`: toggle nearest expandable row
- `Esc`: return focus to tree

Search mode:
- Type to filter live
- `Backspace`: delete character
- `Ctrl+u`: clear query
- `Enter` or `Esc`: return to normal mode

## Error Behavior

Errors are explicit and contextual (invalid sources, discovery failures, fetch/network issues, parse failures, unsupported version).

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
Unsupported OpenAPI version `3.1.1`. This tool currently supports only `3.1.0`.
```
