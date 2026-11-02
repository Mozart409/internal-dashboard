# What `just dashboard-ci` compiles (fmt, clippy, the tests, `sqlx prepare
# --check`) as crane checks, called from the yggdrasil root flake; the
# keep-sorted lint stays a plain recipe (it compiles nothing):
#
#   import ./rust/internal-dashboard/nix/checks.nix {inherit pkgs crane toolchain testPostgres;}
#
# As Nix builds they are cached by their inputs: a push that does not change
# the source finds them already in the store, and the rust-ci pre-push gate
# (the root's legacyPackages.rust-ci.internal-dashboard) costs an evaluation
# instead of a compile. The standalone flake does not use this file.
#
# Returns `checks` and `cargoArtifacts`.
{
  pkgs,
  crane,
  toolchain,
  # The root's rust/test-postgres.nix: a Postgres that lives for one build.
  testPostgres,
}: let
  inherit (pkgs) lib;

  craneLib = (crane.mkLib pkgs).overrideToolchain toolchain;

  # The same source as nix/package.nix, plus the integration tests.
  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
      ../tests
      ../migrations
      ../static
      ../.sqlx
    ];
  };

  commonArgs = {
    inherit src;
    pname = "internal-dashboard";
    version = (lib.importTOML ../Cargo.toml).package.version;
    strictDeps = true;
    # The query! macros check against the committed .sqlx/ cache; the
    # prepare check below proves that cache matches the migrated schema.
    SQLX_OFFLINE = "true";
    # The dev profile `cargo test` uses, so the tests keep debug assertions and
    # overflow checks (crane defaults to release).
    CARGO_PROFILE = "dev";
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in {
  inherit cargoArtifacts;

  checks = {
    fmt = craneLib.cargoFmt {
      inherit src;
      pname = "internal-dashboard-fmt";
    };

    clippy = craneLib.cargoClippy (commonArgs
      // {
        inherit cargoArtifacts;
        pname = "internal-dashboard-clippy";
        # Kept equal to `just dashboard-clippy`.
        cargoClippyExtraArgs = "--all-targets -- -D warnings";
      });

    # #[sqlx::test] gives every test its own migrated database on this server.
    test = craneLib.cargoTest (testPostgres.withPostgres "DATABASE_URL" (commonArgs
      // {
        inherit cargoArtifacts;
        pname = "internal-dashboard-test";
        cargoTestExtraArgs = "--all-targets";
      }));

    # `just dashboard-prepare-check`: the committed .sqlx/ cache still matches
    # the queries, checked against the migrated schema.
    prepare = craneLib.mkCargoDerivation (testPostgres.withPostgres "DATABASE_URL" (commonArgs
      // {
        inherit cargoArtifacts;
        pname = "internal-dashboard-prepare";
        SQLX_OFFLINE = "false";
        nativeCheckInputs = [pkgs.sqlx-cli];
        doCheck = true;
        doInstallCargoArtifacts = false;
        buildPhaseCargoCommand = "";
        checkPhaseCargoCommand = ''
          sqlx migrate run
          cargo sqlx prepare --check
        '';
        installPhaseCommand = "mkdir -p $out";
      }));
  };
}
