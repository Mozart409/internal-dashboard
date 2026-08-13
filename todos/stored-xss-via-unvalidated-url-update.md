# Stored XSS: `update` paths do not validate the URL scheme

**Severity:** high — this is the one to fix first.
**Found by:** the MCP/SSE test agent while writing `tests/mcp.rs`.

## Problem

Creating a link validates the URL, but **updating one does not**. Three surfaces
disagree:

| Surface | Update path | Validates? |
|---|---|---|
| REST | `src/api.rs:131` `update_link` | **no** |
| UI form | `src/ui.rs:580` `update_link` | **no** |
| MCP | `src/mcp.rs:145` `update_link` | yes — checks the scheme inline |

`db::update_link` does not validate either, and `NewLink::validate` (the function
holding the `http://` / `https://` rule, `src/models.rs`) is only ever called on
the create paths.

The stored value is rendered straight into an anchor at `src/ui.rs:391`:

```rust
a href=(link.url) target="_blank" rel="noopener noreferrer" { (link.title) }
```

Maud escapes HTML, so this is not attribute-injection — but escaping does nothing
about the *scheme*. A stored `javascript:` URL executes on click.

## Reproduce

```sh
ID=$(curl -s -XPOST localhost:3000/api/v1/links \
  -H 'content-type: application/json' \
  -d '{"url":"https://ok.dev","title":"ok"}' | jq -r .id)

# accepted, though the identical write is rejected over MCP
curl -s -XPUT localhost:3000/api/v1/links/$ID \
  -H 'content-type: application/json' \
  -d '{"url":"javascript:alert(document.domain)"}'

curl -s localhost:3000/ | grep 'href="javascript:'
```

## Fix

Validation belongs in one place that every surface goes through, rather than
being repeated per-surface — the current duplication is exactly why two of three
surfaces missed it.

1. Extract the scheme check from `NewLink::validate` into a shared
   `models::validate_url(&str) -> Result<(), AppError>`.
2. Call it from `UpdateLink` as well as `NewLink` — ideally by giving
   `UpdateLink` its own `validate(&mut self)` that checks `url` when `Some`.
3. Have `api.rs`, `ui.rs` and `mcp.rs` update handlers all call it, then delete
   the now-duplicated inline check in `mcp.rs`.
4. Consider rejecting at the `db` layer too, as a backstop.

Worth deciding separately: an allow-list of schemes (`http`, `https`) is safer
than a deny-list, since `data:`, `vbscript:` and friends are equally dangerous.

## Verify

Add to `tests/api.rs` and `tests/ui.rs`:

- `PUT` with `javascript:alert(1)` → 400, and the stored link is unchanged
- same for `data:text/html,...`
- a valid `https://` update still succeeds
- `tests/mcp.rs` already covers the MCP side; keep it passing after the
  refactor removes the inline check

Related: [[sse-ignores-active-tag-filter]], [[maud-error-page-and-fallback]]
