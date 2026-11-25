//! Settings: shows which AI connection is active and can test it.

use askama::Template;
use axum::{Router, extract::State, response::Html, routing::get};

use super::{AppState, WebResult, page};
use crate::{
    ai,
    config::{CONFIG_FILE, ProviderKind, secret_source},
};

pub fn routes() -> Router<AppState> {
    Router::new().route("/settings", get(show).post(test))
}

#[derive(Template)]
#[template(path = "settings.html")]
struct SettingsPage {
    nav: &'static str,
    provider: &'static str,
    kind: &'static str,
    details: Vec<(&'static str, String)>,
    config_found: bool,
    config_path: String,
    data_dir: String,
    test_ok: Option<bool>,
    test_message: String,
}

async fn build(state: &AppState) -> WebResult<SettingsPage> {
    let config = state.fresh_config()?;
    let ai = &config.ai;
    let root = state.root();
    let key_status = |name: &str| match secret_source(root, name) {
        Some(src) => format!("found ({src})"),
        None => format!("missing: add {name}=… to the .env file"),
    };
    let details: Vec<(&'static str, String)> = match ai.provider {
        ProviderKind::None => vec![],
        ProviderKind::Openai => vec![
            ("Model", ai.openai.model.clone()),
            ("API key", key_status("OPENAI_API_KEY")),
        ],
        ProviderKind::AzureKey | ProviderKind::AzureEntra => {
            let mut d = vec![
                ("Endpoint", ai.azure.endpoint.clone()),
                ("Deployment", ai.azure.deployment.clone()),
                ("API version", ai.azure.api_version.clone()),
            ];
            if ai.provider == ProviderKind::AzureKey {
                d.push(("API key", key_status("AZURE_OPENAI_API_KEY")));
            } else {
                let who = ai::azure_login::signed_in_account()
                    .await
                    .unwrap_or_else(|| "not signed in: run `az login`".into());
                d.push(("Azure login", who));
                if !ai.azure.tenant.is_empty() {
                    d.push(("Tenant", ai.azure.tenant.clone()));
                }
            }
            d
        }
    };
    let kind = match ai.provider {
        ProviderKind::None => "none",
        ProviderKind::Openai => "openai",
        ProviderKind::AzureKey => "azure_key",
        ProviderKind::AzureEntra => "azure_entra",
    };
    Ok(SettingsPage {
        nav: "settings",
        provider: ai.provider.label(),
        kind,
        details,
        config_found: root.join(CONFIG_FILE).exists(),
        config_path: root.join(CONFIG_FILE).display().to_string(),
        data_dir: state.data_dir().display().to_string(),
        test_ok: None,
        test_message: String::new(),
    })
}

async fn show(State(state): State<AppState>) -> WebResult<Html<String>> {
    page(build(&state).await?)
}

async fn test(State(state): State<AppState>) -> WebResult<Html<String>> {
    let mut p = build(&state).await?;
    let config = state.fresh_config()?;
    match ai::test_connection(&config.ai, state.root()).await {
        Ok(answer) => {
            p.test_ok = Some(true);
            p.test_message = format!("Connected. The AI replied: “{answer}”");
        }
        Err(e) => {
            p.test_ok = Some(false);
            p.test_message = format!("{e:#}");
        }
    }
    page(p)
}
