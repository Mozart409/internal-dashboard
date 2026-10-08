# AGENTS.md — internal-dashboard

## Project at a glance

A Rust (edition 2024) internal dashboard using axum. Three surfaces share one
data-access layer (`src/db.rs`):

| Surface | Routing | Tech |
|---|---|---|
| **UI** | `GET /`, form POST/PUT/DELETE → HTMX fragments | Maud templates |
| **REST API** | `/api/v1/links[/{id}]` → JSON CRUD | utoipa (OpenAPI auto-docs) |
| **MCP** | `/mcp` → streamable-HTTP | rmcp |

All publish to a shared broadcast channel — changes appear live via SSE.
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
  if unreachable.
- Postgres binds **host port 5434** (not 5432, nor 5433: that is hofvarpnir's `postgres-test`).

## Linting and formatting

- **Clippy pedantic is denied** via `Cargo.toml [lints.clippy]`. Applies
  identically to `cargo clippy`, rust-analyzer, and CI.
- The root lefthook only runs alejandra, keep-sorted, `just --fmt` and shellcheck on commit; run `just dashboard-ci` (fmt-check, clippy, tests) yourself.
- `keep-sorted` blocks are enforced on pre-commit. Do not manually reorder
  lines between the start and end markers.
- `cog verify` (conventional commits) runs on commit-msg via the root lefthook.

## CI (Woodpecker, decommissioned)

`.woodpecker.yaml` is kept for reference but is inert, and it still refers to
the removed per-project flake (`nix develop .`, `.#checks`).

- Runs on push and PR. Nix-based with Crane for cached dependency builds.
- Uses `mold` linker on Linux for faster builds (`.cargo/config.toml`).
- `SQLX_OFFLINE=true` in CI — compiles against `.sqlx/` cache only.
- Database: `postgres://dashboard:dashboard@postgres:5432/dashboard` (port
  **5432** in CI, **5434** locally — compose maps 5434:5432).
- Postgres runs with `fsync=off` for test speed.

## Git workflow

- **Conventional commits** enforced by cocogitto (`cog verify`).
- Releases (`cog bump`) and the github mirror push are gone with the project
  `cog.toml` and `justfile`; the root `cog.toml` has no bump hooks.

## Nix

- `direnv allow` at the repo root (the project `.envrc` should just be `source_up`).
- The project flake is gone; the root flake wires `nix/` in: package
  `.#internal-dashboard` (built with the shared toolchain, `rust/toolchain.nix`),
  `nixosModules.internal-dashboard` (module + overlay),
  `overlays.internal-dashboard`, and the module tests under
  `legacyPackages.<system>.internal-dashboard.checks.{module-eval,module-vm}`.

## Key directories

| Path | What |
|---|---|
| `src/lib.rs` | Library target, `build_router()` — the single routing definition |
| `src/main.rs` | Binary: config, pool, migrations, serve |
| `src/db.rs` | Data-access layer shared by all three surfaces |
| `tests/` | Integration tests (db, api, ui, sse, mcp, smoke) |
| `tests/common/mod.rs` | Shared test harness (`spawn`, `TestApp`) |
| `migrations/` | sqlx migrations (applied at startup in main.rs) |
| `nix/` | NixOS module (`module.nix`) and package (`package.nix`) |
| `.sqlx/` | Committed sqlx query cache — **do not delete** |
