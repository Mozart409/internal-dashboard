# internal-dashboard

[![CI status](https://ci.homelab.internal/api/badges/2/status.svg)](https://ci.homelab.internal/repos/2)

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

Everything is provided by the yggdrasil root Nix devShell:

```sh
direnv allow    # at the repo root, or `nix develop` there
```

Outside the monorepo (the GitHub mirror), this directory's own `flake.nix`
has a smaller dev shell (`nix develop`) plus `packages.default`,
`overlays.default` and `nixosModules.{default,internal-dashboard}`, with the
same meaning as the root outputs described below.

That brings in the Rust toolchain, `just`, `sqlx-cli`, `psql`,
`podman-compose`, `cargo-watch`, `lefthook` and `cocogitto`, and installs the
git hooks (the root ones). All recipes are root recipes prefixed `dashboard-`
and run from the repo root: `just dashboard-db-up`, `just dashboard-dev`, ...

## Deploying with Nix

The yggdrasil root flake ships the dashboard as a package and a NixOS module.
A host in the monorepo enables it with:

```nix
{
  imports = [self.nixosModules.internal-dashboard];
  services.internal-dashboard = {
    enable = true;
    address = "0.0.0.0";
    openFirewall = true;
  };
}
```

That is the whole deployment. `nixosModules.internal-dashboard` carries the
overlay that defines `pkgs.internal-dashboard`, and `database.createLocally` —
on by default — provisions PostgreSQL, a database and a role that owns it,
connected over a peer-authenticated unix socket. No password exists to leak.

If you would rather manage the overlay yourself, import
`rust/internal-dashboard/nix/module.nix` (the module alone) and add
`overlays.internal-dashboard` to your own `nixpkgs`. Do that also if you set
`nixpkgs.pkgs` in your configuration, which conflicts with a module adding
overlays.

### The options worth knowing

| Option | Default | Purpose |
|---|---|---|
| `address`, `port` | `127.0.0.1`, `3000` | Bind address. The dashboard has **no authentication**, so leave it on loopback unless a proxy provides some. |
| `openFirewall` | `false` | Opens `port` in the firewall. |
| `mcpAllowedHosts` | `[ ]` | `Host` header values `/mcp` accepts beyond rmcp's loopback-only default. Set this to whatever hostname fronts the dashboard (a Caddy vhost, an MCP aggregator) or every request through it gets a 403. |
| `database.createLocally` | `true` | Provision PostgreSQL here. Turn it off and set `database.url`, or `DATABASE_URL` in `environmentFile`, to use a server you already run. |
| `database.tuning.memoryMB` | `1024` | The memory budget PostgreSQL may treat as its own. `shared_buffers`, `effective_cache_size`, `maintenance_work_mem` and `work_mem` are all derived from it, so it is the one number to change. |
| `database.tuning.statementTimeout` | `30s` | Also applies to migrations — raise it before adding one that indexes a large table. |
| `database.settings` | `{}` | Raw `postgresql.conf` settings; these beat anything `tuning` sets. |
| `database.extensions` | `[ "pg_trgm" ]` | Created as the superuser before the service starts. |
| `database.pgbouncer.enable` | `false` | Puts pgbouncer on its own peer-authenticated socket in front of PostgreSQL. |
| `pool.maxConnections` | `10` | The dashboard's own pool. |
| `environment`, `environmentFile` | `{}`, `null` | Escape hatches. `environmentFile` is where a `DATABASE_URL` with a password belongs. |

The service runs as an unprivileged user under `ProtectSystem=strict` with no
writable paths at all — migrations and the htmx assets are compiled into the
binary, so it needs none.

On pgbouncer: it is off by default and worth leaving that way for a single
process holding one small pool over a local socket. When it is on, the pool
mode is `transaction` with `max_prepared_statements = 200`, because sqlx
prepares every statement it runs and transaction pooling would otherwise break
them.

### Testing the module

Both live under `legacyPackages.<system>.internal-dashboard.checks` of the root
flake (not `checks`, so a flake check does not boot VMs):
`nix build .#legacyPackages.x86_64-linux.internal-dashboard.checks.module-eval`.
In the standalone flake they are plain `checks` (`nix build .#checks.x86_64-linux.module-eval`).

`checks.module-eval` evaluates the module into a full NixOS system and asserts
on the result — 32 checks covering the connection string, the systemd unit, the
derived Postgres settings, the pgbouncer wiring and every assertion the module
can raise. It is pure evaluation, so it runs on macOS too.

`checks.module-vm` boots two NixOS VMs and uses the service for real, one
straight against PostgreSQL and one through pgbouncer. It needs a Linux
builder, so it is only exposed on Linux — **and it has not been run yet**,
since the machine this module was written on has none.

## Quickstart

```sh
cp .env.example .env    # DATABASE_URL, BIND_ADDR, RUST_LOG
just dashboard-db-up              # start Postgres 18.4 and wait for it to be healthy
just dashboard-mig-run            # apply migrations
just dashboard-dev                # run the server, restarting on file changes
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
| `GET /docs/openapi.json` | Generated OpenAPI 3.1 document |
| `GET /docs` | Scalar API reference |
| `GET /health` | Health check; checks PostgreSQL |
| `GET /health/live` | Liveness probe |
| `GET /health/ready` | Readiness probe; checks PostgreSQL |
| `POST/GET /mcp` | MCP streamable-HTTP endpoint |

## Connecting Claude to the MCP server

With the server running:

```sh
claude mcp add --transport http internal-dashboard http://127.0.0.1:3000/mcp
```

Tools exposed: `list_links`, `search_links`, `add_link`, `delete_link`.

## Common tasks

```sh
just --list | grep dashboard   # list every recipe

just dashboard-db-up           # start Postgres, wait for healthy
just dashboard-db-down         # stop it, keep the data
just dashboard-db-reset        # destroy data and rebuild from migrations
just dashboard-db-shell        # psql shell
just dashboard-db-info         # version, size, table definitions

just dashboard-mig-run         # apply pending migrations
just dashboard-mig-info        # applied vs pending
just dashboard-mig-add <name>  # scaffold a new migration
just dashboard-mig-revert      # roll back the last one

just dashboard-dev             # cargo watch -x run
just dashboard-clippy-watch    # cargo watch clippy, warnings denied
just dashboard-ci              # everything CI runs
```

## Configuration

Read from the environment, or from `.env` in the working directory:

| Variable | Default | Purpose |
|---|---|---|
| `DATABASE_URL` | `postgres://dashboard:dashboard@localhost:5433/dashboard` | Connection string |
| `BIND_ADDR` | `127.0.0.1:3000` | Listen address |
| `RUST_LOG` | `internal_dashboard=debug,tower_http=debug,info` | Log filter |
| `DB_MAX_CONNECTIONS` | `10` | Pool size |
| `DB_ACQUIRE_TIMEOUT_SECS` | `5` | How long a request waits for a pooled connection |

A malformed numeric value fails startup rather than falling back to the
default.

## Working with sqlx

Queries are checked at **compile time** against a real schema, and the results
are cached in `.sqlx/` so CI and fresh checkouts build without a database.

After changing any `query!` / `query_as!`:

```sh
just dashboard-prepare         # regenerate .sqlx/ (needs the DB up)
just dashboard-check-offline   # compile the way CI does — no DB, cache only
```

`just dashboard-ci` runs `dashboard-prepare-check`, which fails if `.sqlx/` is stale. Commit the
`.sqlx/` directory alongside the query change.

## Lints

Clippy **pedantic is denied**, configured in `Cargo.toml` under `[lints]` so it
applies identically to `cargo clippy`, rust-analyzer and CI. A few groups are
explicitly allowed there with a comment explaining why.

Commits go through the root lefthook (alejandra, keep-sorted, `just --fmt`, shellcheck, `cog verify`);
run `just dashboard-ci` for the project's own fmt/clippy/test gate. Formerly the project hooks ran: `cargo fmt`, clippy, and `keep-sorted` on
pre-commit; tests and `cog check` on pre-push. Commit messages must be
[conventional commits](https://www.conventionalcommits.org).
