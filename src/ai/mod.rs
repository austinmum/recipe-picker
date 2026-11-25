//! The only place AI is used: reading recipe photos into a recipe.
//!
//! All three providers speak the same "chat completions" format; they only
//! differ in the web address and how they prove who you are:
//! - OpenAI:            api.openai.com, `Authorization: Bearer <OPENAI_API_KEY>`
//! - Azure (key):       your Azure resource, `api-key: <AZURE_OPENAI_API_KEY>`
//! - Azure (Entra ID):  your Azure resource, `Authorization: Bearer <token from az login>`

pub mod azure_login;
mod prompt;

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};

use crate::config::{AiConfig, ProviderKind, secret};
use crate::models::{Category, Ingredient, Recipe};
use crate::units::normalize_unit;

pub struct ImageInput {
    pub mime: String,
    pub bytes: Vec<u8>,
}

/// Where to send the request and how to sign it.
struct Target {
    url: String,
    /// (header name, header value)
    auth: (String, String),
    /// OpenAI needs the model name in the body; Azure picks it from the URL.
    model: Option<String>,
}

async fn target(ai: &AiConfig, root: &Path) -> Result<Target> {
    let azure_url = || -> Result<String> {
        let a = &ai.azure;
        if a.endpoint.trim().is_empty() || a.endpoint.contains("YOUR-RESOURCE") {
            bail!("Set `endpoint` under [ai.azure] in config.toml to your Azure OpenAI address.");
        }
        if a.deployment.trim().is_empty() || a.deployment.contains("YOUR-DEPLOYMENT") {
            bail!("Set `deployment` under [ai.azure] in config.toml to your model deployment's name.");
        }
        Ok(format!(
            "{}/openai/deployments/{}/chat/completions?api-version={}",
            a.endpoint.trim().trim_end_matches('/'),
            a.deployment.trim(),
            a.api_version.trim()
        ))
    };
    Ok(match ai.provider {
        ProviderKind::None => {
            bail!("Photo import isn't set up yet. Choose an AI provider in config.toml (see the Settings page).")
        }
        ProviderKind::Openai => {
            let key = secret(root, "OPENAI_API_KEY").context("OPENAI_API_KEY is missing. Add it to the .env file.")?;
            Target {
                url: format!("{}/chat/completions", ai.openai.base_url.trim_end_matches('/')),
                auth: ("Authorization".into(), format!("Bearer {key}")),
                model: Some(ai.openai.model.clone()),
            }
        }
        ProviderKind::AzureKey => {
            let key = secret(root, "AZURE_OPENAI_API_KEY")
                .context("AZURE_OPENAI_API_KEY is missing. Add it to the .env file.")?;
            Target {
                url: azure_url()?,
                auth: ("api-key".into(), key),
                model: None,
            }
        }
        ProviderKind::AzureEntra => {
            let url = azure_url()?;
            let token = azure_login::token(&ai.azure.tenant).await?;
            Target {
                url,
                auth: ("Authorization".into(), format!("Bearer {token}")),
                model: None,
            }
        }
    })
}

async fn chat(ai: &AiConfig, root: &Path, mut body: Value) -> Result<String> {
    let t = target(ai, root).await?;
    if let Some(model) = t.model {
        body["model"] = json!(model);
    }
    let client = reqwest::Client::builder().timeout(Duration::from_secs(180)).build()?;
    let res = client
        .post(&t.url)
        .header(t.auth.0, t.auth.1)
        .json(&body)
        .send()
        .await
        .with_context(|| {
            format!(
                "Couldn't reach {}. Check your internet connection and the address in config.toml.",
                host(&t.url)
            )
        })?;
    let status = res.status();
    let text = res.text().await?;
    if !status.is_success() {
        let detail = serde_json::from_str::<Value>(&text)
            .ok()
            .and_then(|v| v["error"]["message"].as_str().map(str::to_string))
            .unwrap_or_else(|| text.chars().take(500).collect());
        let hint = match status.as_u16() {
            401 => " (the key or login was rejected)",
            403 => " (your login doesn't have access; on Azure you need the \"Cognitive Services OpenAI User\" role)",
            404 => " (check the endpoint, deployment name or model name)",
            429 => " (rate limit or quota reached; wait a minute and try again)",
            _ => "",
        };
        bail!("The AI service said {status}{hint}: {detail}");
    }
    let v: Value = serde_json::from_str(&text).context("The AI service sent back something unexpected.")?;
    let choice = &v["choices"][0];
    if let Some(refusal) = choice["message"]["refusal"].as_str() {
        bail!("The AI declined to read this: {refusal}");
    }
    if choice["finish_reason"] == "length" {
        bail!("The AI's answer was cut off. Try fewer or smaller photos.");
    }
    choice["message"]["content"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| anyhow!("The AI service sent back an empty answer."))
}

fn host(url: &str) -> &str {
    url.split("//").nth(1).and_then(|r| r.split('/').next()).unwrap_or(url)
}

/// Read one recipe from one or more photos (e.g. front and back of a card).
pub async fn extract_recipe(ai: &AiConfig, root: &Path, images: &[ImageInput], servings: i64) -> Result<Recipe> {
    if images.is_empty() {
        bail!("Add at least one photo.");
    }
    let mut content = vec![json!({ "type": "text", "text": prompt::user_message(servings, images.len()) })];
    for img in images {
        let b64 = base64::engine::general_purpose::STANDARD.encode(&img.bytes);
        content.push(json!({ "type": "image_url", "image_url": { "url": format!("data:{};base64,{b64}", img.mime), "detail": "high" } }));
    }
    let body = json!({
        "messages": [
            { "role": "system", "content": prompt::SYSTEM },
            { "role": "user", "content": content }
        ],
        "response_format": { "type": "json_schema", "json_schema": { "name": "recipe", "strict": true, "schema": prompt::schema() } }
    });
    let answer = chat(ai, root, body).await?;
    parse_answer(&answer, servings)
}

/// A tiny request to check the connection settings work.
pub async fn test_connection(ai: &AiConfig, root: &Path) -> Result<String> {
    let body = json!({ "messages": [{ "role": "user", "content": "Reply with the single word: ok" }] });
    let answer = chat(ai, root, body).await?;
    Ok(answer.trim().chars().take(100).collect())
}

/// The shape of the AI's answer (matches `prompt::schema`).
#[derive(Deserialize)]
struct AiRecipe {
    title: String,
    description: String,
    servings: Option<i64>,
    total_minutes: Option<i64>,
    ingredients: Vec<AiIngredient>,
    steps: Vec<String>,
    notes: String,
}

#[derive(Deserialize)]
struct AiIngredient {
    original: String,
    name: String,
    quantity: Option<f64>,
    unit: String,
    category: String,
    estimated: bool,
    estimate_note: String,
}

fn parse_answer(answer: &str, servings: i64) -> Result<Recipe> {
    let r: AiRecipe =
        serde_json::from_str(answer).context("The AI's answer wasn't in the expected format. Try again.")?;
    Ok(Recipe {
        id: 0,
        title: r.title.trim().to_string(),
        description: r.description.trim().to_string(),
        servings: r.servings.filter(|s| *s > 0).unwrap_or(servings),
        total_minutes: r.total_minutes.filter(|m| *m > 0),
        source: String::new(),
        ingredients: r
            .ingredients
            .into_iter()
            .filter(|i| !i.name.trim().is_empty())
            .map(|i| Ingredient {
                original: i.original.trim().to_string(),
                name: i.name.trim().to_lowercase(),
                quantity: i.quantity.filter(|q| q.is_finite() && *q > 0.0),
                unit: normalize_unit(&i.unit),
                category: Category::parse(&i.category),
                estimated: i.estimated,
                note: i.estimate_note.trim().to_string(),
            })
            .collect(),
        steps: r
            .steps
            .into_iter()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect(),
        notes: r.notes.trim().to_string(),
        images: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{AiConfig, OpenAiConfig};
    use axum::{Json, Router, http::HeaderMap, routing::post};

    const SAMPLE: &str = r#"{
        "title": "Balsamic Fig Chicken", "description": "with roasted carrots", "servings": 2, "total_minutes": 35,
        "ingredients": [
            {"original": "1 packet Balsamic Vinegar", "name": "Balsamic Vinegar", "quantity": 2, "unit": "Tablespoons",
             "category": "pantry", "estimated": true, "estimate_note": "1 packet ≈ 2 tbsp"},
            {"original": "12 oz Carrots", "name": "carrots", "quantity": 12, "unit": "ounces",
             "category": "produce", "estimated": false, "estimate_note": ""},
            {"original": "Salt", "name": "salt", "quantity": null, "unit": "",
             "category": "spices", "estimated": false, "estimate_note": ""}
        ],
        "steps": ["Roast carrots.", " "], "notes": ""
    }"#;

    #[test]
    fn parses_ai_answer() {
        let r = parse_answer(SAMPLE, 2).unwrap();
        assert_eq!(r.title, "Balsamic Fig Chicken");
        assert_eq!(r.ingredients[0].name, "balsamic vinegar");
        assert_eq!(r.ingredients[0].unit, "tbsp");
        assert!(r.ingredients[0].estimated);
        assert_eq!(r.ingredients[1].unit, "oz");
        assert_eq!(r.ingredients[1].category, Category::Produce);
        assert_eq!(r.ingredients[2].quantity, None);
        assert_eq!(r.steps, vec!["Roast carrots."]);
    }

    #[test]
    fn schema_is_valid_strict_json_schema() {
        // Strict mode requires every property to be listed as required.
        fn check(v: &Value) {
            if let Some(props) = v.get("properties").and_then(Value::as_object) {
                let required: Vec<&str> = v["required"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|r| r.as_str().unwrap())
                    .collect();
                for (k, child) in props {
                    assert!(required.contains(&k.as_str()), "{k} must be required");
                    check(child);
                }
                assert_eq!(v["additionalProperties"], json!(false));
            }
            if let Some(items) = v.get("items") {
                check(items);
            }
        }
        check(&prompt::schema());
    }

    /// Runs a stand-in "OpenAI" on a local port and does a full import through it.
    #[tokio::test]
    async fn full_request_against_stand_in_ai() {
        let app = Router::new().route(
            "/v1/chat/completions",
            post(|headers: HeaderMap, Json(body): Json<Value>| async move {
                assert_eq!(headers["authorization"], "Bearer test-key");
                assert_eq!(body["model"], "test-model");
                assert_eq!(body["response_format"]["json_schema"]["strict"], true);
                let content = body["messages"][1]["content"].as_array().unwrap();
                assert!(
                    content[1]["image_url"]["url"]
                        .as_str()
                        .unwrap()
                        .starts_with("data:image/jpeg;base64,")
                );
                Json(json!({ "choices": [{ "finish_reason": "stop", "message": { "content": SAMPLE } }] }))
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join(".env"), "OPENAI_API_KEY=test-key\n").unwrap();
        let ai = AiConfig {
            provider: ProviderKind::Openai,
            openai: OpenAiConfig {
                model: "test-model".into(),
                base_url: format!("http://{addr}/v1"),
            },
            ..AiConfig::default()
        };
        let img = ImageInput {
            mime: "image/jpeg".into(),
            bytes: vec![1, 2, 3],
        };
        let r = extract_recipe(&ai, dir.path(), &[img], 2).await.unwrap();
        assert_eq!(r.ingredients.len(), 3);
    }

    #[tokio::test]
    async fn explains_missing_setup() {
        let dir = tempfile::tempdir().unwrap();
        let err = test_connection(&AiConfig::default(), dir.path()).await.unwrap_err();
        assert!(err.to_string().contains("isn't set up"));
        let azure = AiConfig {
            provider: ProviderKind::AzureKey,
            ..AiConfig::default()
        };
        let err = test_connection(&azure, dir.path()).await.unwrap_err();
        assert!(
            format!("{err:#}").contains("endpoint") || format!("{err:#}").contains("AZURE_OPENAI_API_KEY"),
            "{err:#}"
        );
    }
}
