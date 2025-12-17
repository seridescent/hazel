# how does hazel interact with nix to deploy something?

## example usage

```nix
{
  description = "hazel-test-repo";

  inputs = {
    flake-parts.url = "github:hercules-ci/flake-parts";
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

    bun2nix.url = "github:nix-community/bun2nix";
    bun2nix.inputs.nixpkgs.follows = "nixpkgs";
    
    hazel.url = "github:seridescent/hazel";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [ inputs.hazel.flakeModule ];
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      perSystem = { system, pkgs, config, ... }:
        {
          # https://flake.parts/overlays.html
          _module.args.pkgs = import inputs.nixpkgs
            {
              inherit system;
              overlays = [ inputs.bun2nix.overlays.default ];
            };

          packages = {
            default =
              pkgs.stdenv.mkDerivation
                {
                  pname = "hazel-test-repo";
                  version = "0.0.1";

                  src = pkgs.lib.cleanSource ./.;

                  nativeBuildInputs = [ pkgs.bun2nix.hook ];
                  bunDeps = pkgs.bun2nix.fetchBunDeps {
                    bunNix = ./bun.nix;
                  };

                  buildPhase = ''
                    bun run build
                  '';

                  installPhase = ''
                    mkdir -p $out
                    cp -r build/* $out/
                  '';

                  meta = {
                    description = "hazel-test-repo";
                  };
                };
          };
                    
          # RELEVANT MODULE CONFIG EXAMPLE HERE
          hazel.deploy = {
            # hazel will expose these env vars for consumption:
            # - HAZEL_PORT: the port the service should listen on
            # - HAZEL_RUN_DIR: the working directory for the service
            # - HAZEL_ORIGIN: the origin URL (e.g., http://hostname:port)
            # - HAZEL_BASE_PATH: the base path for the deploy (e.g., /<sha>)
            
            # a user-defined script that runs before the service is started
            # this script will have access to HAZEL_RUN_DIR so that the user
            # can populate the working directory with external data if they want,
            # such as staging fixture data or an env file.
            preStart = ''
              cp ${./data.txt} $HAZEL_RUN_DIR
            '';
            
            # expecting this to be a user-provided derivation that is basically
            # a start script. whatever it is, i plan for the implementation to
            # very simply call `HAZEL_PORT={port_num} nix run <flake-ref>`.
            executable = pkgs.writeShellApplication {
              name = "hazel-test-repo-server";
              text = ''
                export BUN_PORT=$HAZEL_PORT  
                ${pkgs.bun}/bin/bun ${config.packages.default}/index.js
              '';
            };
          };
        };
    };
}
```
