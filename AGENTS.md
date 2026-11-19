# AGENTS.md — internal-dashboard

## Project at a glance

A Rust (edition 2024) internal dashboard using axum. Three surfaces share one
data-access layer (`src/db.rs`):

| Surface | Routing | Tech |
|---|---|---|
| **UI** | `GET /`, form POST/PUT/DELETE → HTMX fragments | Maud templates |
| **REST API** | `/api/v1/links[/{id}]` → JSON CRUD | utoipa (OpenAPI auto-docs) |
| **MCP** | `/mcp` → streamable-HTTP | rmcp |

Writes reach SSE clients through Postgres: the `links_notify` trigger
(migration 0004) NOTIFYs `link_events` on every insert, update and delete of
`links`, and each process's `events::listen` task forwards those into its local
broadcast channel. Handlers never publish. See "HA / Kubernetes".
**No authentication.** Leave on loopback unless a proxy guards it.

## Developer commands (use `just`, not Make)

Recipes live in the repo-root `justfile`, prefixed `dashboard-`; run them from
the repo root (the dev shell is the root one).

```sh
just --list           # all recipes (grep dashboard)
just dashboard-dev              # cargo watch -x run (restarts on change)
just dashboard-test             # full suite, starts Postgres if needed (db-ready)
just dashboard-test-unit        # lib-only tests, no database
just dashboard-test-one ui      # single integration test target
just dashboard-ci               # fmt-check → clippy → test → sorted-check → prepare-check
just dashboard-check-offline    # SQLX_OFFLINE=true cargo check (how CI compiles)
just dashboard-deploy-check     # validate the Helm chart (kubeconform needs network for schemas)
```

## sqlx: compile-time queries

- `query!` / `query_as!` are checked at compile time against a real schema.
- Results are cached in `.sqlx/` and **must be committed**.
- After changing any query:
  ```sh
  just dashboard-prepare          # regenerate .sqlx/ (needs DB up)
  just dashboard-check-offline    # verify it works without DB
  ```
- CI runs `just dashboard-prepare-check` which fails if `.sqlx/` is stale.

## Testing

- Integration tests use `#[sqlx::test]` — each test gets its own migrated
  database, isolated from others.
- All tests drive the **real router** via `internal_dashboard::build_router`
  (tests/common/mod.rs). Do not reconstruct routing in tests.
- `just dashboard-test` depends on `dashboard-db-ready`, which auto-starts Postgres via podman
  if unreachable, and leaves it running. `just dashboard-ci` and
  `just dashboard-with-db <recipes>` start it only for the run (unless one is already up),
  wait until it accepts connections, migrate it, and stop it again afterwards, also on failure.
- Postgres binds **host port 5434** (not 5432, nor 5433: that is hofvarpnir's `postgres-test`).

## Linting and formatting

- **Clippy pedantic is denied** via `Cargo.toml [lints.clippy]`. Applies
  identically to `cargo clippy`, rust-analyzer, and CI.
- The root lefthook runs alejandra, keep-sorted, `just --fmt`, shellcheck and rustfmt on commit,
  and at pre-push, when the push touches this project, the same checks as crane builds
  (`nix/checks.nix`, Postgres inside the build) plus `just dashboard-sorted-check`; an unchanged
  source finds the builds already in the store. Run `just dashboard-ci` yourself before
  committing anyway.
- `keep-sorted` blocks are enforced on pre-commit. Do not manually reorder
  lines between the start and end markers.
- `cog verify` (conventional commits) runs on commit-msg via the root lefthook.

## CI (GitHub Actions)

The public repo (`Mozart409/internal-dashboard`) runs three workflows:

- `ci.yml` runs on push to main and on pull requests: fmt, clippy, tests
  against a Postgres service, `sqlx prepare --check`, cargo-deny/audit, the
  Helm chart check (`scripts/chart-check.sh`) and `nix build .#image`.
- `release.yml` runs on a `vX.Y.Z` tag: it checks the tag against the crate
  and chart versions, creates the GitHub release from the CHANGELOG section,
  and pushes the image and the Helm chart to GHCR. Each image is smoke-tested
  before anything is published: `scripts/image-smoke.sh` runs it against
  Postgres, and `/health/ready`, `/health/live` and `/` must answer.
- `cleanup-ghcr.yml` keeps the 5 newest image releases on GHCR.

Details that matter when you change CI:

- Postgres is a service container on port **5432** in CI. Locally it is
  **5434** (compose maps 5434:5432).
- `SQLX_OFFLINE=true` when compiling, so it builds against the `.sqlx/` cache
  only.
- `mold` is installed in CI and used as the linker (`.cargo/config.toml`).

## Git workflow

- **Conventional commits** enforced by cocogitto (`cog verify`).
- **Releases in the monorepo:** `just release-bump internal-dashboard <version>`
  bumps Cargo.toml, Cargo.lock, CHANGELOG and Chart.yaml and commits on main.
  Then `just release-github internal-dashboard <version>` exports to GitHub and
  pushes the signed `vX.Y.Z` tag that starts `release.yml`.
- **Releases in the standalone GitHub clone:** `./release.sh` runs
  `cog bump --auto`. The `pre_bump_hooks` in `cog.toml` sync Cargo.toml,
  Cargo.lock and Chart.yaml, then it pushes the bump commit and the tag.
- **Chart.yaml `version` and `appVersion` always equal the crate version.**
  Both bump paths sync them. CI and release fail on drift
  (`scripts/chart-check.sh`).

## Nix

- `direnv allow` at the repo root (the project `.envrc` should just be `source_up`).
- The project flake is gone; the root flake wires `nix/` in: package
  `.#internal-dashboard` (built with the shared toolchain, `rust/toolchain.nix`),
  `nixosModules.internal-dashboard` (module + overlay),
  `overlays.internal-dashboard`, and the module tests under
  `legacyPackages.<system>.internal-dashboard.checks.{module-eval,module-vm}`.

## HA / Kubernetes

Several replicas run against one Postgres. The Helm chart is in
`deploy/helm/internal-dashboard`, and the image is `nix/image.nix` (scratch OCI,
`nix build .#internal-dashboard-image` from the root). These rules keep it
working; do not break them:

- **The trigger is the only publisher.** Handlers must not send on the broadcast
  channel. A write on one replica reaches clients on every replica only through
  `link_events`. Change what the trigger emits in a new migration, never by
  editing an applied one.
- **Keep MCP stateless.** `/mcp` has no session id and returns JSON responses,
  so any replica serves any request. Do not add session state or stateful
  transport.
- **No per-process state that must be shared.** Anything replicas must agree on
  lives in Postgres. In-memory caches, counters and locks are per pod.
- **Listener connection.** `events::listen` opens its own connection, outside
  the app pool, from `DATABASE_LISTEN_URL` or else `DATABASE_URL`'s settings.
  Never take it from the pool: `#[sqlx::test]` pools share 20 permits across
  all running tests, and a pinned connection per test starves the suite. It
  must reach Postgres directly; LISTEN does not work through pgbouncer
  transaction pooling.
- **Shutdown order.** On SIGTERM, readiness turns 503 first, SSE streams end
  (clients reconnect to another pod), then in-flight requests drain. Keep that
  order.
- **Startup migrations** run from every replica at once. sqlx's advisory lock
  serialises them, so migrations must not assume they run alone.
- **SSE tests:** subscribe before writing. NOTIFY is delivered asynchronously,
  so an event written before the subscription exists is missed.

## Key directories

| Path | What |
|---|---|
| `src/lib.rs` | Library target, `build_router()` — the single routing definition |
| `src/main.rs` | Binary: config, pool, migrations, serve |
| `src/db.rs` | Data-access layer shared by all three surfaces |
| `tests/` | Integration tests (db, api, ui, sse, mcp, smoke) |
| `tests/common/mod.rs` | Shared test harness (`spawn`, `TestApp`) |
| `migrations/` | sqlx migrations (applied at startup in main.rs) |
| `deploy/helm/internal-dashboard` | Helm chart for the HA deployment |
| `nix/` | NixOS module (`module.nix`) and package (`package.nix`) |
| `.sqlx/` | Committed sqlx query cache — **do not delete** |
