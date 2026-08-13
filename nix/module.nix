{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.internal-dashboard;
  db = cfg.database;
  tuning = db.tuning;
  bouncer = db.pgbouncer;

  inherit (lib)
    concatMapStringsSep
    mkDefault
    mkEnableOption
    mkIf
    mkOption
    mkPackageOption
    optionalAttrs
    optionals
    types
    ;

  bindAddr = "${cfg.address}:${toString cfg.port}";

  # sqlx resolves `?host=/dir` plus `?port=N` to the socket /dir/.s.PGSQL.N, and
  # `?user=` is passed explicitly rather than left to sqlx's fallback of the
  # calling process's OS user — the two happen to agree here, but only because
  # peer authentication forces the role and the system user to share a name.
  socketUrl = dir: port: "postgres:///${db.name}?host=${dir}&port=${toString port}&user=${cfg.user}";

  localUrl =
    if bouncer.enable then
      socketUrl "/run/pgbouncer" bouncer.port
    else
      socketUrl "/run/postgresql" 5432;

  # Peer authentication on the socket: pgbouncer is told to trust the OS user
  # behind the connection, which is the dashboard and nothing else, so no
  # password or auth file has to exist anywhere.
  pgbouncerHba = pkgs.writeText "internal-dashboard-pgbouncer-hba" ''
    # A local line carries no address column.
    # type   database  user  method
    local    all       all   peer
  '';

  # A deliberately small derivation of Postgres settings from one declared
  # memory budget, so the numbers stay in proportion to each other instead of
  # being sprinkled around as magic constants.
  mib = n: "${toString n}MB";
  sharedBuffers = tuning.memoryMB / 4;
  effectiveCache = tuning.memoryMB * 3 / 4;
  maintenanceWorkMem = lib.min (tuning.memoryMB / 16) 512;
  workMem = lib.max 4 (effectiveCache / (tuning.maxConnections * 2));

  tunedSettings = {
    max_connections = tuning.maxConnections;

    shared_buffers = mib sharedBuffers;
    effective_cache_size = mib effectiveCache;
    maintenance_work_mem = mib maintenanceWorkMem;
    work_mem = mib workMem;

    # Spread checkpoints out rather than stalling on them.
    max_wal_size = "2GB";
    min_wal_size = "200MB";
    checkpoint_completion_target = "0.9";
    wal_compression = "on";

    # Random reads are nearly as cheap as sequential ones on an SSD, which is
    # what decides whether the planner will touch the trigram indexes at all.
    random_page_cost = if tuning.diskType == "ssd" then "1.1" else "4.0";
    effective_io_concurrency = if tuning.diskType == "ssd" then 200 else 2;

    # Nothing this dashboard does should run for 30 seconds, sit idle holding a
    # transaction open, or wait minutes on a lock.
    statement_timeout = tuning.statementTimeout;
    lock_timeout = tuning.lockTimeout;
    idle_in_transaction_session_timeout = tuning.idleTransactionTimeout;

    # Notice a peer that disappeared instead of pinning the connection forever.
    tcp_keepalives_idle = 60;
    tcp_keepalives_interval = 10;
    tcp_keepalives_count = 6;
  };
in
{
  options.services.internal-dashboard = {
    enable = mkEnableOption "the internal link dashboard";

    package = mkPackageOption pkgs "internal-dashboard" { };

    address = mkOption {
      type = types.str;
      default = "127.0.0.1";
      example = "0.0.0.0";
      description = ''
        Address to bind. The default keeps the dashboard on loopback; it has no
        authentication of its own, so anything else should sit behind a proxy
        that provides some.
      '';
    };

    port = mkOption {
      type = types.port;
      default = 3000;
      description = "Port to bind.";
    };

    openFirewall = mkOption {
      type = types.bool;
      default = false;
      description = ''
        Open {option}`services.internal-dashboard.port` in the firewall. Read
        the warning on {option}`services.internal-dashboard.address` first — the
        dashboard is unauthenticated.
      '';
    };

    user = mkOption {
      type = types.str;
      default = "internal-dashboard";
      description = ''
        System user to run as. With
        {option}`services.internal-dashboard.database.createLocally` this is
        also the PostgreSQL role, because peer authentication matches the two by
        name.
      '';
    };

    group = mkOption {
      type = types.str;
      default = "internal-dashboard";
      description = "System group to run as.";
    };

    logLevel = mkOption {
      type = types.str;
      default = "info";
      example = "internal_dashboard=debug,tower_http=debug,info";
      description = "`RUST_LOG` filter for the service.";
    };

    pool = {
      maxConnections = mkOption {
        type = types.ints.positive;
        default = 10;
        description = ''
          Size of the dashboard's own connection pool. Keep the total across
          every client below the server's `max_connections`.
        '';
      };

      acquireTimeout = mkOption {
        type = types.ints.positive;
        default = 5;
        description = ''
          Seconds a request waits for a free pooled connection before failing.
        '';
      };
    };

    environment = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        RUST_BACKTRACE = "1";
      };
      description = ''
        Extra environment variables for the service. Applied last, so these
        override everything the module sets, `DATABASE_URL` included.
      '';
    };

    environmentFile = mkOption {
      type = types.nullOr types.path;
      default = null;
      example = "/run/secrets/internal-dashboard.env";
      description = ''
        Path to an `EnvironmentFile` read at start, for secrets that must stay
        out of the Nix store. systemd reads `EnvironmentFile=` after
        `Environment=`, so these win over
        {option}`services.internal-dashboard.environment`.
      '';
    };

    database = {
      createLocally = mkOption {
        type = types.bool;
        default = true;
        description = ''
          Provision PostgreSQL on this host with a database and a role owning
          it, and connect over the unix socket using peer authentication. No
          password is involved and nothing has to be kept in a secret file.

          Set to `false` to use a server you manage yourself, then supply the
          connection string through
          {option}`services.internal-dashboard.database.url` or as
          `DATABASE_URL` in
          {option}`services.internal-dashboard.environmentFile`.
        '';
      };

      name = mkOption {
        type = types.str;
        default = "internal-dashboard";
        description = ''
          Database name. Under
          {option}`services.internal-dashboard.database.createLocally` it must
          equal {option}`services.internal-dashboard.user`, because
          `ensureDBOwnership` grants a role ownership of the database sharing
          its name.
        '';
      };

      url = mkOption {
        type = types.nullOr types.str;
        default = null;
        example = "postgres://dashboard@db.internal/dashboard";
        description = ''
          Connection string, used when
          {option}`services.internal-dashboard.database.createLocally` is
          disabled.

          This ends up in the unit file, which is world-readable in the Nix
          store. A URL carrying a password belongs in
          {option}`services.internal-dashboard.environmentFile` instead, where
          it overrides this one.
        '';
      };

      extensions = mkOption {
        type = types.listOf types.str;
        default = [ "pg_trgm" ];
        example = [
          "pg_trgm"
          "unaccent"
        ];
        description = ''
          Extensions to create in the database before the service starts, as
          the PostgreSQL superuser.

          `pg_trgm` is listed by default because the link search is a
          `ilike '%…%'` scan that only trigram indexes can serve; the migration
          that creates those indexes also creates the extension, so this is
          belt-and-braces for extensions the migration cannot create itself —
          untrusted ones need a superuser, which the dashboard's role is not.
        '';
      };

      settings = mkOption {
        type = types.attrsOf (
          types.oneOf [
            types.bool
            types.int
            types.str
          ]
        );
        default = { };
        example = {
          shared_buffers = "2GB";
          log_min_duration_statement = "500ms";
        };
        description = ''
          Extra `postgresql.conf` settings, merged into
          {option}`services.postgresql.settings` at normal priority so they
          beat anything
          {option}`services.internal-dashboard.database.tuning` sets.
        '';
      };

      tuning = {
        enable = mkOption {
          type = types.bool;
          default = true;
          description = ''
            Apply the derived Postgres settings below. Every one of them is set
            with `mkDefault`, so individual keys can still be overridden through
            {option}`services.internal-dashboard.database.settings` or
            {option}`services.postgresql.settings` without turning this off.
          '';
        };

        memoryMB = mkOption {
          type = types.ints.positive;
          default = 1024;
          example = 8192;
          description = ''
            Memory budget in MiB that PostgreSQL may treat as its own. Not the
            host's total: leave room for the dashboard, the page cache and
            anything else on the machine. `shared_buffers`,
            `effective_cache_size`, `maintenance_work_mem` and `work_mem` are
            all derived from it, so this is the one number to change.
          '';
        };

        maxConnections = mkOption {
          type = types.ints.positive;
          default = 100;
          description = ''
            Server-side `max_connections`. `work_mem` is sized against it, on
            the assumption that a connection may run a couple of sorts at once.
          '';
        };

        diskType = mkOption {
          type = types.enum [
            "ssd"
            "hdd"
          ];
          default = "ssd";
          description = ''
            Storage the cluster sits on, which sets `random_page_cost` and
            `effective_io_concurrency`. On `hdd` the planner is much less
            willing to use the trigram indexes.
          '';
        };

        statementTimeout = mkOption {
          type = types.str;
          default = "30s";
          description = ''
            `statement_timeout`. This applies to migrations too, so raise it
            before adding one that builds an index over a large table.
          '';
        };

        lockTimeout = mkOption {
          type = types.str;
          default = "10s";
          description = "`lock_timeout`.";
        };

        idleTransactionTimeout = mkOption {
          type = types.str;
          default = "60s";
          description = "`idle_in_transaction_session_timeout`.";
        };
      };

      pgbouncer = {
        enable = mkOption {
          type = types.bool;
          default = false;
          description = ''
            Put pgbouncer between the dashboard and PostgreSQL, on its own unix
            socket with peer authentication.

            Worth knowing before enabling it: the dashboard is a single process
            holding one small pool over a local socket, which is not the
            workload connection pooling exists to fix. It earns its keep once
            other clients share the database.
          '';
        };

        port = mkOption {
          type = types.port;
          default = 6432;
          description = ''
            Port pgbouncer listens on. It only listens on the unix socket
            `/run/pgbouncer/.s.PGSQL.''${port}`, never on TCP.
          '';
        };

        poolMode = mkOption {
          type = types.enum [
            "session"
            "transaction"
            "statement"
          ];
          default = "transaction";
          description = ''
            pgbouncer `pool_mode`. `transaction` is the default because it is
            the mode that actually multiplexes; the dashboard runs no explicit
            transactions and holds no session state, so it is safe here.

            Note that sqlx uses protocol-level prepared statements, which need
            {option}`services.internal-dashboard.database.pgbouncer.maxPreparedStatements`
            above zero in any mode other than `session`.
          '';
        };

        poolSize = mkOption {
          type = types.ints.positive;
          default = 20;
          description = "pgbouncer `default_pool_size`.";
        };

        maxClientConn = mkOption {
          type = types.ints.positive;
          default = 100;
          description = "pgbouncer `max_client_conn`.";
        };

        maxPreparedStatements = mkOption {
          type = types.ints.unsigned;
          default = 200;
          description = ''
            pgbouncer `max_prepared_statements`. Must be above zero unless
            {option}`services.internal-dashboard.database.pgbouncer.poolMode`
            is `session`, because sqlx prepares every statement it runs.
          '';
        };
      };
    };
  };

  config = mkIf cfg.enable {
    assertions = [
      {
        assertion = db.createLocally -> db.name == cfg.user;
        message = ''
          services.internal-dashboard: database.createLocally grants the role
          ownership of the database with the same name, so database.name
          ("${db.name}") must equal user ("${cfg.user}").
        '';
      }
      {
        assertion = db.createLocally || db.url != null || cfg.environmentFile != null;
        message = ''
          services.internal-dashboard: with database.createLocally disabled you
          must set database.url, or provide DATABASE_URL through
          environmentFile.
        '';
      }
      {
        assertion = bouncer.enable -> db.createLocally;
        message = ''
          services.internal-dashboard: database.pgbouncer.enable wires pgbouncer
          to the local cluster over a peer-authenticated socket, so it needs
          database.createLocally. Point database.url at your own pgbouncer
          instead.
        '';
      }
      {
        assertion = (bouncer.enable && bouncer.poolMode != "session") -> bouncer.maxPreparedStatements > 0;
        message = ''
          services.internal-dashboard: pgbouncer pool_mode "${bouncer.poolMode}"
          with max_prepared_statements = 0 breaks sqlx, which prepares every
          statement it runs. Raise
          database.pgbouncer.maxPreparedStatements, or use pool_mode "session".
        '';
      }
    ];

    services.postgresql = mkIf db.createLocally {
      enable = true;
      ensureDatabases = [ db.name ];
      ensureUsers = [
        {
          name = cfg.user;
          ensureDBOwnership = true;
        }
      ];
      settings = lib.mapAttrs (_: mkDefault) (optionalAttrs tuning.enable tunedSettings) // db.settings;
    };

    services.pgbouncer = mkIf (db.createLocally && bouncer.enable) {
      enable = true;
      # Running as the dashboard's own user is what makes peer authentication
      # work in both directions: clients are checked against this user, and the
      # onward connection to PostgreSQL presents it as the role name.
      user = cfg.user;
      group = cfg.group;
      settings = {
        databases.${db.name} = "host=/run/postgresql dbname=${db.name}";
        pgbouncer = {
          listen_addr = null;
          listen_port = bouncer.port;
          unix_socket_dir = "/run/pgbouncer";
          pool_mode = bouncer.poolMode;
          default_pool_size = bouncer.poolSize;
          max_client_conn = bouncer.maxClientConn;
          max_prepared_statements = bouncer.maxPreparedStatements;
          auth_type = "hba";
          auth_hba_file = toString pgbouncerHba;
          # Parameters pgbouncer cannot track across a pooled connection, and
          # which the client is allowed to set anyway.
          ignore_startup_parameters = "extra_float_digits,options";
        };
      };
    };

    users.users = optionalAttrs (cfg.user == "internal-dashboard") {
      internal-dashboard = {
        inherit (cfg) group;
        isSystemUser = true;
      };
    };

    users.groups = optionalAttrs (cfg.group == "internal-dashboard") {
      internal-dashboard = { };
    };

    networking.firewall.allowedTCPPorts = mkIf cfg.openFirewall [ cfg.port ];

    # Extensions have to exist before the dashboard runs its migrations, and
    # creating an untrusted one needs the superuser the dashboard is not.
    systemd.services.internal-dashboard-db-setup = mkIf (db.createLocally && db.extensions != [ ]) {
      description = "Create PostgreSQL extensions for the internal dashboard";
      after = [ "postgresql.service" ];
      requires = [ "postgresql.service" ];
      before = [ "internal-dashboard.service" ];
      wantedBy = [ "internal-dashboard.service" ];

      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
        User = config.services.postgresql.superUser;
        Group = "postgres";
      };

      script = concatMapStringsSep "\n" (ext: ''
        ${config.services.postgresql.package}/bin/psql -d ${lib.escapeShellArg db.name} \
          -tAc ${lib.escapeShellArg ''CREATE EXTENSION IF NOT EXISTS "${ext}"''}
      '') db.extensions;
    };

    systemd.services.internal-dashboard = {
      description = "Internal link dashboard";
      wantedBy = [ "multi-user.target" ];
      wants = [ "network-online.target" ];
      after = [
        "network-online.target"
      ]
      ++ optionals db.createLocally [ "postgresql.service" ]
      ++ optionals (db.createLocally && bouncer.enable) [ "pgbouncer.service" ];
      requires =
        optionals db.createLocally [ "postgresql.service" ]
        ++ optionals (db.createLocally && bouncer.enable) [ "pgbouncer.service" ];

      environment = {
        BIND_ADDR = bindAddr;
        RUST_LOG = cfg.logLevel;
        DB_MAX_CONNECTIONS = toString cfg.pool.maxConnections;
        DB_ACQUIRE_TIMEOUT_SECS = toString cfg.pool.acquireTimeout;
      }
      // optionalAttrs db.createLocally { DATABASE_URL = localUrl; }
      // optionalAttrs (!db.createLocally && db.url != null) { DATABASE_URL = db.url; }
      // cfg.environment;

      serviceConfig = {
        Type = "exec";
        ExecStart = lib.getExe cfg.package;
        User = cfg.user;
        Group = cfg.group;
        Restart = "on-failure";
        RestartSec = 5;

        EnvironmentFile = mkIf (cfg.environmentFile != null) [ cfg.environmentFile ];

        # Migrations and the htmx assets are compiled into the binary, so the
        # service needs nothing writable anywhere.
        # block=yes so the multi-line list values are sorted as whole entries;
        # without it keep-sorted --mode fix drops their closing brackets.
        # keep-sorted start block=yes
        AmbientCapabilities = optionals (cfg.port < 1024) [ "CAP_NET_BIND_SERVICE" ];
        CapabilityBoundingSet = optionals (cfg.port < 1024) [ "CAP_NET_BIND_SERVICE" ];
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        NoNewPrivileges = true;
        PrivateDevices = true;
        PrivateTmp = true;
        ProcSubset = "pid";
        ProtectClock = true;
        ProtectControlGroups = true;
        ProtectHome = true;
        ProtectHostname = true;
        ProtectKernelLogs = true;
        ProtectKernelModules = true;
        ProtectKernelTunables = true;
        ProtectProc = "invisible";
        ProtectSystem = "strict";
        RemoveIPC = true;
        RestrictAddressFamilies = [
          "AF_INET"
          "AF_INET6"
          "AF_UNIX"
        ];
        RestrictNamespaces = true;
        RestrictRealtime = true;
        RestrictSUIDSGID = true;
        SystemCallArchitectures = "native";
        SystemCallFilter = [
          "@system-service"
          "~@privileged"
          "~@resources"
        ];
        UMask = "0077";
        # keep-sorted end
      };
    };
  };
}
