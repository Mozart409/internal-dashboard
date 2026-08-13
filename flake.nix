{
  description = "Rust development environment";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    # nixpkgs 26.11 dropped x86_64-darwin, which is what the Intel MacBook runs.
    # 26.05 is the last branch carrying it and is supported until the end of
    # 2026; only that one system is resolved against it, everything else stays
    # on unstable.
    nixpkgs-darwin.url = "github:NixOS/nixpkgs/nixpkgs-26.05-darwin";
  };

  outputs =
    { nixpkgs, nixpkgs-darwin, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;

      nixpkgsFor =
        system:
        import (if system == "x86_64-darwin" then nixpkgs-darwin else nixpkgs) { inherit system; };
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgsFor system;
        in
        {
          default = pkgs.mkShell {
            packages = with pkgs; [
              # Rust toolchain
              # keep-sorted start
              cargo
              clippy
              rust-analyzer
              rustc
              rustfmt
              # keep-sorted end

              # Project tooling
              # keep-sorted start
              cargo-watch
              cocogitto
              just
              keep-sorted
              lefthook
              podman-compose
              postgresql_18
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
