use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, models::InstallationToken};
use secrecy::{ExposeSecret, SecretString};
use std::str::FromStr;
use tracing::debug;

use crate::{FetchUrl, Repo, Sha, cached_token::CachedToken};

/// A PR deployment target with all info needed to deploy and comment.
#[derive(Debug, Clone)]
pub struct DeployTarget {
    pub sha: Sha,
    pub fetch_url: FetchUrl,
    pub pr_number: u64,
}

/// Wraps an installation client with token caching and repo access.
pub struct Installation {
    pub client: Octocrab,
    access_tokens_url: String,
    pub repo: Repo,
    token: CachedToken,
}

impl Installation {
    pub fn new(client: Octocrab, access_tokens_url: String, repo: Repo) -> Self {
        Self {
            client,
            access_tokens_url,
            repo,
            token: CachedToken::default(),
        }
    }

    /// Ensures we have a valid token, refreshing via app client if needed.
    pub async fn ensure_token(&self, app_client: &Octocrab) -> anyhow::Result<SecretString> {
        if let Some(token) = self.token.valid_token() {
            debug!("using cached installation token");
            return Ok(token);
        }

        debug!("refreshing installation token");
        let token_object: InstallationToken = app_client
            .post(&self.access_tokens_url, None::<&()>)
            .await
            .context("failed to fetch installation token")?;

        let expiration = token_object
            .expires_at
            .as_ref()
            .map(|time| DateTime::<Utc>::from_str(time))
            .transpose()
            .context("failed to parse token expiration")?;

        debug!(expires_at = ?expiration, "fetched installation token");

        let secret = SecretString::from(token_object.token);
        self.token.set(secret.clone(), expiration);

        Ok(secret)
    }

    /// Get deploy targets for open PRs in this installation's repo.
    pub async fn fetch_deploy_targets(
        &self,
        app_client: &Octocrab,
    ) -> anyhow::Result<Vec<DeployTarget>> {
        let token = self.ensure_token(app_client).await?;
        let fetch_url = FetchUrl::new(format!(
            "https://x-access-token:{}@github.com/{}.git",
            token.expose_secret(),
            self.repo
        ));

        let pulls = self
            .client
            .pulls(&self.repo.owner, &self.repo.name)
            .list()
            .state(octocrab::params::State::Open)
            .send()
            .await
            .context(format!("failed to fetch PRs for {}", self.repo))?;

        Ok(pulls
            .items
            .into_iter()
            .map(|pull| DeployTarget {
                sha: Sha::new(pull.head.sha),
                fetch_url: fetch_url.clone(),
                pr_number: pull.number,
            })
            .collect())
    }

    /// Get the HEAD SHA of a branch.
    pub async fn fetch_branch_sha(&self, branch: &str) -> anyhow::Result<Sha> {
        let branch_info = self
            .client
            .repos(&self.repo.owner, &self.repo.name)
            .get_ref(&octocrab::params::repos::Reference::Branch(branch.to_string()))
            .await
            .with_context(|| format!("failed to fetch branch {}", branch))?;

        let sha = match branch_info.object {
            octocrab::models::repos::Object::Commit { sha, .. } => sha,
            octocrab::models::repos::Object::Tag { sha, .. } => sha,
            _ => bail!("unexpected ref object type for branch {}", branch),
        };

        Ok(Sha::new(sha))
    }

    /// Create or update the deploy preview comment on a PR.
    pub async fn upsert_deploy_comment(&self, pr_number: u64, url: &str) -> anyhow::Result<()> {
        const MARKER: &str = "<!-- hazel-deploy -->";
        let body = format!("{MARKER}\n🚀 Preview: {url}");

        let comments = self
            .client
            .issues(&self.repo.owner, &self.repo.name)
            .list_comments(pr_number)
            .send()
            .await
            .context("failed to list PR comments")?;

        let existing = comments
            .items
            .iter()
            .find(|c| c.body.as_ref().is_some_and(|b| b.contains(MARKER)));

        if let Some(comment) = existing {
            self.client
                .issues(&self.repo.owner, &self.repo.name)
                .update_comment(comment.id, body)
                .await
                .context("failed to update deploy comment")?;
            debug!(pr = pr_number, "updated deploy comment");
        } else {
            self.client
                .issues(&self.repo.owner, &self.repo.name)
                .create_comment(pr_number, body)
                .await
                .context("failed to create deploy comment")?;
            debug!(pr = pr_number, "created deploy comment");
        }

        Ok(())
    }
}
