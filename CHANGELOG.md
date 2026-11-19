# Changelog
All notable changes to this project will be documented in this file. See [conventional commits](https://www.conventionalcommits.org/) for commit guidelines.

- - -
## v0.3.0 - 2026-11-19
#### Features
- (**dashboard**) smoke test the release image against postgres before publishing - (cfecdb8) - Amadeus Mader
- (**dashboard**) add github ci and release workflows with cog bump and helm chart publishing - (4067d34) - Amadeus Mader
- (**dashboard**) run highly available on kubernetes with a helm chart and postgres notify fan-out - (bcb218a) - Amadeus Mader
- (**deps**) upgrade flake - (1d2c04c) - Amadeus Mader
- (**deps**) upgrade flake - (8c173b9) - Amadeus Mader
- (**deps**) upgrade flake - (ee15c75) - Amadeus Mader
- (**deps**) upgrade flake - (63bf9d2) - Amadeus Mader
- (**deps**) upgrade flake - (e725892) - Amadeus Mader
- (**deps**) upgrade flake - (f316a73) - Amadeus Mader
- (**rust**) run the rust-ci gate as cached crane checks - (83c2f69) - Amadeus Mader
- (**rust**) give lanternd and countdown a with-db test postgres and run hofvarpnir's tests in rust-ci - (feabd06) - Amadeus Mader
- (**dashboard**) run ci against a postgres that lives only for the run - (742db7b) - Amadeus Mader
- (**deps**) upgrade flake - (822dafb) - Amadeus Mader
- (**deps**) upgrade flake - (4422822) - Amadeus Mader
- (**deps**) upgrade flake - (bbc9309) - Amadeus Mader
- (**deps**) upgrade flake - (6827a80) - Amadeus Mader
- (**deps**) upgrade flake - (d46ea63) - Amadeus Mader
- (**dashboard**) standalone flake and github export for internal-dashboard - (c74384e) - Amadeus Mader
- (**dashboard**) add newest, a-z and most-clicked sort toggle - (38f1c46) - Amadeus Mader
- (**rust**) share one toolchain, wire axon, dashboard and surrealkit - (0110c9f) - Amadeus Mader
- (**ui**) redesign homepage with card grid and collapsible add form - (4b972c5) - macbook-pro
- (**ci**) cache Rust deps through crane + Attic - (7b9689c) - developmentbot
- (**mcp**) make the /mcp Host allow-list configurable - (c789efd) - developmentbot
- (**api**) add health probes and docs routes - (d41c68a) - macbook-pro
- (**icon**) add favicon handler - (7d07e2a) - macbook-pro
- (**icon**) add favicon - (328195f) - macbook-pro
- (**css**) update layout - (a323f41) - macbook-pro
- (**tailwind**) add tailwind - (7c74162) - macbook-pro
#### Bug Fixes
- (**dashboard**) ignore the chrono unmaintained advisory in cargo deny - (4943c70) - Amadeus Mader
- (**dashboard**) pin workflow actions to commit shas and scope release permissions per job - (28a1f8b) - Amadeus Mader
- (**dashboard**) bump rmcp to 3.9.0 for RUSTSEC-2026-0457, 0461 and 0462 - (2e84617) - Amadeus Mader
- (**infra**) add sqlite to sqlx-cli and cargo-outdated build inputs - (0213df7) - Amadeus Mader
- (**rust**) bump ctutils, tokio and webpki for rustsec advisories and run rust-ci in parallel - (93f78d3) - Amadeus Mader
- (**dashboard**) satisfy pedantic clippy lints in tests and mcp handler - (07dcb8f) - Amadeus Mader
- (**dashboard**) bump rustls, rustls-webpki, webpki-roots and utoipa stack to clear RUSTSEC advisories and drop paste - (f4832b2) - Amadeus Mader
- (**dashboard**) move internal-dashboard postgres to 5434, off hofvarpnir's test db - (ed74187) - Amadeus Mader
- (**dashboard**) validate url scheme on link updates - (b301cb9) - Amadeus Mader
- (**infra**) decommission zeroclaw and drop the retired agenix keys - (54f2609) - Amadeus Mader
- (**tailwind**) only watch rust src - (31bba6a) - macbook-pro
- (**ci**) migrate db before - (f9b6ae5) - developmentbot
- (**ci**) reach the homelab nix cache and stop dropping the postgres tuning - (ad7bd7b) - macbook-pro
#### Documentation
- add AGENTS.md - (505d505) - macbook-pro
- (**ci**) add Woodpecker status badge - (b39187d) - developmentbot
#### Tests
- (**dashboard**) cover link sorting and click counting - (7d92b9b) - Amadeus Mader
#### Continuous Integration
- (**git**) fetch tags for cocogitto - (86c6d50) - developmentbot
- (**sqlx**) preserve offline mode in dev shell - (72cd0b1) - developmentbot
- (**cog**) ignore merge commits - (302f79c) - developmentbot
#### Style
- (**dashboard**) rustfmt sort toggle handler - (c8d83dc) - Amadeus Mader
#### Miscellaneous Chores
- (**dashboard**) update all crates to latest versions - (db0f24c) - Amadeus Mader
- (**deps**) drop the remaining linux-only guards and darwin systems - (4c24007) - Amadeus Mader
- (**deps**) remove the remaining darwin support from subflakes, templates, hofvarpnir releases and docs - (ccf758a) - Amadeus Mader
- (**dashboard**) add cargo-deny config - (e32ef82) - Amadeus Mader
- (**deps**) pin subflake locks to the root flake.lock revisions - (58dea17) - Amadeus Mader
- (**rust**) delete the dead woodpecker configs and axon sync-remotes - (1a2ac36) - Amadeus Mader
- (**rust**) source the root env in dashboard and engram - (77e828d) - Amadeus Mader
- (**dashboard**) fold internal-dashboard tooling into the root - (6ddb132) - Amadeus Mader
- (**dev**) compile offline against sqlx cache - (6a31fdb) - macbook-pro
- (**build**) use mold linker and tune release profile for compile speed - (a88c397) - developmentbot

- - -
## v0.2.1 - 2026-08-14
#### Miscellaneous Chores
- (**ignore**) update ignore - (53161ed) - macbook-pro
- (**release**) add just release, which tags annotated - (ad2bbf5) - macbook-pro

- - -

## v0.2.0 - 2026-08-14
#### Features
- (**mcp**) add update_link tool - (e2a6e4e) - developmentbot
- (**nix**) add a NixOS module and package - (6a93d74) - macbook-pro
- make the connection pool configurable - (fd878f2) - macbook-pro
- vendor htmx assets for offline use - (258195e) - developmentbot
- add link dashboard UI, REST API and MCP server - (dd28d08) - developmentbot
- add db schema, models and data-access contract - (be445e7) - developmentbot
#### Bug Fixes
- (**nix**) give pgbouncer a user list so peer auth can succeed - (fb8b7c6) - macbook-pro
- (**nix**) close three gaps in the NixOS module - (07e47c4) - macbook-pro
- (**nix**) pin nixpkgs 26.05 for x86_64-darwin - (4d3afaf) - macbook-pro
#### Performance Improvements
- (**db**) index the ilike search with pg_trgm - (910aa9f) - macbook-pro
#### Documentation
- record three open issues in todos/ - (7ff1b0e) - developmentbot
#### Tests
- add integration suite across db, api, ui, sse and mcp - (6b74b9d) - developmentbot
#### Refactoring
- (**api**) drop swagger-ui in favour of scalar - (38ad4b5) - macbook-pro
#### Miscellaneous Chores
- (**deps**) add git to devShell - (e27a883) - macbook-pro
- (**release**) let cog bump own the version, tag and push - (bc52b14) - macbook-pro
- add just sync-remotes and enable unstable - (1bb6b49) - macbook-pro
- add just test-one for running a single test target - (f43bd54) - developmentbot
- import rust project scaffold - (500aa30) - developmentbot

- - -

Changelog generated by [cocogitto](https://github.com/cocogitto/cocogitto).
