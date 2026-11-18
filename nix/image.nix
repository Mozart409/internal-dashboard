# The container image for rust/internal-dashboard.
#
# `dockerTools.buildImage` with `fromImage = null`: a scratch image, no shell, no
# package manager, no distro. That is also why Kubernetes must use the native
# preStop sleep rather than an exec hook, since there is no shell to run it in.
# The binary is built for both x86_64 and aarch64 like the package, and all
# static assets are compiled in, so the image needs nothing but CA certs.
#
# Runs as nobody. Postgres is external: pass DATABASE_URL. The image ships no
# database and no config. The Helm chart lives in deploy/helm/internal-dashboard.
{
  pkgs,
  internal-dashboard,
}: let
  # buildEnv gives the image a /bin to put the binary in; dockerTools copies
  # this env's runtime closure into the layer.
  contents = pkgs.buildEnv {
    name = "internal-dashboard-contents";
    paths = [internal-dashboard pkgs.cacert];
    pathsToLink = ["/bin"];
  };
in
  pkgs.dockerTools.buildImage {
    name = "internal-dashboard";
    tag = "latest";

    # FROM scratch.
    fromImage = null;
    copyToRoot = contents;

    config = {
      Entrypoint = ["/bin/internal-dashboard"];

      # nobody:nogroup. The scratch image has no /etc/passwd, so use the raw ids.
      User = "65534:65534";

      Env = [
        # The scratch image has no /etc/ssl, so TLS clients need the bundle named here.
        "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
        "RUST_LOG=info"
        # The binary defaults to loopback; a container must listen on all interfaces.
        "BIND_ADDR=0.0.0.0:3000"
      ];

      ExposedPorts = {
        "3000/tcp" = {};
      };

      Labels = {
        "org.opencontainers.image.title" = "internal-dashboard";
        "org.opencontainers.image.description" = "Internal link dashboard: HTMX UI, REST API and MCP server";
        "org.opencontainers.image.licenses" = "MIT";
      };
    };
  }
