{ inputs, ... }:
{
  perSystem = { self', pkgs, lib, ... }:
    let
      toolchainForPkgs = p:
        p.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

      craneLib = (inputs.crane.mkLib pkgs).overrideToolchain toolchainForPkgs;

      src = craneLib.cleanCargoSource ./.;

      # Common arguments can be set here to avoid repeating them later
      commonArgs = {
        inherit src;
        strictDeps = true;

        buildInputs =
          [
          ]
          ++ lib.optionals pkgs.stdenv.isDarwin [
            # pkgs.libiconv
          ];
      };

      cargoArtifacts = craneLib.buildDepsOnly commonArgs;

      hazel = craneLib.buildPackage (
        commonArgs
        // {
          inherit cargoArtifacts;
          doCheck = false;
        }
      );
    in
    {
      checks = {
        inherit hazel;

        hazel-clippy = craneLib.cargoClippy (
          commonArgs
          // {
            inherit cargoArtifacts;
            cargoClippyExtraArgs = "--all-targets -- --deny warnings";
          }
        );

        hazel-nextest = craneLib.cargoNextest (
          commonArgs
          // {
            inherit cargoArtifacts;
            partitions = 1;
            partitionType = "count";
            cargoNextestPartitionsExtraArgs = "--no-tests=pass";
          }
        );
      };

      packages = {
        default = hazel;
      };

      apps.default = {
        type = "app";
        program = "${hazel}/bin/hazel";
        meta.description = "";
      };

      devShells.rust = craneLib.devShell {
        checks = {
          inherit (self'.checks) hazel hazel-clippy;
        };

        shellHook = ''
          # For rust-analyzer 'hover' tooltips to work.
          # shell hook because setting the attribute doesn't work for me and i'm too
          # lazy to figure out why.
          export RUST_SRC_PATH="${toolchainForPkgs pkgs}/lib/rustlib/src/rust/library";
        '';
      };
    };
}
