//! Settings from `config.toml` (all optional) plus secrets from the
//! environment / `.env`. The AI part is re-read on every use, so switching
//! provider only needs a file edit, not a restart.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

pub const CONFIG_FILE: &str = "config.toml";

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    pub data_dir: PathBuf,
    /// Which column of a HelloFresh card (2 or 4 people) to read amounts from.
    pub default_servings: i64,
    pub ai: AiConfig,
}

impl Default for Config {
    fn default() -> Self {
        Config { port: 3000, data_dir: PathBuf::from("data"), default_servings: 2, ai: AiConfig::default() }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    #[default]
    None,
    /// OpenAI with an API key (OPENAI_API_KEY).
    Openai,
    /// Azure OpenAI with an API key (AZURE_OPENAI_API_KEY).
    AzureKey,
    /// Azure OpenAI with your Azure login (Entra ID / RBAC), via `az login`.
    AzureEntra,
}

impl ProviderKind {
    pub fn label(self) -> &'static str {
        match self {
            ProviderKind::None => "Not set up",
            ProviderKind::Openai => "OpenAI (API key)",
            ProviderKind::AzureKey => "Azure OpenAI (API key)",
            ProviderKind::AzureEntra => "Azure OpenAI (Azure login / Entra ID)",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
pub struct AiConfig {
    pub provider: ProviderKind,
    pub openai: OpenAiConfig,
    pub azure: AzureConfig,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct OpenAiConfig {
    pub model: String,
    pub base_url: String,
}

impl Default for OpenAiConfig {
    fn default() -> Self {
        OpenAiConfig { model: "gpt-4.1".into(), base_url: "https://api.openai.com/v1".into() }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct AzureConfig {
    /// e.g. https://my-resource.openai.azure.com
    pub endpoint: String,
    /// The deployment name you gave the model in Azure.
    pub deployment: String,
    pub api_version: String,
    /// Optional: Azure tenant to get the login token from (azure_entra only).
    pub tenant: String,
}

impl Default for AzureConfig {
    fn default() -> Self {
        AzureConfig { endpoint: String::new(), deployment: String::new(), api_version: "2024-10-21".into(), tenant: String::new() }
    }
}

impl Config {
    /// Load `config.toml` from `dir` if it exists; otherwise use defaults.
    pub fn load(dir: &Path) -> Result<Config> {
        let path = dir.join(CONFIG_FILE);
        if !path.exists() {
            return Ok(Config::default());
        }
        let text = std::fs::read_to_string(&path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("{} has a mistake in it", path.display()))
    }
}

pub fn env_var(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

/// An API key: from the `.env` file next to config.toml first, then the
/// environment. The file wins so that a key set in your shell for something
/// else (e.g. work) is never picked up by accident. The file is re-read each
/// time, so a new key works without restarting.
pub fn secret(root: &Path, name: &str) -> Option<String> {
    if let Ok(iter) = dotenvy::from_path_iter(root.join(".env")) {
        for (k, v) in iter.flatten() {
            if k == name && !v.trim().is_empty() {
                return Some(v.trim().to_string());
            }
        }
    }
    env_var(name)
}

/// Is the key available, and from where? (for the Settings page)
pub fn secret_source(root: &Path, name: &str) -> Option<&'static str> {
    let in_file = dotenvy::from_path_iter(root.join(".env"))
        .map(|it| it.flatten().any(|(k, v)| k == name && !v.trim().is_empty()))
        .unwrap_or(false);
    if in_file {
        Some(".env file")
    } else if env_var(name).is_some() {
        Some("environment variable")
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_example_config() {
        let text = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/config.example.toml")).unwrap();
        let c: Config = toml::from_str(&text).unwrap();
        assert_eq!(c.port, 3000);
        assert_eq!(c.ai.provider, ProviderKind::Openai);
    }

    #[test]
    fn empty_config_uses_defaults() {
        let c: Config = toml::from_str("").unwrap();
        assert_eq!(c.ai.provider, ProviderKind::None);
        assert_eq!(c.default_servings, 2);
    }
}
