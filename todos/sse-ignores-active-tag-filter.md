# SSE re-render discards the client's active tag or search filter

**Severity:** medium — silent, and confusing when it happens.
**Found by:** the MCP/SSE test agent while writing `tests/sse.rs`.

## Problem

`src/sse.rs:37` re-renders the whole list on every event, always unfiltered:

```rust
let links = crate::db::list_links(&pool, None).await.ok()?;
```

The rendered `#link-list` is pushed to **every** subscriber and replaces their
existing list via `hx-swap="outerHTML"`. So a browser sitting on `/?tag=rust` or
`/?q=proxmox` has its filtered view silently replaced by the complete list the
moment anyone adds, edits or deletes a link.

The user sees their filter apparently reset itself, while the URL bar still shows
the filter — so a reload restores it and the bug looks intermittent.

## Why it is not trivial

The broadcast channel is global, but the filter is per-connection. The event
carries no notion of who is listening, and one rendered payload is currently
shared by all subscribers.

Options, roughly in order of preference:

1. **Filter per subscriber.** Read `tag`/`q` from the `/events` query string
   (the UI would connect to `/events?tag=rust`), and have each subscriber's
   stream render with its own filter. Keeps the server authoritative and the
   payload correct per client. Costs one query per event *per subscriber*.
2. **Send an event notification only**, and let HTMX re-fetch the list itself
   with its existing filter (`hx-get` on the current URL, triggered by the SSE
   event). Payload becomes tiny and the filter problem disappears, at the cost
   of an extra round trip.
3. Send per-row fragments plus an id, and let the client decide — most efficient,
   most complex, and needs the client to know whether a row matches its filter.

Option 2 is probably the best fit for this codebase: it removes the duplicated
rendering path entirely and makes the filter the client's business, which it
already is.

## Verify

Add to `tests/sse.rs`:

- open `/events` as a client filtered to `tag=a`, create a link tagged `b`, and
  assert the pushed payload does **not** contain the `b` link
- two subscribers with different filters each receive a payload consistent with
  their own filter
- the existing unfiltered cross-client test keeps passing

Related: [[stored-xss-via-unvalidated-url-update]], [[maud-error-page-and-fallback]]
