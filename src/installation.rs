use anyhow::Context;
use chrono::{DateTime, Utc};
use octocrab::{Octocrab, models::InstallationToken};
use secrecy::{ExposeSecret, SecretString};
use std::str::FromStr;
use tracing::debug;

use crate::{FetchUrl, Repo, Sha, cached_token::CachedToken};

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

    /// Get (Sha, FetchUrl) pairs for open PRs in this installation's repo.
    pub async fn fetch_deploy_targets(
        &self,
        app_client: &Octocrab,
    ) -> anyhow::Result<Vec<(Sha, FetchUrl)>> {
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
            .map(|pull| (Sha::new(pull.head.sha), fetch_url.clone()))
            .collect())
    }
}
