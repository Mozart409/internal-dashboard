# Full VM test: boot NixOS with the module, then use the service for real.
#
# This needs a Linux builder, so `nix flake check` only offers it on Linux. It
# has not been run on the macOS host the module was written on — see the note
# in the README.
{ self, pkgs }:

pkgs.testers.runNixOSTest {
  name = "internal-dashboard";

  # The nodes import the plain module rather than nixosModules.default:
  # runNixOSTest pins each node's `nixpkgs.pkgs`, which a module setting
  # `nixpkgs.overlays` cannot then add to. They still get the package, because
  # the `pkgs` this test is called on already carries the overlay — the same
  # pairing a consumer uses when managing the overlay themselves.
  nodes = {
    # The default shape: dashboard talking straight to a local PostgreSQL over
    # the peer-authenticated socket.
    direct =
      { pkgs, ... }:
      {
        imports = [ self.nixosModules.internal-dashboard ];
        services.internal-dashboard.enable = true;
        environment.systemPackages = [ pkgs.curl ];
      };

    # The same, with pgbouncer in the path and a non-default port, so both the
    # pooled connection string and the port plumbing get exercised.
    pooled =
      { pkgs, ... }:
      {
        imports = [ self.nixosModules.internal-dashboard ];
        services.internal-dashboard = {
          enable = true;
          port = 8080;
          database.pgbouncer.enable = true;
        };
        environment.systemPackages = [ pkgs.curl ];
      };
  };

  testScript = ''
    start_all()

    def check_dashboard(machine, port):
        machine.wait_for_unit("postgresql.service")
        machine.wait_for_unit("internal-dashboard.service")
        machine.wait_for_open_port(port)

        base = f"http://127.0.0.1:{port}"

        # The UI renders and the generated spec is served.
        machine.succeed(f"curl -sSf {base}/ >/dev/null")
        machine.succeed(f"curl -sSf {base}/api-docs/openapi.json | grep -q '\"openapi\"'")
        machine.succeed(f"curl -sSf {base}/scalar >/dev/null")

        # A link written over the API comes back out of it, which only works if
        # the embedded migrations ran against the provisioned database.
        machine.succeed(
            f"curl -sSf -X POST {base}/api/v1/links "
            "-H 'content-type: application/json' "
            """-d '{"url":"https://example.com","title":"an example link"}' >/dev/null"""
        )
        machine.succeed(f"curl -sSf {base}/api/v1/links | grep -q 'an example link'")

        # The trigram migration created its extension, so search is indexed.
        machine.succeed(
            "sudo -u postgres psql -d internal-dashboard -tAc "
            "\"select 1 from pg_extension where extname = 'pg_trgm'\" | grep -q 1"
        )
        machine.succeed(f"curl -sSf '{base}/api/v1/links?q=example' | grep -q 'an example link'")

        # It really is running as its own unprivileged user.
        machine.succeed(
            "systemctl show -p User --value internal-dashboard.service | grep -qx internal-dashboard"
        )

    with subtest("dashboard against a local postgres"):
        check_dashboard(direct, 3000)

    with subtest("dashboard through pgbouncer"):
        pooled.wait_for_unit("pgbouncer.service")
        pooled.succeed("test -S /run/pgbouncer/.s.PGSQL.6432")
        check_dashboard(pooled, 8080)

    with subtest("the service survives a restart of the database"):
        direct.succeed("systemctl restart postgresql.service")
        direct.succeed("systemctl restart internal-dashboard.service")
        direct.wait_for_unit("internal-dashboard.service")
        direct.succeed("curl -sSf http://127.0.0.1:3000/api/v1/links | grep -q 'an example link'")
  '';
}
