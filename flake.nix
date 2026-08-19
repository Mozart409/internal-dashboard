{
  description = "Internal link dashboard: package, NixOS module and dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # nixpkgs 26.11 dropped x86_64-darwin, which is what the Intel MacBook runs.
    # 26.05 is the last branch carrying it and is supported until the end of
    # 2026; only that one system is resolved against it, everything else stays
    # on unstable.
    nixpkgs-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
    crane.url = "github:ipetkov/crane";
  };

  outputs = {
    self,
    nixpkgs,
    nixpkgs-darwin,
    crane,
    ...
  }: let
    systems = [
      "x86_64-linux"
      "aarch64-linux"
      "x86_64-darwin"
      "aarch64-darwin"
    ];
    forAllSystems = nixpkgs.lib.genAttrs systems;

    nixpkgsFor = system:
      import (
        if system == "x86_64-darwin"
        then nixpkgs-darwin
        else nixpkgs
      ) {
        inherit system;
        overlays = [self.overlays.default];
      };

    # Crane wiring, computed once per system and shared by checks/legacyPackages
    # below. No toolchain override: crane picks up the same nixpkgs
    # cargo/rustc the devShell already uses, so there is only one Rust
    # toolchain in this flake, not two.
    craneFor = system: let
      pkgs = nixpkgsFor system;
      craneLib = crane.mkLib pkgs;
      commonArgs = {
        src = craneLib.cleanCargoSource ./.;
        strictDeps = true;
        # The query! macros check against the committed .sqlx/ cache instead
        # of a live Postgres, the only way they can run in the sandbox -- same
        # setting nix/package.nix already uses for the full build.
        env.SQLX_OFFLINE = "true";
      };
      # Dependency-only build: compiles `[dependencies]`, never touches this
      # crate's own code, so it's a pure, sandboxable, Attic-cacheable
      # derivation even though `cargo test` below needs a live Postgres Nix's
      # sandbox can't reach.
      cargoArtifacts = craneLib.buildDepsOnly commonArgs;
    in {
      inherit craneLib commonArgs cargoArtifacts;
    };
  in {
    # Consumers who would rather build against their own nixpkgs than have a
    # second one pinned for them can add this and skip nixosModules.default.
    overlays.default = final: _prev: {
      internal-dashboard = final.callPackage ./nix/package.nix {};
    };

    packages = forAllSystems (
      system: let
        pkgs = nixpkgsFor system;
      in {
        inherit (pkgs) internal-dashboard;
        default = pkgs.internal-dashboard;
      }
    );

    nixosModules = {
      # The module on its own. It expects `pkgs.internal-dashboard` to exist,
      # so pair it with overlays.default.
      internal-dashboard = ./nix/module.nix;

      # The batteries-included import: the module plus the overlay that gives
      # its `package` option something to point at.
      default = {
        imports = [./nix/module.nix];
        nixpkgs.overlays = [self.overlays.default];
      };
    };

    checks = forAllSystems (
      system: let
        pkgs = nixpkgsFor system;
        inherit (craneFor system) craneLib commonArgs cargoArtifacts;
      in
        {
          package = self.packages.${system}.default;

          # Evaluates the module into a whole NixOS system and asserts on the
          # result. Pure evaluation, so it runs on any host regardless of what
          # it can build.
          module-eval = import ./nix/tests/eval.nix {inherit self nixpkgs pkgs;};

          # `fmt`/`clippy` only -- `cargo test` needs the live Postgres from
          # .woodpecker.yaml's `services:`, which the Nix build sandbox has no
          # network path to. It stays a plain `nix develop` command in CI,
          # seeded from `legacyPackages.<system>.cargo-artifacts` below so it
          # isn't recompiling the dependency graph fmt/clippy already cached.
          fmt = craneLib.cargoFmt commonArgs;
          clippy = craneLib.cargoClippy (commonArgs
            // {
              inherit cargoArtifacts;
              cargoClippyExtraArgs = "--all-targets -- -D warnings";
            });
        }
        # The VM test needs a Linux builder, so it is only offered where one is
        # a given.
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "linux" system) {
          module-vm = import ./nix/tests/vm.nix {inherit self pkgs;};
        }
    );

    legacyPackages = forAllSystems (
      system: {
        cargo-artifacts = (craneFor system).cargoArtifacts;
      }
    );

    devShells = forAllSystems (
      system: let
        pkgs = nixpkgsFor system;
      in {
        default = pkgs.mkShell {
          packages = with pkgs;
            [
              # keep-sorted start
              cargo
              # cargo set-version, which cog's pre_bump_hooks call to move the
              # version in Cargo.toml.
              cargo-edit
              cargo-watch
              clippy
              cocogitto
              git
              just
              keep-sorted
              lefthook
              nixfmt
              podman-compose
              postgresql_18
              rust-analyzer
              rustc
              rustfmt
              sqlx-cli
              tailwindcss_4
              # keep-sorted end
            ]
            # mold only ships for Linux in nixpkgs (aarch64-darwin support is
            # newer/less battle-tested, and there's no x86_64-darwin build at
            # all) — .cargo/config.toml only points at it on linux-gnu targets
            # to match, so it's simply unused, not broken, on Darwin.
            ++ lib.optionals stdenv.isLinux [mold];

          shellHook = ''
            lefthook install
            if [ -d target ]; then
              du -sh target
            fi
          '';
        };
      }
    );
  };
}
