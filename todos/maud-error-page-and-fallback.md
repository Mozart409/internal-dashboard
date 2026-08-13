# No HTML error page, and no router fallback

**Severity:** low — cosmetic, but it is the first thing a user sees when something
goes wrong.

## Problem

Two gaps, both in error presentation rather than error handling.

**1. `AppError` always renders JSON.** `src/error.rs` `into_response` returns a
JSON body for every surface. The same `AppError` is returned by `api.rs` *and*
`ui.rs` handlers, so a browser hitting `/links/{missing}/edit` gets

```json
{"error":"not found"}
```

as raw text on screen, instead of a page.

**2. Nothing is wired to `fallback`.** `build_router` in `src/lib.rs` has no
`.fallback(...)`, so any unmatched URL returns axum's built-in 404: status only,
empty body, no styling.

## Fix

The constraint is that the JSON API contract must not change — `tests/api.rs`
asserts a JSON `error` field on 400 and 404 — so this has to be content
negotiation, not a wholesale switch to HTML.

1. Add `ui::error_page(status: StatusCode, message: &str) -> Markup`, reusing the
   existing `layout()` so errors inherit the dashboard styling and the vendored
   assets.
2. In `error.rs`, negotiate on the `Accept` header: prefer HTML when the client
   asks for `text/html` (browsers do), otherwise keep the current JSON. Note the
   `IntoResponse` impl currently has no access to the request — the cleanest way
   is a small middleware, or an extractor-carried flag, rather than reaching for
   a thread-local.
3. Add `.fallback(ui::not_found)` in `build_router`.
4. Keep the existing behaviour where 500s log the real error and return a generic
   message — do not leak database internals into the HTML page either.

## Verify

Add to `tests/ui.rs` and `tests/api.rs`:

- `GET /nonexistent` with `Accept: text/html` → 404 **and** an HTML body
  containing the layout
- `GET /api/v1/nonexistent` with `Accept: application/json` → 404 with a JSON
  `error` field
- `GET /links/{random-uuid}/edit` in a browser-shaped request renders the error
  page rather than JSON
- every existing `tests/api.rs` assertion about JSON error bodies still passes —
  this is the regression risk

Related: [[stored-xss-via-unvalidated-url-update]], [[sse-ignores-active-tag-filter]]
