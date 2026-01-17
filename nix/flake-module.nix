# Hazel flake-parts module
#
# Provides options for configuring PR staging deploys and production deploys.
# Users define `hazel.staging` with preStart script and executable for PR previews,
# and `hazel.production` with just an executable for production deployments.
# The module exposes wrapped packages for hazel to invoke via `nix run`.
#
# Environment variables provided by hazel at runtime:
#   - HAZEL_PORT: The port the service should listen on
#   - HAZEL_RUN_DIR: Working directory for the service
#   - HAZEL_ORIGIN: The full origin URL (e.g., http://hostname:50001)
{ lib, flake-parts-lib, ... }:
{
  options.perSystem = flake-parts-lib.mkPerSystemOption ({ config, pkgs, ... }:
    let
      stagingCfg = config.hazel.staging;
      productionCfg = config.hazel.production;
    in
    {
      options.hazel.staging = {
        enable = lib.mkEnableOption "hazel staging configuration for PR previews";

        preStart = lib.mkOption {
          type = lib.types.lines;
          default = "";
          description = ''
            Script to run before the service is started.
            Has access to HAZEL_RUN_DIR and HAZEL_ORIGIN for populating
            the working directory with external data (e.g., fixture data, env files).
          '';
        };

        executable = lib.mkOption {
          type = lib.types.package;
          description = ''
            A derivation that serves as the start script for the service.
            Should be runnable via `nix run` and respect HAZEL_PORT for binding.
            Also has access to HAZEL_RUN_DIR and HAZEL_ORIGIN.
            Typically created with pkgs.writeShellApplication.
          '';
        };
      };

      options.hazel.production = {
        enable = lib.mkEnableOption "hazel production deployment configuration";

        executable = lib.mkOption {
          type = lib.types.package;
          description = ''
            A derivation that serves as the start script for the production service.
            Should be runnable via `nix run` and respect HAZEL_PORT for binding.
            Also has access to HAZEL_RUN_DIR (persistent) and HAZEL_ORIGIN.
            Typically created with pkgs.writeShellApplication.
          '';
        };
      };

      config = lib.mkMerge [
        (lib.mkIf stagingCfg.enable {
          packages.hazel-preStart = pkgs.writeShellApplication {
            name = "hazel-preStart";
            text = ''
              if [ -z "''${HAZEL_RUN_DIR:-}" ]; then
                echo "Error: HAZEL_RUN_DIR is not set" >&2
                exit 1
              fi
              ${stagingCfg.preStart}
            '';
          };

          packages.hazel-executable = pkgs.writeShellApplication {
            name = "hazel-executable";
            text = ''
              if [ -z "''${HAZEL_RUN_DIR:-}" ]; then
                echo "Error: HAZEL_RUN_DIR is not set" >&2
                exit 1
              fi
              if [ -z "''${HAZEL_PORT:-}" ]; then
                echo "Error: HAZEL_PORT is not set" >&2
                exit 1
              fi
              exec ${stagingCfg.executable}/bin/${stagingCfg.executable.name}
            '';
          };
        })

        (lib.mkIf productionCfg.enable {
          packages.hazel-production-executable = pkgs.writeShellApplication {
            name = "hazel-production-executable";
            text = ''
              if [ -z "''${HAZEL_RUN_DIR:-}" ]; then
                echo "Error: HAZEL_RUN_DIR is not set" >&2
                exit 1
              fi
              if [ -z "''${HAZEL_PORT:-}" ]; then
                echo "Error: HAZEL_PORT is not set" >&2
                exit 1
              fi
              exec ${productionCfg.executable}/bin/${productionCfg.executable.name}
            '';
          };
        })
      ];
    });
}
