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

> On an Intel Mac the shell resolves against a separate `nixpkgs-26.05-darwin`
> input, because nixpkgs 26.11 dropped x86_64-darwin. Everything else, CI
> included, stays on unstable.

## Deploying with Nix

The flake ships the dashboard as a package and a NixOS module, so another flake
can run it with a handful of lines:

```nix
{
  inputs.internal-dashboard.url = "git+ssh://forgejo@homelab-forgejo…/amadeus/internal-dashboard.git";

  outputs = { nixpkgs, internal-dashboard, ... }: {
    nixosConfigurations.homelab = nixpkgs.lib.nixosSystem {
      system = "x86_64-linux";
      modules = [
        internal-dashboard.nixosModules.default
        {
          services.internal-dashboard = {
            enable = true;
            address = "0.0.0.0";
            openFirewall = true;
          };
        }
      ];
    };
  };
}
```

That is the whole deployment. `nixosModules.default` carries the overlay that
defines `pkgs.internal-dashboard`, and `database.createLocally` — on by default
— provisions PostgreSQL, a database and a role that owns it, connected over a
peer-authenticated unix socket. No password exists to leak.

If you would rather manage the overlay yourself, import
`nixosModules.internal-dashboard` (the module alone) and add
`overlays.default` to your own `nixpkgs`. Do that also if you set
`nixpkgs.pkgs` in your configuration, which conflicts with a module adding
overlays.

### The options worth knowing

| Option | Default | Purpose |
|---|---|---|
| `address`, `port` | `127.0.0.1`, `3000` | Bind address. The dashboard has **no authentication**, so leave it on loopback unless a proxy provides some. |
| `openFirewall` | `false` | Opens `port` in the firewall. |
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

```sh
just nix-check          # or: nix flake check
```

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

just nix-build       # build the package for this system
just nix-check       # nix flake check: package, module eval, VM test on Linux

just sync-remotes    # push to forgejo, then to the github mirror
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
