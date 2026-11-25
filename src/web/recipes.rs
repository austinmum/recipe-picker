//! Recipe list, recipe page, and the add/edit form (also used to review
//! a recipe the AI read from a photo).

use askama::Template;
use axum::{
    Router,
    extract::{Path, Query, RawQuery, State},
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
};
use serde::Deserialize;

use super::{AppError, AppState, WebResult, page};
use crate::{
    db::RecipeSummary,
    models::{Category, Ingredient, Recipe},
    units::{format_number, normalize_unit, parse_quantity},
};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/", get(list))
        .route("/recipes/new", get(new_form))
        .route("/recipes/save", post(save))
        .route("/recipes/{id}", get(show))
        .route("/recipes/{id}/edit", get(edit_form))
        .route("/recipes/{id}/delete", post(delete))
}

// ---------- list ----------

#[derive(Deserialize)]
struct ListQuery {
    #[serde(default)]
    q: String,
}

#[derive(Template)]
#[template(path = "recipes.html")]
struct ListPage {
    nav: &'static str,
    q: String,
    recipes: Vec<RecipeSummary>,
    total: usize,
}

async fn list(State(state): State<AppState>, Query(query): Query<ListQuery>) -> WebResult<Html<String>> {
    let db = state.db();
    let recipes = db.list_recipes(&query.q)?;
    let total = if query.q.trim().is_empty() { recipes.len() } else { db.list_recipes("")?.len() };
    page(ListPage { nav: "recipes", q: query.q, recipes, total })
}

// ---------- show ----------

pub struct IngredientLine {
    pub amount: String,
    pub name: String,
    pub estimated: bool,
    pub original: String,
    pub note: String,
}

impl IngredientLine {
    pub fn from(i: &Ingredient) -> IngredientLine {
        let amount = match i.quantity {
            Some(q) => format!("{} {}", format_number(q), i.unit).trim().to_string(),
            None => String::new(),
        };
        IngredientLine { amount, name: i.name.clone(), estimated: i.estimated, original: i.original.clone(), note: i.note.clone() }
    }
}

#[derive(Template)]
#[template(path = "recipe.html")]
struct ShowPage {
    nav: &'static str,
    recipe: Recipe,
    lines: Vec<IngredientLine>,
}

async fn show(State(state): State<AppState>, Path(id): Path<i64>) -> WebResult<Html<String>> {
    let recipe = state.db().get_recipe(id)?.ok_or_else(|| AppError::not_found("Recipe"))?;
    let lines = recipe.ingredients.iter().map(IngredientLine::from).collect();
    page(ShowPage { nav: "recipes", recipe, lines })
}

async fn delete(State(state): State<AppState>, Path(id): Path<i64>) -> WebResult<Redirect> {
    state.db().delete_recipe(id)?;
    Ok(Redirect::to("/"))
}

// ---------- add / edit / review form ----------

pub struct CatOpt {
    pub value: &'static str,
    pub label: &'static str,
}

pub fn category_options() -> Vec<CatOpt> {
    Category::ALL.iter().map(|c| CatOpt { value: c.as_str(), label: c.label() }).collect()
}

pub struct IngRow {
    pub original: String,
    pub quantity: String,
    pub unit: String,
    pub name: String,
    pub category: &'static str,
    pub estimated: bool,
    pub note: String,
}

impl IngRow {
    fn blank() -> IngRow {
        IngRow {
            original: String::new(),
            quantity: String::new(),
            unit: String::new(),
            name: String::new(),
            category: Category::Other.as_str(),
            estimated: false,
            note: String::new(),
        }
    }
}

#[derive(Template)]
#[template(path = "recipe_form.html")]
pub struct FormPage {
    pub nav: &'static str,
    pub heading: String,
    /// Shown above the form, e.g. "Check what the AI read".
    pub intro: String,
    pub recipe: Recipe,
    pub rows: Vec<IngRow>,
    pub blank: Vec<IngRow>,
    pub categories: Vec<CatOpt>,
    pub steps_text: String,
    pub minutes_text: String,
    pub estimated_count: usize,
}

impl FormPage {
    pub fn new(nav: &'static str, heading: &str, intro: &str, recipe: Recipe) -> FormPage {
        let mut rows: Vec<IngRow> = recipe
            .ingredients
            .iter()
            .map(|i| IngRow {
                original: i.original.clone(),
                quantity: i.quantity.map(format_number).unwrap_or_default(),
                unit: i.unit.clone(),
                name: i.name.clone(),
                category: i.category.as_str(),
                estimated: i.estimated,
                note: i.note.clone(),
            })
            .collect();
        if rows.is_empty() {
            rows.extend((0..3).map(|_| IngRow::blank()));
        }
        FormPage {
            nav,
            heading: heading.into(),
            intro: intro.into(),
            steps_text: recipe.steps.join("\n\n"),
            minutes_text: recipe.total_minutes.map(|m| m.to_string()).unwrap_or_default(),
            estimated_count: recipe.ingredients.iter().filter(|i| i.estimated).count(),
            recipe,
            rows,
            blank: vec![IngRow::blank()],
            categories: category_options(),
        }
    }
}

/// `?images=a.jpg&images=b.jpg` attaches photos already saved (used when
/// the AI couldn't read them and you type the recipe in instead).
async fn new_form(State(state): State<AppState>, RawQuery(query): RawQuery) -> WebResult<Html<String>> {
    let mut recipe = Recipe::empty(state.config().default_servings);
    #[derive(Deserialize, Default)]
    struct Q {
        #[serde(default)]
        images: Vec<String>,
    }
    let q: Q = serde_html_form::from_str(query.as_deref().unwrap_or("")).unwrap_or_default();
    recipe.images = q.images.into_iter().filter(|i| is_safe_file_name(i) && state.images_dir().join(i).exists()).collect();
    page(FormPage::new("manual", "Add a recipe", "", recipe))
}

async fn edit_form(State(state): State<AppState>, Path(id): Path<i64>) -> WebResult<Html<String>> {
    let recipe = state.db().get_recipe(id)?.ok_or_else(|| AppError::not_found("Recipe"))?;
    page(FormPage::new("recipes", "Edit recipe", "", recipe))
}

/// The form as submitted. Ingredient columns arrive as parallel lists.
#[derive(Deserialize, Default)]
#[serde(default)]
pub struct RecipeForm {
    id: i64,
    title: String,
    description: String,
    servings: String,
    total_minutes: String,
    source: String,
    notes: String,
    steps: String,
    images: Vec<String>,
    ing_original: Vec<String>,
    ing_quantity: Vec<String>,
    ing_unit: Vec<String>,
    ing_name: Vec<String>,
    ing_category: Vec<String>,
    ing_estimated: Vec<String>,
    ing_note: Vec<String>,
}

impl RecipeForm {
    pub fn into_recipe(self) -> Result<Recipe, String> {
        let title = self.title.trim().to_string();
        if title.is_empty() {
            return Err("The recipe needs a title.".into());
        }
        let get = |v: &Vec<String>, i: usize| v.get(i).map(|s| s.trim().to_string()).unwrap_or_default();
        let mut ingredients = Vec::new();
        for i in 0..self.ing_name.len().max(self.ing_original.len()) {
            let original = get(&self.ing_original, i);
            let mut name = get(&self.ing_name, i);
            let qty_text = get(&self.ing_quantity, i);
            if name.is_empty() && original.is_empty() {
                continue; // empty row
            }
            if name.is_empty() {
                name = original.clone();
            }
            let quantity = parse_quantity(&qty_text);
            if quantity.is_none() && !qty_text.is_empty() {
                return Err(format!("Couldn't read the amount \"{qty_text}\" for {name}. Use numbers like 2, 1.5, 1/2 or 1 1/2."));
            }
            ingredients.push(Ingredient {
                original: if original.is_empty() { name.clone() } else { original },
                name,
                quantity,
                unit: normalize_unit(&get(&self.ing_unit, i)),
                category: Category::parse(&get(&self.ing_category, i)),
                estimated: get(&self.ing_estimated, i) == "1",
                note: get(&self.ing_note, i),
            });
        }
        Ok(Recipe {
            id: self.id,
            title,
            description: self.description.trim().to_string(),
            servings: self.servings.trim().parse().unwrap_or(2).max(1),
            total_minutes: self.total_minutes.trim().parse().ok(),
            source: self.source.trim().to_string(),
            ingredients,
            steps: split_steps(&self.steps),
            notes: self.notes.trim().to_string(),
            images: self.images.into_iter().filter(|s| is_safe_file_name(s)).collect(),
        })
    }
}

/// One step per paragraph or line; strips "1." / "2)" numbering people type.
pub fn split_steps(text: &str) -> Vec<String> {
    text.lines()
        .map(|l| {
            let l = l.trim();
            let digits = l.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits > 0 && l[digits..].starts_with(['.', ')']) { l[digits + 1..].trim() } else { l }
        })
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect()
}

/// Image names come back from the form; only accept names we could have made.
pub fn is_safe_file_name(name: &str) -> bool {
    !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.') && !name.starts_with('.')
}

async fn save(State(state): State<AppState>, body: String) -> WebResult<impl IntoResponse> {
    let form: RecipeForm = serde_html_form::from_str(&body).map_err(|e| AppError::bad_request(format!("Bad form: {e}")))?;
    let recipe = form.into_recipe().map_err(AppError::bad_request)?;
    let id = state.db().save_recipe(&recipe)?;
    Ok(Redirect::to(&format!("/recipes/{id}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_to_recipe() {
        let body = "id=0&title=Tacos&servings=2&total_minutes=30&steps=1.+Chop%0A%0A2)+Cook&\
                    ing_original=1+packet+taco+seasoning&ing_quantity=1+1%2F2&ing_unit=Tablespoons&ing_name=taco+seasoning&ing_category=spices&ing_estimated=1&ing_note=1+packet&\
                    ing_original=&ing_quantity=&ing_unit=&ing_name=&ing_category=other&ing_estimated=0&ing_note=&\
                    ing_original=&ing_quantity=2&ing_unit=&ing_name=limes&ing_category=produce&ing_estimated=0&ing_note=&\
                    images=abc.jpg&images=..%2Fsecret";
        let form: RecipeForm = serde_html_form::from_str(body).unwrap();
        let r = form.into_recipe().unwrap();
        assert_eq!(r.title, "Tacos");
        assert_eq!(r.total_minutes, Some(30));
        assert_eq!(r.steps, vec!["Chop", "Cook"]);
        assert_eq!(r.ingredients.len(), 2, "blank row skipped");
        assert_eq!(r.ingredients[0].quantity, Some(1.5));
        assert_eq!(r.ingredients[0].unit, "tbsp");
        assert!(r.ingredients[0].estimated);
        assert_eq!(r.ingredients[0].category, Category::Spices);
        assert_eq!(r.ingredients[1].original, "limes");
        assert_eq!(r.images, vec!["abc.jpg"]);
    }

    #[test]
    fn rejects_bad_amount_and_missing_title() {
        let form: RecipeForm = serde_html_form::from_str("title=&ing_name=x").unwrap();
        assert!(form.into_recipe().is_err());
        let form: RecipeForm = serde_html_form::from_str("title=A&ing_name=x&ing_quantity=lots").unwrap();
        assert!(form.into_recipe().unwrap_err().contains("lots"));
    }
}
