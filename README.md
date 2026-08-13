# internal-dashboard

A small internal dashboard for manually curating links. Three surfaces sit on
one shared data-access layer (`src/db.rs`):

| Surface | What it is |
|---|---|
| **UI** | Server-rendered [Maud](https://maud.lambda.xyz) templates driven by HTMX, updating live over SSE |
| **REST API** | axum handlers under `/api/v1`, documented automatically with utoipa |
| **MCP server** | An `rmcp` streamable-HTTP endpoint at `/mcp`, so Claude can read and add links |

Because every surface publishes to the same broadcast channel, a link added
through the API or by Claude over MCP appears in open browser tabs immediately —
no refresh.

## Requirements

Everything is provided by the Nix devShell:

```sh
nix develop     # or `direnv allow`
```

That brings in the Rust toolchain, `just`, `sqlx-cli`, `psql` (postgresql 18),
`podman-compose`, `cargo-watch`, `lefthook` and `cocogitto`, and installs the
git hooks.

## Quickstart

```sh
cp .env.example .env    # DATABASE_URL, BIND_ADDR, RUST_LOG
just db-up              # start Postgres 18.4 and wait for it to be healthy
just mig-run            # apply migrations
just dev                # run the server, restarting on file changes
```

Then open <http://127.0.0.1:3000>.

> Postgres binds host port **5433**, not 5432, to stay out of the way of any
> other local Postgres.

## Routes

| Route | Purpose |
|---|---|
| `GET /` | Dashboard: add-link form, filter form, live link list |
| `GET /links/{id}/edit` | Edit page for one link |
| `POST /links`, `PUT /links/{id}`, `DELETE /links/{id}` | HTMX form targets, return HTML fragments |
| `GET /events` | SSE stream of `created` / `updated` / `deleted` events |
| `GET /api/v1/links` | List links; supports `?tag=` and `?q=` |
| `GET/POST/PUT/DELETE /api/v1/links[/{id}]` | JSON CRUD |
| `GET /api-docs/openapi.json` | Generated OpenAPI 3.1 document |
| `GET /scalar` | Scalar API reference |
| `POST/GET /mcp` | MCP streamable-HTTP endpoint |

## Connecting Claude to the MCP server

With the server running:

```sh
claude mcp add --transport http internal-dashboard http://127.0.0.1:3000/mcp
```

Tools exposed: `list_links`, `search_links`, `add_link`, `delete_link`.

## Common tasks

```sh
just                 # list every recipe

just db-up           # start Postgres, wait for healthy
just db-down         # stop it, keep the data
just db-reset        # destroy data and rebuild from migrations
just db-shell        # psql shell
just db-info         # version, size, table definitions

just mig-run         # apply pending migrations
just mig-info        # applied vs pending
just mig-add <name>  # scaffold a new migration
just mig-revert      # roll back the last one

just dev             # cargo watch -x run
just clippy-watch    # cargo watch clippy, warnings denied
just ci              # everything CI runs
```

## Working with sqlx

Queries are checked at **compile time** against a real schema, and the results
are cached in `.sqlx/` so CI and fresh checkouts build without a database.

After changing any `query!` / `query_as!`:

```sh
just prepare         # regenerate .sqlx/ (needs the DB up)
just check-offline   # compile the way CI does — no DB, cache only
```

`just ci` runs `prepare-check`, which fails if `.sqlx/` is stale. Commit the
`.sqlx/` directory alongside the query change.

## Lints

Clippy **pedantic is denied**, configured in `Cargo.toml` under `[lints]` so it
applies identically to `cargo clippy`, rust-analyzer and CI. A few groups are
explicitly allowed there with a comment explaining why.

Commits go through lefthook: `cargo fmt`, clippy, and `keep-sorted` on
pre-commit; tests and `cog check` on pre-push. Commit messages must be
[conventional commits](https://www.conventionalcommits.org).
