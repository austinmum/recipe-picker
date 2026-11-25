//! Gets an Entra ID (Azure AD) token from the Azure CLI's existing login,
//! so Azure OpenAI can be used with RBAC permissions instead of a key.
//! Tokens last about an hour; we reuse one until 5 minutes before it expires.

use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

const RESOURCE: &str = "https://cognitiveservices.azure.com";

struct Cached {
    tenant: String,
    token: String,
    expires_at: u64,
}

static CACHE: Mutex<Option<Cached>> = Mutex::new(None);

#[derive(Deserialize)]
struct AzToken {
    #[serde(rename = "accessToken")]
    access_token: String,
    /// Seconds since 1970 (newer Azure CLI versions).
    expires_on: Option<u64>,
}

fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

pub async fn token(tenant: &str) -> Result<String> {
    let tenant = tenant.trim().to_string();
    if let Some(c) = CACHE.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if c.tenant == tenant && c.expires_at > now() + 300 {
            return Ok(c.token.clone());
        }
    }

    let mut cmd = tokio::process::Command::new("az");
    cmd.args(["account", "get-access-token", "--resource", RESOURCE, "--output", "json"]);
    if !tenant.is_empty() {
        cmd.args(["--tenant", &tenant]);
    }
    let out = cmd
        .output()
        .await
        .context("Couldn't run the Azure CLI (`az`). Install it, or switch to a key-based provider in config.toml.")?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr);
        bail!("Couldn't get an Azure login token. Run `az login` in a terminal{} and try again.\n\nAzure CLI said: {}",
            if tenant.is_empty() { String::new() } else { format!(" (with --tenant {tenant})") },
            err.trim());
    }
    let t: AzToken = serde_json::from_slice(&out.stdout).context("Unexpected answer from `az account get-access-token`.")?;
    let expires_at = t.expires_on.unwrap_or(now() + 30 * 60);
    *CACHE.lock().unwrap_or_else(|e| e.into_inner()) = Some(Cached { tenant, token: t.access_token.clone(), expires_at });
    Ok(t.access_token)
}

/// For the Settings page: who is `az` logged in as (if anyone)?
pub async fn signed_in_account() -> Option<String> {
    let out = tokio::process::Command::new("az").args(["account", "show", "--query", "user.name", "--output", "tsv"]).output().await.ok()?;
    out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim().to_string()).filter(|s| !s.is_empty())
}
