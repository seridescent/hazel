{ withSystem, ... }: {
  flake.nixosModules.default = { lib, config, pkgs, ... }:
    let
      cfg = config.services.hazel;
    in
    {
      options = {
        services.hazel = {
          enable = lib.mkEnableOption "hazel PR preview service";

          package = lib.mkOption {
            description = "The hazel service package to use";
            default = withSystem pkgs.stdenv.hostPlatform.system ({ config, ... }:
              config.packages.default
            );
            type = lib.types.package;
          };

          user = lib.mkOption {
            description = "User account under which the service runs";
            default = "hazel";
            type = lib.types.str;
          };

          group = lib.mkOption {
            description = "Group under which the service runs";
            default = "hazel";
            type = lib.types.str;
          };

          dataDir = lib.mkOption {
            description = "Data directory for the service";
            default = "/var/lib/hazel";
            type = lib.types.path;
          };

          githubAppId = lib.mkOption {
            description = "GitHub App ID";
            type = lib.types.int;
          };

          githubAppKeyPath = lib.mkOption {
            description = "Path to the GitHub App private key file";
            type = lib.types.path;
          };

          watchedRepo = {
            owner = lib.mkOption {
              description = "Owner of the repository to watch";
              type = lib.types.str;
            };

            name = lib.mkOption {
              description = "Name of the repository to watch";
              type = lib.types.str;
            };
          };

          portRange = {
            min = lib.mkOption {
              description = "Minimum port for deploy allocations";
              type = lib.types.port;
              default = 50000;
            };

            max = lib.mkOption {
              description = "Maximum port for deploy allocations";
              type = lib.types.port;
              default = 50100;
            };
          };

          pollIntervalSecs = lib.mkOption {
            description = "Polling interval in seconds";
            type = lib.types.int;
            default = 30;
          };

          production = {
            enable = lib.mkEnableOption "production deployment";

            runDir = lib.mkOption {
              description = "Persistent directory for production deployment runtime data (HAZEL_RUN_DIR). This directory persists between deployments for SQLite state, etc.";
              type = lib.types.path;
              example = "/var/lib/myapp-production";
            };

            branch = lib.mkOption {
              description = "Branch to watch for production deployment";
              type = lib.types.str;
              default = "main";
            };

            port = lib.mkOption {
              description = "Fixed port for production deployment";
              type = lib.types.port;
            };
          };
        };
      };

      config = lib.mkIf cfg.enable {
        users.users = lib.mkIf (cfg.user == "hazel") {
          hazel = {
            inherit (cfg) group;
            isSystemUser = true;
          };
        };

        users.groups = lib.mkIf (cfg.group == "hazel") {
          hazel = { };
        };

        # Ensure production runDir exists with proper ownership
        systemd.tmpfiles.rules = lib.mkIf cfg.production.enable [
          "d ${cfg.production.runDir} 0755 ${cfg.user} ${cfg.group} -"
        ];

        systemd.services.hazel = {
          description = "Hazel PR preview deployment service";
          wantedBy = [ "multi-user.target" ];
          path = [ pkgs.git pkgs.tailscale config.nix.package pkgs.gnutar ];

          environment = {
            # nicer to have a place for the service user's nix cache
            HOME = cfg.dataDir;
            XDG_CACHE_HOME = "${cfg.dataDir}/.cache";

            # application configuration
            RUST_LOG = "info";
            HAZEL_DATA_DIR = "${cfg.dataDir}/ephemeral";
            GITHUB_APP_ID = toString cfg.githubAppId;
            GITHUB_APP_KEY_PATH = cfg.githubAppKeyPath;
            HAZEL_WATCHED_REPO_OWNER = cfg.watchedRepo.owner;
            HAZEL_WATCHED_REPO_NAME = cfg.watchedRepo.name;
            HAZEL_PORT_MIN = toString cfg.portRange.min;
            HAZEL_PORT_MAX = toString cfg.portRange.max;
            HAZEL_POLL_INTERVAL_SECS = toString cfg.pollIntervalSecs;
          } // lib.optionalAttrs cfg.production.enable {
            HAZEL_PRODUCTION_ENABLE = "true";
            HAZEL_PRODUCTION_RUN_DIR = cfg.production.runDir;
            HAZEL_PRODUCTION_BRANCH = cfg.production.branch;
            HAZEL_PRODUCTION_PORT = toString cfg.production.port;
          };

          serviceConfig = {
            ExecStart = "${cfg.package}/bin/hazel";
            Restart = "on-failure";
            RestartSec = 5;

            User = cfg.user;
            Group = cfg.group;
            WorkingDirectory = cfg.dataDir;

            PrivateTmp = true;
            NoNewPrivileges = true;
          };
        };
      };
    };
}
