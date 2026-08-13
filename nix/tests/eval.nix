# Evaluation-only checks for the NixOS module.
#
# These never build the service, they only evaluate it into a full NixOS
# configuration and assert on the result — which means they run anywhere,
# including on a macOS host that has no Linux builder to run a VM test on.
{
  self,
  nixpkgs,
  pkgs,
}:

let
  lib = nixpkgs.lib;

  # The module system does the merging: a shallow `//` here would drop
  # `enable = true` the moment a case sets anything else under `services`.
  evalWith =
    modules:
    (lib.nixosSystem {
      modules = [
        self.nixosModules.default
        {
          nixpkgs.hostPlatform = "x86_64-linux";
          system.stateVersion = "26.11";
        }
      ]
      ++ modules;
    }).config;

  enabled =
    module:
    evalWith [
      { services.internal-dashboard.enable = true; }
      module
    ];

  # Only this module's assertions. A bare configuration always trips the base
  # system's own ones — no root filesystem, no bootloader — and those say
  # nothing about whether the dashboard is configured sanely.
  failures =
    config:
    lib.filter (message: lib.hasInfix "services.internal-dashboard" message) (
      map (a: a.message) (lib.filter (a: !a.assertion) config.assertions)
    );
  saysNothingIsWrong = config: failures config == [ ];
  complainsAbout =
    config: fragment: lib.any (message: lib.hasInfix fragment message) (failures config);

  # --- the configurations under test -----------------------------------------

  default = enabled { };
  defaultUnit = default.systemd.services.internal-dashboard;

  external = enabled {
    services.internal-dashboard.database = {
      createLocally = false;
      url = "postgres://someone@db.internal/dashboard";
    };
  };

  externalWithNothing = enabled {
    services.internal-dashboard.database.createLocally = false;
  };

  mismatchedNames = enabled {
    services.internal-dashboard.user = "dashboard";
  };

  overridden = enabled {
    services.internal-dashboard = {
      address = "0.0.0.0";
      port = 80;
      openFirewall = true;
      logLevel = "debug";
      pool = {
        maxConnections = 25;
        acquireTimeout = 9;
      };
      environment.DATABASE_URL = "postgres://elsewhere/db";
    };
  };
  overriddenUnit = overridden.systemd.services.internal-dashboard;

  bouncing = enabled {
    services.internal-dashboard.database.pgbouncer.enable = true;
  };
  bouncingUnit = bouncing.systemd.services.internal-dashboard;

  bouncingUnprepared = enabled {
    services.internal-dashboard.database.pgbouncer = {
      enable = true;
      maxPreparedStatements = 0;
    };
  };

  bouncingRemote = enabled {
    services.internal-dashboard.database = {
      createLocally = false;
      url = "postgres://someone@db.internal/dashboard";
      pgbouncer.enable = true;
    };
  };

  tunedBig = enabled {
    services.internal-dashboard.database.tuning.memoryMB = 8192;
  };

  untuned = enabled {
    services.internal-dashboard.database = {
      tuning.enable = false;
      settings.shared_buffers = "9GB";
    };
  };

  overriddenSetting = enabled {
    services.internal-dashboard.database.settings.shared_buffers = "9GB";
  };

  noExtensions = enabled {
    services.internal-dashboard.database.extensions = [ ];
  };

  # --- the checks ------------------------------------------------------------

  checks = [
    {
      name = "the default configuration raises no assertion";
      ok = saysNothingIsWrong default;
    }
    {
      name = "ExecStart runs the binary out of the package";
      ok = lib.hasSuffix "/bin/internal-dashboard" defaultUnit.serviceConfig.ExecStart;
    }
    {
      name = "the service runs as its own user and group";
      ok =
        defaultUnit.serviceConfig.User == "internal-dashboard"
        && defaultUnit.serviceConfig.Group == "internal-dashboard";
    }
    {
      name = "DATABASE_URL is the peer-authenticated postgres socket";
      ok =
        defaultUnit.environment.DATABASE_URL
        == "postgres:///internal-dashboard?host=/run/postgresql&port=5432&user=internal-dashboard";
    }
    {
      name = "BIND_ADDR, RUST_LOG and the pool reach the unit";
      ok =
        defaultUnit.environment.BIND_ADDR == "127.0.0.1:3000"
        && defaultUnit.environment.RUST_LOG == "info"
        && defaultUnit.environment.DB_MAX_CONNECTIONS == "10"
        && defaultUnit.environment.DB_ACQUIRE_TIMEOUT_SECS == "5";
    }
    {
      name = "the unit is ordered after postgresql";
      ok =
        lib.elem "postgresql.service" defaultUnit.after
        && lib.elem "postgresql.service" defaultUnit.requires;
    }
    {
      # ensureDatabases and ensureUsers run from postgresql-setup.service, so
      # waiting only on postgresql.service races the database into existence.
      name = "the unit waits for the database and role to be created";
      ok =
        lib.elem "postgresql-setup.service" defaultUnit.after
        && lib.elem "postgresql-setup.service" defaultUnit.requires
        && lib.elem "postgresql-setup.service" default.systemd.services.internal-dashboard-db-setup.after;
    }
    {
      name = "createLocally provisions the database and its owner";
      ok =
        default.services.postgresql.enable
        && default.services.postgresql.ensureDatabases == [ "internal-dashboard" ]
        && lib.any (
          u: u.name == "internal-dashboard" && u.ensureDBOwnership
        ) default.services.postgresql.ensureUsers;
    }
    {
      name = "the hardening settings are applied";
      ok =
        defaultUnit.serviceConfig.ProtectSystem == "strict"
        && defaultUnit.serviceConfig.NoNewPrivileges
        && defaultUnit.serviceConfig.PrivateTmp
        && lib.elem "@system-service" defaultUnit.serviceConfig.SystemCallFilter
        &&
          defaultUnit.serviceConfig.RestrictAddressFamilies == [
            "AF_INET"
            "AF_INET6"
            "AF_NETLINK"
            "AF_UNIX"
          ];
    }
    {
      # Without netlink, glibc cannot resolve a hostname, so an external
      # database.url pointing at one would never connect.
      name = "name resolution is possible for an external database";
      ok = lib.elem "AF_NETLINK" (
        external.systemd.services.internal-dashboard.serviceConfig.RestrictAddressFamilies
      );
    }
    {
      name = "all capabilities are dropped for an unprivileged port";
      ok =
        defaultUnit.serviceConfig.AmbientCapabilities == [ ]
        && defaultUnit.serviceConfig.CapabilityBoundingSet == [ "" ];
    }
    {
      name = "a privileged port keeps exactly CAP_NET_BIND_SERVICE";
      ok = overriddenUnit.serviceConfig.AmbientCapabilities == [ "CAP_NET_BIND_SERVICE" ];
    }
    {
      name = "address, port, log level and pool overrides land in the unit";
      ok =
        overriddenUnit.environment.BIND_ADDR == "0.0.0.0:80"
        && overriddenUnit.environment.RUST_LOG == "debug"
        && overriddenUnit.environment.DB_MAX_CONNECTIONS == "25"
        && overriddenUnit.environment.DB_ACQUIRE_TIMEOUT_SECS == "9";
    }
    {
      name = "openFirewall opens the bound port";
      ok = lib.elem 80 overridden.networking.firewall.allowedTCPPorts;
    }
    {
      name = "the firewall stays shut by default";
      ok = !(lib.elem 3000 default.networking.firewall.allowedTCPPorts);
    }
    {
      name = "environment overrides beat the module's own DATABASE_URL";
      ok = overriddenUnit.environment.DATABASE_URL == "postgres://elsewhere/db";
    }
    {
      name = "an external database url is used verbatim and starts no server";
      ok =
        external.systemd.services.internal-dashboard.environment.DATABASE_URL
        == "postgres://someone@db.internal/dashboard"
        && !external.services.postgresql.enable
        && saysNothingIsWrong external;
    }
    {
      name = "an external database with no url at all is rejected";
      ok = complainsAbout externalWithNothing "must set database.url";
    }
    {
      name = "a role that does not match the database name is rejected";
      ok = complainsAbout mismatchedNames "must equal user";
    }
    {
      name = "the tuned defaults are derived from the memory budget";
      ok =
        default.services.postgresql.settings.shared_buffers == "256MB"
        && default.services.postgresql.settings.effective_cache_size == "768MB"
        && default.services.postgresql.settings.maintenance_work_mem == "64MB"
        && default.services.postgresql.settings.work_mem == "4MB";
    }
    {
      name = "a bigger memory budget scales every derived value";
      ok =
        tunedBig.services.postgresql.settings.shared_buffers == "2048MB"
        && tunedBig.services.postgresql.settings.effective_cache_size == "6144MB"
        && tunedBig.services.postgresql.settings.maintenance_work_mem == "512MB"
        && tunedBig.services.postgresql.settings.work_mem == "30MB";
    }
    {
      name = "the safety timeouts are set";
      ok =
        default.services.postgresql.settings.statement_timeout == "30s"
        && default.services.postgresql.settings.lock_timeout == "10s"
        && default.services.postgresql.settings.idle_in_transaction_session_timeout == "60s";
    }
    {
      name = "an ssd budget makes the planner willing to use the indexes";
      ok = default.services.postgresql.settings.random_page_cost == "1.1";
    }
    {
      name = "database.settings overrides a tuned value";
      ok = overriddenSetting.services.postgresql.settings.shared_buffers == "9GB";
    }
    {
      name = "tuning can be switched off entirely";
      ok =
        untuned.services.postgresql.settings.shared_buffers == "9GB"
        && !(untuned.services.postgresql.settings ? statement_timeout);
    }
    {
      name = "the extension setup unit runs before the dashboard";
      ok =
        let
          setup = default.systemd.services.internal-dashboard-db-setup;
        in
        lib.hasInfix "pg_trgm" setup.script
        && lib.elem "internal-dashboard.service" setup.before
        && setup.serviceConfig.Type == "oneshot";
    }
    {
      name = "an empty extension list creates no setup unit";
      ok = !(noExtensions.systemd.services ? internal-dashboard-db-setup);
    }
    {
      name = "pgbouncer takes over the connection string";
      ok =
        bouncingUnit.environment.DATABASE_URL
        == "postgres:///internal-dashboard?host=/run/pgbouncer&port=6432&user=internal-dashboard";
    }
    {
      name = "pgbouncer runs as the dashboard's user so peer auth works";
      ok =
        bouncing.services.pgbouncer.enable
        && bouncing.services.pgbouncer.user == "internal-dashboard"
        && bouncing.services.pgbouncer.settings.pgbouncer.auth_type == "hba";
    }
    {
      # pgbouncer resolves the login name before applying the HBA method, and
      # rejects a name it cannot resolve — so peer auth needs both files, not
      # just the HBA one.
      name = "pgbouncer is given a user list as well as an hba file";
      ok =
        bouncing.services.pgbouncer.settings.pgbouncer ? auth_file
        && bouncing.services.pgbouncer.settings.pgbouncer.auth_file != null
        && bouncing.services.pgbouncer.settings.pgbouncer ? auth_hba_file;
    }
    {
      name = "pgbouncer listens on a socket only, never on TCP";
      ok =
        bouncing.services.pgbouncer.settings.pgbouncer.listen_addr == null
        && bouncing.services.pgbouncer.settings.pgbouncer.unix_socket_dir == "/run/pgbouncer";
    }
    {
      name = "transaction pooling keeps prepared statements working";
      ok =
        bouncing.services.pgbouncer.settings.pgbouncer.pool_mode == "transaction"
        && bouncing.services.pgbouncer.settings.pgbouncer.max_prepared_statements == 200;
    }
    {
      name = "the dashboard waits for pgbouncer when it is in the path";
      ok = lib.elem "pgbouncer.service" bouncingUnit.requires;
    }
    {
      name = "transaction pooling without prepared statements is rejected";
      ok = complainsAbout bouncingUnprepared "breaks sqlx";
    }
    {
      name = "pgbouncer against a database we do not manage is rejected";
      ok = complainsAbout bouncingRemote "peer-authenticated socket";
    }
  ];

  failed = lib.filter (c: !c.ok) checks;

  report = lib.concatMapStringsSep "\n" (c: "${if c.ok then "ok  " else "FAIL"}  ${c.name}") checks;
in

if failed != [ ] then
  throw ''
    ${toString (lib.length failed)} of ${toString (lib.length checks)} module eval checks failed:

    ${report}
  ''
else
  pkgs.runCommand "internal-dashboard-module-eval"
    {
      # The report is plain prose, but discarding context keeps any store path
      # that ever leaks into a check name from dragging a Linux build into a
      # check that is meant to be evaluation-only.
      report = builtins.unsafeDiscardStringContext ''
        ${toString (lib.length checks)} module eval checks passed:

        ${report}
      '';
      passAsFile = [ "report" ];
    }
    ''
      cp "$reportPath" "$out"
    ''
