# Concurrency Model

## Overview

Each PR is an independent task. The deployment flow is linear per-PR but PRs can run concurrently.

## Components

### PortAllocator

Shared state, briefly locked to allocate a port:

```rust
struct PortAllocator {
    port_min: u16,
    port_max: u16,
    next_port: Mutex<u16>,
    // TODO: reclaimed: Mutex<BinaryHeap<Reverse<u16>>>
}

impl PortAllocator {
    fn allocate(&self) -> Result<u16>;
    fn release(&self, port: u16);  // adds to reclaimed heap
}
```

### Deployments

Tracks running deployments. Not shared during deploy - each task returns a `Deployment`, collected afterward:

```rust
struct Deployment {
    sha: String,
    port: u16,
    process: Child,
    serve: Child,
}

struct Deployments {
    by_sha: HashMap<String, Deployment>,
}
```

## Per-PR Task

```rust
async fn deploy_pr(
    pr: &PullRequest,
    port_allocator: &PortAllocator,
    fetch_url: &str,
    data_dir: &Path,
    tailscale_proxy_port: u16,
) -> Result<Deployment> {
    let sha = &pr.head.sha;
    let checkout_dir = data_dir.join("checkouts").join(sha);
    let run_dir = data_dir.join("deploys").join(sha);

    // 1. Extract commit (yields during git/tar)
    git::extract_commit(...).await?;

    // 2. Allocate port (brief lock)
    let port = port_allocator.allocate()?;

    // 3. Run preStart (yields during nix run)
    run_pre_start(&checkout_dir, &run_dir).await?;

    // 4. Spawn executable (yields during nix run)
    let process = spawn_executable(&checkout_dir, &run_dir, port).await?;

    // 5. Start tailscale serve (yields)
    let serve = start_tailscale_serve(sha, port, tailscale_proxy_port).await?;

    Ok(Deployment { sha, port, process, serve })
}
```

## Run Flow

```rust
async fn run(state: &mut AppState) -> Result<()> {
    // 1. Refresh token if expiring soon
    if state.token.expires_in() < Duration::from_secs(30) {
        state.token = fetch_new_token().await?;
    }

    // 2. Fetch open PRs
    let pulls = fetch_open_pulls(&state.token).await?;

    // 3. Deploy all PRs concurrently
    let deployments: Vec<Deployment> = futures::future::try_join_all(
        pulls.iter().map(|pr| deploy_pr(pr, &state.port_allocator, ...))
    ).await?;

    // 4. Track deployments
    for deployment in deployments {
        state.deployments.insert(deployment.sha.clone(), deployment);
    }

    // 5. Cleanup: remove old checkouts/deploys, kill stale processes
    cleanup_old_deployments(&state.deployments, &data_dir).await?;

    Ok(())
}
```

## Token Refresh/Installation handling

Use `CachedToken` (implementation below provided by octocrab):

```rust
#[derive(Debug, Clone)]
struct CachedTokenInner {
    expiration: Option<DateTime<Utc>>,
    secret: SecretString,
}

impl CachedTokenInner {
    fn new(secret: SecretString, expiration: Option<DateTime<Utc>>) -> Self {
        Self { secret, expiration }
    }

    fn expose_secret(&self) -> &str {
        self.secret.expose_secret()
    }
}

/// A cached API access token (which may be None)
pub struct CachedToken(RwLock<Option<CachedTokenInner>>);

impl CachedToken {
    fn clear(&self) {
        *self.0.write().unwrap() = None;
    }

    /// Returns a valid token if it exists and is not expired or if there is no expiration date.
    fn valid_token_with_buffer(&self, buffer: chrono::Duration) -> Option<SecretString> {
        let inner = self.0.read().unwrap();

        if let Some(token) = inner.as_ref() {
            if let Some(exp) = token.expiration {
                if exp - Utc::now() > buffer {
                    return Some(token.secret.clone());
                }
            } else {
                return Some(token.secret.clone());
            }
        }

        None
    }

    fn valid_token(&self) -> Option<SecretString> {
        self.valid_token_with_buffer(chrono::Duration::seconds(30))
    }

    fn set<S: Into<SecretString>>(&self, token: S, expiration: Option<DateTime<Utc>>) {
        *self.0.write().unwrap() = Some(CachedTokenInner::new(token.into(), expiration));
    }
}
```

we need an installation client to get pulls, the access token url to fetch the token, and a cached token
to update. i think we can bundle this together into a wrapper struct or just a tuple so that we can fold
over installations to get all the (sha, fetch_url) pairs we care about

```rust
let token_object =
            InstallationToken::from_response(crate::map_github_error(response).await?).await?;

let expiration = token_object
    .expires_at
    .map(|time| {
        DateTime::<Utc>::from_str(&time).map_err(|e| error::Error::Other {
            source: Box::new(e),
            backtrace: snafu::Backtrace::capture(),
        })
    })
    .transpose()?;

#[cfg(feature = "tracing")]
tracing::debug!("Token expires at: {:?}", expiration);

token.set(token_object.token.clone(), expiration);

Ok(SecretString::from(token_object.token))
```


## Cleanup Tasks

After deploys complete:

1. Scan `checkouts/` for SHAs not in active deployments
2. Remove orphaned checkout dirs
3. Scan `deploys/` for SHAs not in active deployments
4. Kill orphaned processes (if any), remove dirs
5. Release ports for killed deployments

## Error Handling

Per-PR errors shouldn't fail the entire run. Options:

1. `try_join_all` - fail fast, all or nothing
2. `join_all` with `Result` - collect successes and failures separately
3. Individual error logging, continue with successful deploys

Recommend option 2 for resilience.
