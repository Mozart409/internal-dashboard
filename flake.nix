{
  description = "Internal link dashboard: package, NixOS module and dev shell";

  # In the yggdrasil monorepo this is a subflake of the root flake, which makes
  # every input below follow its own, so the toolchain is the monorepo's one
  # stable Rust (rust/toolchain.nix `build` there). The monorepo itself builds
  # the project from ./nix/package.nix with its shared toolchain; this file
  # exists so the exported GitHub repo builds without the monorepo around it.
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
    fenix = {
      url = "github:nix-community/fenix";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = inputs @ {
    self,
    nixpkgs,
    flake-parts,
    ...
  }: let
    # Latest stable, pinned by flake.lock rather than a version literal.
    # Keep the component list equal to `build` in yggdrasil's
    # rust/toolchain.nix (the root's `rust-toolchain-sync` asserts it).
    toolchainFor = system:
      inputs.fenix.packages.${system}.stable.withComponents [
        "cargo"
        "clippy"
        "rust-src"
        "rustc"
        "rustfmt"
      ];

    packageFor = pkgs: let
      toolchain = toolchainFor pkgs.stdenv.hostPlatform.system;
    in
      pkgs.callPackage ./nix/package.nix {
        rustPlatform = pkgs.makeRustPlatform {
          cargo = toolchain;
          rustc = toolchain;
        };
      };
  in
    flake-parts.lib.mkFlake {inherit inputs;} {
      systems = ["x86_64-linux" "aarch64-linux" "aarch64-darwin"];

      perSystem = {
        pkgs,
        system,
        ...
      }: let
        toolchain = toolchainFor system;
        package = packageFor pkgs;
        # The tests take `pkgs` with the overlay already applied.
        testPkgs = pkgs.extend self.overlays.default;
      in {
        packages = {
          default = package;
          internal-dashboard = package;
          inherit toolchain;
        };

        checks =
          {
            inherit package;
            # Evaluates the module into a whole NixOS system and asserts on the
            # result. Pure evaluation, so it runs on any host.
            module-eval = import ./nix/tests/eval.nix {
              inherit self nixpkgs;
              pkgs = testPkgs;
            };
          }
          # The VM test needs a Linux builder.
          // pkgs.lib.optionalAttrs pkgs.stdenv.hostPlatform.isLinux {
            module-vm = import ./nix/tests/vm.nix {
              inherit self;
              pkgs = testPkgs;
            };
          };

        devShells.default = pkgs.mkShell {
          packages = with pkgs;
            [
              # keep-sorted start
              cargo-edit
              cargo-watch
              inputs.fenix.packages.${system}.stable.rust-analyzer
              podman-compose
              postgresql_18
              sqlx-cli
              tailwindcss_4
              toolchain
              # keep-sorted end
            ]
            # .cargo/config.toml only points at mold on linux-gnu targets.
            ++ lib.optionals stdenv.isLinux [mold];
        };
      };

      flake = {
        # For consumers who would rather build against their own nixpkgs; the
        # toolchain is still this flake's pinned one.
        overlays.default = final: _prev: {
          internal-dashboard = packageFor final;
        };

        nixosModules = {
          # The module on its own. It expects `pkgs.internal-dashboard` to
          # exist, so pair it with overlays.default.
          internal-dashboard = ./nix/module.nix;

          # The batteries-included import: the module plus the overlay that
          # gives its `package` option something to point at.
          default = {
            imports = [./nix/module.nix];
            nixpkgs.overlays = [self.overlays.default];
          };
        };
      };
    };
}
