{
  description = "hazel";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";

    rust-overlay.url = "github:oxalica/rust-overlay";
    rust-overlay.inputs.nixpkgs.follows = "nixpkgs";

    crane.url = "github:ipetkov/crane";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        ./rust.nix
        ./nix/nixos-modules.nix
      ];
      systems = [ "x86_64-linux" "aarch64-linux" "aarch64-darwin" "x86_64-darwin" ];
      perSystem = { self', pkgs, system, ... }: {
        # https://flake.parts/overlays.html
        _module.args.pkgs = import inputs.nixpkgs
          {
            inherit system;
            overlays = [ inputs.rust-overlay.overlays.default ];
          };


        devShells.default = pkgs.mkShell
          {
            inputsFrom = [
              self'.devShells.rust
            ];

            packages = [ pkgs.uv ];
          };
      };
      flake = {
        # Flake-parts module for consumers to import
        flakeModules.default = ./nix/flake-module.nix;
      };
    };
}
