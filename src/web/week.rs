//! "This week" (pick recipes + trips) and the shopping list.

use std::collections::HashMap;

use askama::Template;
use axum::{
    Router,
    extract::{Path, State},
    http::{StatusCode, header},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;

use super::{AppError, AppState, WebResult, page};
use crate::{
    db::CheckField,
    models::{Category, Pick, Recipe, Trip},
    shopping::{self, TripList},
    units::name_key,
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/week", get(week_page).post(save_week))
        .route("/week/new", post(new_week))
        .route("/list", get(list_page))
        .route("/list.txt", get(list_text))
        .route("/list/check", post(check))
        .route("/list/extra", post(add_extra))
        .route("/list/extra/{id}/delete", post(delete_extra))
}

// ---------- pick recipes ----------

struct PickRow {
    id: i64,
    title: String,
    thumbnail: Option<String>,
    search: String,
    /// 0 = not this week, 1 or 2 = trip.
    trip: i64,
    /// Perishable produce, "|"-separated, for the "Recommended" badge.
    produce: String,
    /// Meat and seafood, "|"-separated.
    protein: String,
}

/// Produce that keeps for weeks, so sharing it isn't worth a recommendation.
const KEEPS: [&str; 2] = ["garlic", "ginger"];

/// The recipe's ingredient names in one aisle, matched loosely ("Carrots" = "carrot").
fn names_in(r: &Recipe, category: Category) -> String {
    let mut names: Vec<String> = r
        .ingredients
        .iter()
        .filter(|i| i.category == category)
        .map(|i| name_key(&i.name))
        .filter(|n| !n.is_empty() && !KEEPS.contains(&n.as_str()))
        .collect();
    names.sort();
    names.dedup();
    names.join("|")
}

#[derive(Template)]
#[template(path = "week.html")]
struct WeekPage {
    nav: &'static str,
    rows: Vec<PickRow>,
}

async fn week_page(State(state): State<AppState>) -> WebResult<Html<String>> {
    let db = state.db();
    let picks: HashMap<i64, Trip> = db.picks()?.into_iter().map(|p| (p.recipe_id, p.trip)).collect();
    let mut rows: Vec<PickRow> = db
        .all_recipes()?
        .into_iter()
        .map(|r| PickRow {
            id: r.id,
            search: format!(
                "{} {}",
                r.title,
                r.ingredients
                    .iter()
                    .map(|i| i.name.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            )
            .to_lowercase(),
            thumbnail: r.images.first().cloned(),
            produce: names_in(&r, Category::Produce),
            protein: names_in(&r, Category::MeatSeafood),
            trip: picks.get(&r.id).map(|t| t.num()).unwrap_or(0),
            title: r.title,
        })
        .collect();
    // Picked recipes first, so you can see this week's choices at a glance.
    rows.sort_by_key(|r| (r.trip == 0, r.title.to_lowercase()));
    page(WeekPage { nav: "week", rows })
}

/// Form fields are `pick_<recipe id>` = 0 / 1 / 2.
async fn save_week(State(state): State<AppState>, body: String) -> WebResult<Redirect> {
    let fields: Vec<(String, String)> =
        serde_html_form::from_str(&body).map_err(|e| AppError::bad_request(e.to_string()))?;
    let picks: Vec<Pick> = fields
        .into_iter()
        .filter_map(|(k, v)| {
            let recipe_id = k.strip_prefix("pick_")?.parse().ok()?;
            let trip = Trip::from_i64(v.parse().ok()?)?;
            Some(Pick { recipe_id, trip })
        })
        .collect();
    state.db().set_picks(&picks)?;
    Ok(Redirect::to("/list"))
}

async fn new_week(State(state): State<AppState>) -> WebResult<Redirect> {
    state.db().new_week()?;
    Ok(Redirect::to("/week"))
}

// ---------- shopping list ----------

fn current_list(state: &AppState) -> WebResult<Vec<TripList>> {
    let db = state.db();
    let picked: Vec<(Recipe, Trip)> = db
        .picks()?
        .into_iter()
        .filter_map(|p| db.get_recipe(p.recipe_id).transpose().map(|r| r.map(|r| (r, p.trip))))
        .collect::<Result<_, _>>()?;
    Ok(shopping::build(&picked, &db.extra_items()?, &db.checks()?))
}

#[derive(Template)]
#[template(path = "list.html")]
struct ListPage {
    nav: &'static str,
    trips: Vec<TripList>,
    picked_count: usize,
}

async fn list_page(State(state): State<AppState>) -> WebResult<Html<String>> {
    let trips = current_list(&state)?;
    let picked_count = trips.iter().map(|t| t.recipes.len()).sum();
    page(ListPage {
        nav: "list",
        trips,
        picked_count,
    })
}

async fn list_text(State(state): State<AppState>) -> WebResult<impl IntoResponse> {
    let text = shopping::as_text(&current_list(&state)?);
    Ok(([(header::CONTENT_TYPE, "text/plain; charset=utf-8")], text))
}

#[derive(Deserialize)]
struct CheckForm {
    key: String,
    field: String,
    value: String,
    #[serde(default)]
    amount: String,
}

async fn check(State(state): State<AppState>, body: String) -> WebResult<StatusCode> {
    let f: CheckForm = serde_html_form::from_str(&body).map_err(|e| AppError::bad_request(e.to_string()))?;
    let field = match f.field.as_str() {
        "have" => CheckField::Have,
        "got" => CheckField::Got,
        _ => return Err(AppError::bad_request("unknown field")),
    };
    state.db().set_check(&f.key, field, f.value == "1", &f.amount)?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
struct ExtraForm {
    name: String,
    #[serde(default)]
    amount: String,
    trip: i64,
}

async fn add_extra(State(state): State<AppState>, body: String) -> WebResult<Redirect> {
    let f: ExtraForm = serde_html_form::from_str(&body).map_err(|e| AppError::bad_request(e.to_string()))?;
    let trip = Trip::from_i64(f.trip).ok_or_else(|| AppError::bad_request("trip must be 1 or 2"))?;
    if !f.name.trim().is_empty() {
        state.db().add_extra_item(f.name.trim(), f.amount.trim(), trip)?;
    }
    Ok(Redirect::to(&format!("/list#trip{}", trip.num())))
}

async fn delete_extra(State(state): State<AppState>, Path(id): Path<i64>) -> WebResult<Redirect> {
    state.db().delete_extra_item(id)?;
    Ok(Redirect::to("/list"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::Ingredient;

    #[test]
    fn produce_names_skip_long_keepers() {
        let mut r = Recipe::empty(2);
        for (name, cat) in [
            ("Carrots", Category::Produce),
            ("Garlic", Category::Produce),
            ("carrot", Category::Produce),
            ("Chicken Thighs", Category::MeatSeafood),
        ] {
            r.ingredients.push(Ingredient {
                original: name.into(),
                name: name.into(),
                quantity: None,
                unit: String::new(),
                category: cat,
                estimated: false,
                note: String::new(),
            });
        }
        assert_eq!(names_in(&r, Category::Produce), "carrot");
        assert_eq!(names_in(&r, Category::MeatSeafood), "chicken thigh");
    }
}
