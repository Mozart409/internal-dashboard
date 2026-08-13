# Show this list of recipes
default:
    @just --list

# Build the project
build:
    cargo build

# Type-check without producing artifacts
check:
    cargo check --all-targets

# Lint with clippy, warnings denied
clippy:
    #!/usr/bin/env bash
    set -euo pipefail
    # No-op until the scaffold has a real Cargo.toml.
    if [ -f Cargo.toml ]; then
        cargo clippy --all-targets -- -D warnings
    fi

# Format the code
fmt:
    cargo fmt --all

# Run the full test suite
test:
    #!/usr/bin/env bash
    set -euo pipefail
    # No-op until the scaffold has a real Cargo.toml.
    if [ -f Cargo.toml ]; then
        cargo test --all-targets
    fi

# Generate CHANGELOG.md from conventional commits
changelog:
    cog changelog

# Check formatting without changing files
fmt-check:
    #!/usr/bin/env bash
    set -euo pipefail
    # No-op until the scaffold has a real Cargo.toml.
    if [ -f Cargo.toml ]; then
        cargo fmt --all -- --check
    fi

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
ci: fmt-check clippy test sorted-check
    cog check
