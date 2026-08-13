{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "internal-dashboard";
  version = "0.1.0";

  # Only the inputs the compiler actually reads, so editing the README or the
  # compose file does not invalidate the build.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      # sqlx::migrate! embeds these at compile time, and include_str! embeds
      # the vendored htmx — neither is read from disk at runtime.
      ../migrations
      ../static
      ../.sqlx
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # The query! macros check against the committed .sqlx/ cache instead of a
  # live server, which is the only way they can run in the sandbox.
  env.SQLX_OFFLINE = "true";

  # Every integration test wants a Postgres it can create databases on.
  doCheck = false;

  meta = {
    description = "Internal dashboard for curating links, with a REST API and an MCP server";
    homepage = "https://github.com/Mozart409/internal-dashboard";
    # No license is declared upstream. Left unset deliberately: marking it
    # unfree would force allowUnfree on everyone importing this module.
    mainProgram = "internal-dashboard";
    platforms = lib.platforms.unix;
  };
}
