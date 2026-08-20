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

```sh
just                  # list all recipes
just dev              # cargo watch -x run (restarts on change)
just test             # full suite, starts Postgres if needed (db-ready)
just test-unit        # lib-only tests, no database
just test-one ui      # single integration test target
just ci               # fmt-check → clippy → test → sorted-check → prepare-check → cog check
just check-offline    # SQLX_OFFLINE=true cargo check (how CI compiles)
```

## sqlx: compile-time queries

- `query!` / `query_as!` are checked at compile time against a real schema.
- Results are cached in `.sqlx/` and **must be committed**.
- After changing any query:
  ```sh
  just prepare          # regenerate .sqlx/ (needs DB up)
  just check-offline    # verify it works without DB
  ```
- CI runs `just prepare-check` which fails if `.sqlx/` is stale.

## Testing

- Integration tests use `#[sqlx::test]` — each test gets its own migrated
  database, isolated from others.
- All tests drive the **real router** via `internal_dashboard::build_router`
  (tests/common/mod.rs). Do not reconstruct routing in tests.
- `just test` depends on `db-ready`, which auto-starts Postgres via podman
  if unreachable.
- Postgres binds **host port 5433** (not 5432) to avoid conflicts.

## Linting and formatting

- **Clippy pedantic is denied** via `Cargo.toml [lints.clippy]`. Applies
  identically to `cargo clippy`, rust-analyzer, and CI.
- `cargo fmt` and clippy run on **pre-commit** via lefthook.
- `keep-sorted` blocks are enforced on pre-commit. Do not manually reorder
  lines between the start and end markers.
- `cog check` (conventional commits) runs on **pre-push**.

## CI (Woodpecker)

- Runs on push and PR. Nix-based with Crane for cached dependency builds.
- Uses `mold` linker on Linux for faster builds (`.cargo/config.toml`).
- `SQLX_OFFLINE=true` in CI — compiles against `.sqlx/` cache only.
- Database: `postgres://dashboard:dashboard@postgres:5432/dashboard` (port
  **5432** in CI, **5433** locally — compose maps 5433:5432).
- Postgres runs with `fsync=off` for test speed.

## Git workflow

- **Conventional commits** enforced by cocogitto (`cog verify`).
- `just release` or `just release <version>` — creates an annotated tag,
  bumps Cargo.toml, regenerates `.sqlx/`, and pushes to forgejo (primary)
  + github (mirror).
- `just sync-remotes` — pushes branch and tags to both remotes.

## Nix

- `nix develop` or `direnv allow` for the dev shell.
- `just nix-check` = `nix flake check` (package build, module eval, VM test on Linux).
- The flake ships as both a package and a NixOS module (`nixosModules.default`).

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
