{
  description = "Internal link dashboard: package, NixOS module and dev shell";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # nixpkgs 26.11 dropped x86_64-darwin, which is what the Intel MacBook runs.
    # 26.05 is the last branch carrying it and is supported until the end of
    # 2026; only that one system is resolved against it, everything else stays
    # on unstable.
    nixpkgs-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs = {
    self,
    nixpkgs,
    nixpkgs-darwin,
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
      in
        {
          package = self.packages.${system}.default;

          # Evaluates the module into a whole NixOS system and asserts on the
          # result. Pure evaluation, so it runs on any host regardless of what
          # it can build.
          module-eval = import ./nix/tests/eval.nix {inherit self nixpkgs pkgs;};
        }
        # The VM test needs a Linux builder, so it is only offered where one is
        # a given.
        // nixpkgs.lib.optionalAttrs (nixpkgs.lib.hasSuffix "linux" system) {
          module-vm = import ./nix/tests/vm.nix {inherit self pkgs;};
        }
    );

    devShells = forAllSystems (
      system: let
        pkgs = nixpkgsFor system;
      in {
        default = pkgs.mkShell {
          packages = with pkgs; [
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
            # keep-sorted end
          ];

          shellHook = ''
            lefthook install
          '';
        };
      }
    );
  };
}
