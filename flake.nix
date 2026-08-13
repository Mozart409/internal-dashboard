{
  description = "Rust development environment";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "x86_64-darwin"
        "aarch64-darwin"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      devShells = forAllSystems (
        system:
        let
          pkgs = import nixpkgs { inherit system; };
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
