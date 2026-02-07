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

## Keyboard Controls

Normal mode:
- `j` / `k` or Down / Up: move selection
- `g` / `G`: first / last endpoint
- `PageUp` / `PageDown`: jump
- `h` / `l` or Left / Right: detail scroll
- `/` or `Ctrl+s`: enter search mode
- `Ctrl+u`: clear filter
- `q`: quit

Search mode:
- Type to filter by method/path/summary/operationId
- `Backspace`: delete
- `Ctrl+u`: clear query
- `Enter` or `Esc`: return to normal mode

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
