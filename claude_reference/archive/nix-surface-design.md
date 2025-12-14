# Hazel Nix User Surface Design

## Problem

We need a way for users to specify:
1. What to build (nix derivation)
2. How to run it (command, env vars)
3. Runtime setup (mutable files, test fixtures, database seeding)

Standard flake outputs don't fit well:
- `packages` - just a derivation, no runtime config
- `apps` - just a path to a binary, no env/setup hooks
- `checks` - for verification, not long-running services

## Key Insight: Two Contexts

### Build Context (nix)
- Immutable, hermetic
- `$out` = nix store path (e.g., `/nix/store/abc123-myapp`)
- Result is frozen forever
- User writes standard nix derivation

### Deploy Context (hazel)
- Mutable working directory
- Hazel controls the environment
- User needs hooks to set up runtime state
- App can write to disk (logs, sqlite, uploads, etc.)

## Proposed Schema

Custom flake output `hazel.<system>.<name>`:

```nix
{
  hazel.x86_64-linux.staging = {
    # The package to deploy (built by nix, immutable)
    package = self.packages.x86_64-linux.myapp;

    # Deploy-time configuration (run by hazel, mutable context)
    deploy = {
      # Environment variables
      # Can reference hazel-provided vars: $DEPLOY_DIR, $DATA_DIR, $PACKAGE
      env = {
        PORT = "3000";
        DATABASE_URL = "sqlite:$DATA_DIR/db.sqlite";
        LOG_LEVEL = "debug";
      };

      # Runs before the main command, in $DEPLOY_DIR
      # Use for: copying fixtures, creating directories, seeding DB
      preStart = ''
        mkdir -p $DATA_DIR
        if [ ! -f $DATA_DIR/db.sqlite ]; then
          cp $PACKAGE/share/fixtures/seed.sql $DATA_DIR/
          sqlite3 $DATA_DIR/db.sqlite < $DATA_DIR/seed.sql
        fi
      '';

      # The command to run (working directory is $DEPLOY_DIR)
      command = "$PACKAGE/bin/myapp serve --port $PORT";

      # Optional: runs after command exits
      postStop = ''
        echo "Cleaning up..."
      '';
    };
  };
}
```

## Hazel-Provided Variables

At deploy time, hazel provides:

| Variable | Description |
|----------|-------------|
| `$PACKAGE` | Path to built nix store derivation |
| `$DEPLOY_DIR` | Mutable working directory for this deploy |
| `$DATA_DIR` | Persistent data directory (survives restarts) |
| `$PR_NUMBER` | PR number being deployed |
| `$COMMIT_SHA` | Git commit SHA |
| `$REPO` | Repository (owner/name) |

## Directory Structure

```
$HAZEL_DATA_DIR/
  repos/
    owner/repo/
      repo.git/
      worktrees/
        pr-123/
  deploys/
    owner/repo/
      pr-123/
        data/           # $DATA_DIR - persistent
        run/            # $DEPLOY_DIR - working directory
        result -> /nix/store/...  # symlink to $PACKAGE
```

## Alternative: Phase Hooks (mkDerivation-style)

Could adopt mkDerivation's phase pattern:

```nix
hazel.staging = {
  package = self.packages.x86_64-linux.myapp;

  # Phases run in order, each has pre/post hooks
  phases = "preStart startPhase postStart";

  preStart = ''
    mkdir -p $DATA_DIR
  '';

  startPhase = ''
    exec $PACKAGE/bin/myapp serve
  '';

  # Override individual phases
  preStartHooks = [ ./setup-db.sh ];
};
```

Downside: more complex, maybe overkill for "just run this app with some setup".

## Open Questions

1. **Hot reload**: When PR updates, do we restart? Rebuild?
   - Probably: rebuild package, run preStart again, restart command

2. **Health checks**: How to know if deploy is healthy?
   - Could add `healthCheck = "curl localhost:$PORT/health"`

3. **Multiple services**: What if app needs worker + web?
   - Could allow list of deploys, or separate outputs

4. **Secrets**: How to handle sensitive env vars?
   - Maybe: `envFile = ./secrets.env` (gitignored)
   - Or hazel reads from its own secrets store

5. **Port allocation**: How to avoid conflicts across PRs?
   - Hazel could provide `$PORT` automatically
   - Or user specifies base port and hazel offsets by PR number

6. **Cleanup**: When PR closes, clean up deploy dir?
   - Probably yes, with configurable retention

## Inspiration

- **mkDerivation**: Phase hooks, pre/post pattern
- **mkShell**: Top-level attrs become env vars, `shellHook`
- **NixOS services**: `preStart`, `script`, `environment`, `serviceConfig`
- **docker-compose**: `environment`, `command`, `volumes`, `depends_on`

## Next Steps

1. Implement minimal version: package + env + preStart + command
2. Test with simple app that checks for env var and file
3. Iterate on schema based on real usage
