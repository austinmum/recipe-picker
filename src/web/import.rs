//! "Add from photo": upload photos, let the AI read them, then review.

use askama::Template;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    response::Html,
    routing::get,
};

use super::{AppState, WebResult, page, recipes::FormPage};
use crate::{ai, config::Config, images};

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/import",
        get(import_page)
            .post(upload)
            .layer(DefaultBodyLimit::max(80 * 1024 * 1024)),
    )
}

#[derive(Template)]
#[template(path = "import.html")]
struct ImportPage {
    nav: &'static str,
    provider: &'static str,
    ready: bool,
    servings: i64,
    error: Option<String>,
    /// Photos already saved when reading failed (so you can type it in instead).
    kept: Vec<String>,
}

impl ImportPage {
    fn new(config: &Config) -> ImportPage {
        ImportPage {
            nav: "import",
            provider: config.ai.provider.label(),
            ready: config.ai.provider != crate::config::ProviderKind::None,
            servings: config.default_servings,
            error: None,
            kept: Vec::new(),
        }
    }

    fn kept_query(&self) -> String {
        self.kept
            .iter()
            .map(|k| format!("images={k}"))
            .collect::<Vec<_>>()
            .join("&")
    }
}

async fn import_page(State(state): State<AppState>) -> WebResult<Html<String>> {
    page(ImportPage::new(&state.fresh_config()?))
}

async fn upload(State(state): State<AppState>, mut multipart: Multipart) -> WebResult<Html<String>> {
    let config = state.fresh_config()?;
    let mut servings = config.default_servings;
    let mut stored = Vec::new();
    let mut problem = None;

    while let Some(field) = multipart
        .next_field()
        .await
        .map_err(|e| super::AppError::bad_request(format!("Upload failed: {e}")))?
    {
        match field.name() {
            Some("servings") => {
                servings = field
                    .text()
                    .await
                    .ok()
                    .and_then(|t| t.trim().parse().ok())
                    .unwrap_or(servings)
            }
            Some("photos") => {
                let file_name = field.file_name().unwrap_or("photo").to_string();
                let content_type = field.content_type().unwrap_or("").to_string();
                let bytes = field
                    .bytes()
                    .await
                    .map_err(|e| super::AppError::bad_request(format!("Upload failed: {e}")))?;
                if bytes.is_empty() {
                    continue;
                }
                match images::store(&state.images_dir(), &file_name, &content_type, &bytes).await {
                    Ok(s) => stored.push(s),
                    Err(e) => problem = Some(format!("{e:#}")),
                }
            }
            _ => {}
        }
    }

    let mut p = ImportPage::new(&config);
    p.servings = servings;
    p.kept = stored.iter().map(|s| s.name.clone()).collect();
    if let Some(problem) = problem {
        p.error = Some(problem);
        return page(p);
    }
    if stored.is_empty() {
        p.error = Some("Choose at least one photo.".into());
        return page(p);
    }

    let inputs: Vec<ai::ImageInput> = stored
        .iter()
        .map(|s| ai::ImageInput {
            mime: s.mime.clone(),
            bytes: s.bytes.clone(),
        })
        .collect();
    match ai::extract_recipe(&config.ai, state.root(), &inputs, servings).await {
        Ok(mut recipe) => {
            recipe.images = p.kept;
            let intro = "Here's what the AI read. Check it against the photos at the bottom, fix anything that looks off, then save.";
            page(FormPage::new("import", "Check the recipe", intro, recipe))
        }
        Err(e) => {
            p.error = Some(format!("{e:#}"));
            page(p)
        }
    }
}
