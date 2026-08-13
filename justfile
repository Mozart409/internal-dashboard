# Load .env if present, so DATABASE_URL is available to sqlx and the app.
set dotenv-load := true
set unstable := true

# Fallback matches compose.yaml, so the recipes work before .env is created.
export DATABASE_URL := env_var_or_default("DATABASE_URL", "postgres://dashboard:dashboard@localhost:5433/dashboard")

container := "internal-dashboard-db"

# Show this list of recipes
default:
    @just --list

# Build the project
build:
    cargo build

# Type-check without producing artifacts
check:
    cargo check --all-targets

# Lint with clippy, warnings denied (pedantic is denied via Cargo.toml [lints])
clippy:
    cargo clippy --all-targets -- -D warnings

# Re-run clippy on every file change
clippy-watch:
    cargo watch -c -x 'clippy --all-targets -- -D warnings'

# Format the code
fmt:
    cargo fmt --all

# Run the full test suite, starting Postgres first if it is not already up
test: db-ready
    cargo test --all-targets

# Only the tests that need no database (fast)
test-unit:
    cargo test --lib

# Run one integration target, e.g. `just test-one ui`
test-one name: db-ready
    cargo test --test {{ name }}

# Re-run the full suite on every change
test-watch: db-ready
    cargo watch -c -x 'test --all-targets'

# Run the server once
run:
    cargo run

# Run the server, restarting on every file change
dev:
    cargo watch -c -x run

# Generate CHANGELOG.md from conventional commits
changelog:
    cog changelog

# Push the current branch and its tags to forgejo, then to the github mirror
sync-remotes:
    #!/usr/bin/env bash
    set -euo pipefail
    branch="$(git rev-parse --abbrev-ref HEAD)"
    # origin is forgejo and is the primary: if it rejects the push, stop here
    # rather than leaving the mirror ahead of the source of truth.
    echo "==> origin (forgejo, primary)"
    git push --follow-tags origin "$branch"
    echo "==> github (mirror)"
    git push --follow-tags github "$branch"

# Check formatting without changing files
fmt-check:
    cargo fmt --all -- --check

# Verify keep-sorted blocks are sorted
sorted-check:
    #!/usr/bin/env bash
    set -euo pipefail
    # keep-sorted takes files, never a directory — feed it the tracked files.
    if git rev-parse --git-dir >/dev/null 2>&1; then
        mapfile -t files < <(git ls-files)
    else
        mapfile -t files < <(find . -type f -not -path './.git/*')
    fi
    keep-sorted --mode lint "${files[@]}"

# Run every check that CI runs (check mode, nothing mutates)
ci: fmt-check clippy test sorted-check prepare-check
    cog check

# --- vendored frontend assets -----------------------------------------------

htmx_version := "2.0.4"
htmx_sse_version := "2.2.2"

# Re-download htmx into static/. They are embedded at compile time via
# include_str!, so the dashboard works with no network at runtime — this recipe
# is only needed to upgrade the pinned versions above.
vendor-assets:
    #!/usr/bin/env bash
    set -euo pipefail
    mkdir -p static
    curl -sSLf --max-time 60 -o static/htmx.min.js \
        "https://unpkg.com/htmx.org@{{ htmx_version }}/dist/htmx.min.js"
    curl -sSLf --max-time 60 -o static/sse.js \
        "https://unpkg.com/htmx-ext-sse@{{ htmx_sse_version }}/sse.js"
    # A CDN error page would also be written happily, so check it looks like JS.
    for f in static/htmx.min.js static/sse.js; do
        if grep -qi '<!doctype html\|<html' "$f"; then
            echo "$f looks like HTML, not JavaScript — download failed" >&2
            exit 1
        fi
    done
    echo "vendored htmx {{ htmx_version }} and htmx-ext-sse {{ htmx_sse_version }}"

# --- database ---------------------------------------------------------------

# Start Postgres and wait until it accepts connections
db-up:
    #!/usr/bin/env bash
    set -euo pipefail
    podman compose up -d
    echo "waiting for {{ container }} to become healthy..."
    for _ in $(seq 1 60); do
        if [ "$(podman inspect --format '{{{{.State.Health.Status}}}}' {{ container }} 2>/dev/null)" = "healthy" ]; then
            echo "ready on $DATABASE_URL"
            exit 0
        fi
        sleep 1
    done
    echo "timed out waiting for postgres" >&2
    podman logs --tail 30 {{ container }} >&2
    exit 1

# Start Postgres only if it is not already accepting connections.
# Cheap enough to depend on from `test`, so a stopped container never blocks a
# test run or a push.
db-ready:
    #!/usr/bin/env bash
    set -euo pipefail
    # Probe over the network rather than via podman, so this also succeeds in CI
    # where Postgres is a service container and podman does not exist.
    if pg_isready -d "$DATABASE_URL" >/dev/null 2>&1; then
        exit 0
    fi
    if ! command -v podman >/dev/null 2>&1; then
        echo "postgres is unreachable at $DATABASE_URL and podman is unavailable" >&2
        exit 1
    fi
    echo "postgres is not accepting connections — starting it"
    just db-up

# Stop Postgres, keeping the data volume
db-down:
    podman compose down

# Destroy the container AND its data, then rebuild from migrations
db-reset:
    podman compose down -v
    @just db-up
    @just mig-run

# Tail the Postgres logs
db-logs:
    podman logs -f {{ container }}

# Open a psql shell against the dev database
db-shell:
    podman exec -it {{ container }} psql -U dashboard -d dashboard

# Server version, database size, and the links table definition
db-info:
    #!/usr/bin/env bash
    set -euo pipefail
    podman exec {{ container }} psql -U dashboard -d dashboard -c 'select version();'
    podman exec {{ container }} psql -U dashboard -d dashboard -c \
        "select pg_size_pretty(pg_database_size('dashboard')) as size;"
    podman exec {{ container }} psql -U dashboard -d dashboard -c '\dt'
    podman exec {{ container }} psql -U dashboard -d dashboard -c '\d links'

# --- migrations -------------------------------------------------------------

# Apply all pending migrations
mig-run:
    sqlx migrate run

# Show which migrations are applied and which are pending
mig-info:
    sqlx migrate info

# Revert the most recent migration (needs a matching .down.sql)
mig-revert:
    sqlx migrate revert

# Scaffold a new migration: just mig-add add_favicon_column
mig-add name:
    sqlx migrate add {{ name }}

# --- sqlx offline cache -----------------------------------------------------

# Regenerate .sqlx/ from a live database — run after changing any query!
prepare:
    cargo sqlx prepare

# Fail if .sqlx/ is stale (this is what CI enforces)
prepare-check:
    cargo sqlx prepare --check

# Compile the way CI does: no database, cache only
check-offline:
    SQLX_OFFLINE=true cargo check --all-targets
