//! The browser screens. Each submodule is one screen.

mod export;
mod import;
mod recipes;
mod settings;
mod week;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};

use askama::Template;
use axum::{
    Router,
    http::{StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use tower_http::services::ServeDir;

use crate::{config::Config, db::Db};

pub use recipes::is_safe_file_name;

#[derive(Clone)]
pub struct AppState {
    inner: Arc<Inner>,
}

struct Inner {
    db: Mutex<Db>,
    /// Folder holding config.toml (re-read for AI settings on each use).
    root: PathBuf,
    data_dir: PathBuf,
    config: Config,
}

impl AppState {
    pub fn new(db: Db, root: PathBuf, data_dir: PathBuf, config: Config) -> AppState {
        AppState { inner: Arc::new(Inner { db: Mutex::new(db), root, data_dir, config }) }
    }

    pub fn db(&self) -> MutexGuard<'_, Db> {
        // A panic while holding the lock can't leave the SQLite file broken,
        // so keep going rather than refusing every later request.
        self.inner.db.lock().unwrap_or_else(|e| e.into_inner())
    }

    pub fn images_dir(&self) -> PathBuf {
        self.inner.data_dir.join("images")
    }

    pub fn root(&self) -> &PathBuf {
        &self.inner.root
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.inner.data_dir
    }

    /// Settings as they were when the app started.
    pub fn config(&self) -> &Config {
        &self.inner.config
    }

    /// config.toml read again now, so AI changes apply without a restart.
    /// (Port and data folder still need a restart.)
    pub fn fresh_config(&self) -> anyhow::Result<Config> {
        let mut c = Config::load(&self.inner.root)?;
        c.port = self.inner.config.port;
        c.data_dir = self.inner.config.data_dir.clone();
        Ok(c)
    }
}

pub fn router(state: AppState) -> Router {
    let images = ServeDir::new(state.images_dir());
    Router::new()
        .merge(recipes::routes())
        .merge(week::routes())
        .merge(import::routes())
        .merge(settings::routes())
        .merge(export::routes())
        .route("/static/style.css", get(|| async { ([(header::CONTENT_TYPE, "text/css")], include_str!("../../static/style.css")) }))
        .route("/static/app.js", get(|| async { ([(header::CONTENT_TYPE, "text/javascript")], include_str!("../../static/app.js")) }))
        .nest_service("/images", images)
        .with_state(state)
}

/// Any error shows a simple error page instead of crashing.
pub struct AppError {
    status: StatusCode,
    message: String,
}

impl AppError {
    pub fn not_found(what: &str) -> AppError {
        AppError { status: StatusCode::NOT_FOUND, message: format!("{what} not found") }
    }
    pub fn bad_request(message: impl Into<String>) -> AppError {
        AppError { status: StatusCode::BAD_REQUEST, message: message.into() }
    }
}

impl<E: Into<anyhow::Error>> From<E> for AppError {
    fn from(e: E) -> Self {
        let e: anyhow::Error = e.into();
        AppError { status: StatusCode::INTERNAL_SERVER_ERROR, message: format!("{e:#}") }
    }
}

#[derive(Template)]
#[template(path = "error.html")]
struct ErrorPage<'a> {
    nav: &'a str,
    status: u16,
    message: &'a str,
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        eprintln!("error {}: {}", self.status, self.message);
        let page = ErrorPage { nav: "", status: self.status.as_u16(), message: &self.message };
        let body = page.render().unwrap_or_else(|_| self.message.clone());
        (self.status, Html(body)).into_response()
    }
}

pub type WebResult<T> = Result<T, AppError>;

/// Render a template into an HTML response.
pub fn page(t: impl Template) -> WebResult<Html<String>> {
    Ok(Html(t.render()?))
}
