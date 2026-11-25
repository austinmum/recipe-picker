//! Export (download a backup) and import (restore one).

use askama::Template;
use axum::{
    Router,
    extract::{DefaultBodyLimit, Multipart, State},
    http::header,
    response::{Html, IntoResponse},
    routing::{get, post},
};

use super::{AppError, AppState, WebResult, page};
use crate::export;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/export", get(export_page))
        .route("/export/download", get(download))
        .route("/export/import", post(import).layer(DefaultBodyLimit::max(1024 * 1024 * 1024)))
}

#[derive(Template)]
#[template(path = "export.html")]
struct ExportPage {
    nav: &'static str,
    count: usize,
    message: Option<String>,
    error: Option<String>,
}

async fn export_page(State(state): State<AppState>) -> WebResult<Html<String>> {
    let count = state.db().list_recipes("")?.len();
    page(ExportPage { nav: "export", count, message: None, error: None })
}

async fn download(State(state): State<AppState>) -> WebResult<impl IntoResponse> {
    let (bytes, date) = {
        let db = state.db();
        (export::export_zip(&db, &state.images_dir())?, db.today()?)
    };
    let disposition = format!("attachment; filename=\"recipes-{date}.zip\"");
    Ok(([(header::CONTENT_TYPE, "application/zip".to_string()), (header::CONTENT_DISPOSITION, disposition)], bytes))
}

async fn import(State(state): State<AppState>, mut multipart: Multipart) -> WebResult<Html<String>> {
    let mut bytes = None;
    while let Some(field) = multipart.next_field().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))? {
        if field.name() == Some("file") {
            bytes = Some(field.bytes().await.map_err(|e| AppError::bad_request(format!("Upload failed: {e}")))?);
        }
    }
    let mut p = ExportPage { nav: "export", count: 0, message: None, error: None };
    match bytes.filter(|b| !b.is_empty()) {
        None => p.error = Some("Choose a file to import.".into()),
        Some(b) => match export::import(&mut state.db(), &state.images_dir(), &b) {
            Ok(s) => {
                let mut msg = format!("Imported {} recipe{}.", s.added, if s.added == 1 { "" } else { "s" });
                if !s.skipped.is_empty() {
                    msg += &format!(" Skipped {} you already have: {}.", s.skipped.len(), s.skipped.join(", "));
                }
                p.message = Some(msg);
            }
            Err(e) => p.error = Some(format!("{e:#}")),
        },
    }
    p.count = state.db().list_recipes("")?.len();
    page(p)
}
