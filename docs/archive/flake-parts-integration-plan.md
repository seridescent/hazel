# Flake-Parts Integration Plan

## Overview

Hazel provides a flake-parts module that users import. The module:
1. Defines `perSystem.hazel.<name>` options for staging configs
2. Produces outputs that hazel can read via `nix eval`
3. Hazel then executes preStart and command with `$DEPLOY_DIR` set

## User's flake.nix (Consumer)

```nix
{
  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
    hazel.url = "github:seridescent/hazel";
  };

  outputs = inputs@{ flake-parts, ... }:
    flake-parts.lib.mkFlake { inherit inputs; } {
      imports = [
        inputs.hazel.flakeModules.default  # Import hazel module
        ./rust.nix
      ];
      systems = [ "x86_64-linux" "aarch64-darwin" ];

      perSystem = { self', pkgs, ... }: {
        # User defines their staging config
        hazel.staging = {
          package = self'.packages.default;
          command = "${self'.packages.default}/bin/myapp";
          env = {
            MYAPP_CONFIG = "staging";
          };
          preStart = ''
            cp ${./fixtures/test-data.txt} $DEPLOY_DIR/
            cp ${./fixtures/seed.db} $DEPLOY_DIR/db.sqlite
          '';
        };
      };
    };
}
```

## Hazel's flake-parts Module

```nix
# hazel/flake-module.nix
{ lib, flake-parts-lib, ... }:
let
  inherit (lib) mkOption types;
  inherit (flake-parts-lib) mkPerSystemOption;
in
{
  options.perSystem = mkPerSystemOption ({ config, pkgs, ... }: {
    options.hazel = mkOption {
      type = types.attrsOf (types.submodule {
        options = {
          package = mkOption {
            type = types.package;
            description = "The package to deploy";
          };
          command = mkOption {
            type = types.str;
            description = "Command to run (can reference store paths)";
          };
          env = mkOption {
            type = types.attrsOf types.str;
            default = {};
            description = "Environment variables";
          };
          preStart = mkOption {
            type = types.lines;
            default = "";
            description = "Script to run before command, in $DEPLOY_DIR";
          };
        };
      });
      default = {};
    };

    # Generate outputs for hazel to consume
    options.hazelOutputs = mkOption {
      type = types.attrsOf types.attrs;
      readOnly = true;
      description = "Generated outputs for hazel (don't set manually)";
    };

    config.hazelOutputs = lib.mapAttrs (name: cfg: {
      # preStart becomes a script in the nix store
      preStartScript = pkgs.writeShellScript "hazel-prestart-${name}" ''
        set -euo pipefail
        ${cfg.preStart}
      '';
      command = cfg.command;
      env = cfg.env;
      # Include package so hazel can ensure it's built
      package = cfg.package;
    }) config.hazel;
  });

  # Expose hazelOutputs at the flake level
  options.flake.hazelOutputs = mkOption {
    type = types.lazyAttrsOf (types.lazyAttrsOf types.attrs);
  };
}
```

## Hazel's Execution Flow

### Step 1: Detect current system

```rust
fn nix_system() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("macos", "aarch64") => "aarch64-darwin",
        ("macos", "x86_64") => "x86_64-darwin",
        ("linux", "x86_64") => "x86_64-linux",
        ("linux", "aarch64") => "aarch64-linux",
        _ => panic!("unsupported system"),
    }
}
```

Or use nix itself:
```bash
nix eval --impure --expr 'builtins.currentSystem' --raw
# Returns: aarch64-darwin
```

### Step 2: Build everything with `nix build --json`

```bash
# Build package, preStartScript, and get paths
nix build .#legacyPackages.<system>.hazel.staging.package --json
nix build .#legacyPackages.<system>.hazel.staging.preStartScript --json
```

`nix build --json` returns:
```json
[{"drvPath":"/nix/store/...-hazel.drv","outputs":{"out":"/nix/store/...-hazel"}}]
```

We extract the `outputs.out` path.

For `env` and `command` (which are strings, not derivations), we still need eval:
```bash
nix eval .#legacyPackages.<system>.hazel.staging.command --raw
nix eval .#legacyPackages.<system>.hazel.staging.env --json
```

### Step 3: Run preStart script

```rust
let deploy_dir = data_dir.join("deploys").join(&repo.to_string()).join(format!("pr-{}", pr_number));
tokio::fs::create_dir_all(&deploy_dir).await?;

Command::new(&pre_start_script_path)  // from nix build --json
    .env("DEPLOY_DIR", &deploy_dir)
    .current_dir(&deploy_dir)
    .status()
    .await?;
```

### Step 4: Run the command

```rust
Command::new("sh")
    .args(["-c", &command])  // from nix eval --raw
    .envs(&env)              // from nix eval --json
    .env("DEPLOY_DIR", &deploy_dir)
    .current_dir(&deploy_dir)
    .spawn()?;
```

## Nix Commands Summary

| Command | Purpose | Output |
|---------|---------|--------|
| `nix build .#...package --json` | Build package, get store path | JSON with `outputs.out` |
| `nix build .#...preStartScript --json` | Build script, get store path | JSON with `outputs.out` |
| `nix eval .#...command --raw` | Get command string | Raw string |
| `nix eval .#...env --json` | Get env vars | JSON object |

**Key insight:** `nix build --json` both builds AND returns the store path. No separate eval needed for derivations.

## System Detection

Nix auto-detects system for standard outputs (`packages`, `apps`), but not for `legacyPackages`.

**Options:**
1. Detect in Rust (recommended for simplicity):
   ```rust
   fn nix_system() -> &'static str { ... }
   ```
2. Query nix:
   ```bash
   nix eval --impure --expr 'builtins.currentSystem' --raw
   ```

Both work. Rust detection avoids an extra process spawn.

3. **Multiple staging configs**: What if user defines `hazel.staging` and `hazel.production`?
   - Hazel could deploy all of them, or take a CLI arg

## Test Plan

1. Update test repo's main.rs to:
   - Read `$MYAPP_CONFIG` env var
   - Read `$DEPLOY_DIR/test-data.txt` file
   - Print both, exit 0 if found, exit 1 if missing

2. Add hazel config to test repo's flake.nix

3. Manually test the flow:
   ```bash
   cd data/repos/seridescent/hazel-test-repo/worktrees/pr-1
   nix build .#packages.aarch64-darwin.default
   nix eval .#hazelOutputs.aarch64-darwin.staging --json
   # Then run preStart and command manually
   ```

4. Implement in hazel once manual flow works
