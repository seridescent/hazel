# Hazel flake-parts module
#
# Provides options for configuring PR staging deploys.
# Users define `hazel.deploys.<name>` and get `hazel.outputs.<name>` with
# processed values that hazel can consume via nix build/eval.
{ lib, flake-parts-lib, ... }:
let
  inherit (lib) mkOption types;
  inherit (flake-parts-lib) mkPerSystemOption;
in
{
  options.perSystem = mkPerSystemOption ({ config, pkgs, ... }: {
    options.hazel = {
      deploys = mkOption {
        type = types.attrsOf (types.submodule {
          options = {
            # TODO: replace
            package = mkOption {
              type = types.package;
              description = "The package to deploy";
            };
            command = mkOption {
              type = types.str;
              description = "Command to run (can reference store paths via nix interpolation)";
            };
            env = mkOption {
              type = types.attrsOf types.str;
              default = { };
              description = "Environment variables to set";
            };
            preStart = mkOption {
              type = types.lines;
              default = "";
              description = ''
                Script to run before the command.
                Runs in $DEPLOY_DIR with access to nix store paths.
              '';
            };
          };
        });
        default = { };
        description = "hazel deploy configurations";
      };

      wrappedInstallable = mkOption {
        type = types.attrsOf (types.submodule {
          options = {
            # TODO: replace
            package = mkOption { type = types.package; };
            command = mkOption { type = types.str; };
            env = mkOption { type = types.attrsOf types.str; };
            preStartScript = mkOption { type = types.package; };
          };
        });
        readOnly = true;
        description = "Generated outputs for hazel to consume (read-only)";
      };
    };
  });
}
