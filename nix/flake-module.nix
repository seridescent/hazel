# Hazel flake-parts module
#
# Provides options for configuring PR staging deploys.
# Users define `hazel.deploy` with preStart script and executable,
# and the module exposes wrapped packages for hazel to invoke via `nix run`.
#
# Environment variables provided by hazel at runtime:
#   - HAZEL_PORT: The port the service should listen on
#   - HAZEL_RUN_DIR: Working directory for the service
{ lib, flake-parts-lib, ... }:
let
  inherit (lib) mkOption mkEnableOption types mkIf;
  inherit (flake-parts-lib) mkPerSystemOption;
in
{
  options.perSystem = mkPerSystemOption ({ config, pkgs, ... }:
    let
      cfg = config.hazel.deploy;
      hazelEnabled = cfg.enable;
    in
    {
      options.hazel.deploy = {
        enable = mkEnableOption "hazel deploy configuration";

        preStart = mkOption {
          type = types.lines;
          default = "";
          description = ''
            Script to run before the service is started.
            Has access to HAZEL_RUN_DIR for populating the working directory
            with external data (e.g., fixture data, env files).
          '';
        };

        executable = mkOption {
          type = types.package;
          description = ''
            A derivation that serves as the start script for the service.
            Should be runnable via `nix run` and respect HAZEL_PORT for binding.
            Typically created with pkgs.writeShellApplication.
          '';
        };
      };

      config = mkIf hazelEnabled {
        # Expose packages for hazel to invoke via `nix run <flake-ref>#<name>`
        packages = {
          hazel-preStart = pkgs.writeShellApplication {
            name = "hazel-preStart";
            text = ''
              if [ -z "''${HAZEL_RUN_DIR:-}" ]; then
                echo "Error: HAZEL_RUN_DIR is not set" >&2
                exit 1
              fi
              ${cfg.preStart}
            '';
          };

          hazel-executable = pkgs.writeShellApplication {
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
              exec ${cfg.executable}/bin/${cfg.executable.name}
            '';
          };
        };
      };
    });
}
