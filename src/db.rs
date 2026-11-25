//! Everything stored on disk lives in one SQLite file.

use std::path::Path;

use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};

use crate::models::{Category, ExtraItem, Ingredient, Pick, Recipe, Trip};

pub struct Db {
    conn: Connection,
}

const SCHEMA: &str = r#"
PRAGMA foreign_keys = ON;

CREATE TABLE IF NOT EXISTS recipes (
    id            INTEGER PRIMARY KEY,
    title         TEXT NOT NULL,
    description   TEXT NOT NULL DEFAULT '',
    servings      INTEGER NOT NULL DEFAULT 2,
    total_minutes INTEGER,
    source        TEXT NOT NULL DEFAULT '',
    steps         TEXT NOT NULL DEFAULT '[]',   -- JSON array of strings
    notes         TEXT NOT NULL DEFAULT '',
    images        TEXT NOT NULL DEFAULT '[]',   -- JSON array of file names
    created_at    TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at    TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE TABLE IF NOT EXISTS ingredients (
    id        INTEGER PRIMARY KEY,
    recipe_id INTEGER NOT NULL REFERENCES recipes(id) ON DELETE CASCADE,
    position  INTEGER NOT NULL,
    original  TEXT NOT NULL DEFAULT '',
    name      TEXT NOT NULL,
    quantity  REAL,
    unit      TEXT NOT NULL DEFAULT '',
    category  TEXT NOT NULL DEFAULT 'other',
    estimated INTEGER NOT NULL DEFAULT 0,
    note      TEXT NOT NULL DEFAULT ''
);
CREATE INDEX IF NOT EXISTS ingredients_recipe ON ingredients(recipe_id);

-- Full-text search over title, ingredient names and steps. rowid = recipe id.
CREATE VIRTUAL TABLE IF NOT EXISTS recipe_search USING fts5(title, ingredients, body);

-- The current week: which recipes are picked and for which trip.
CREATE TABLE IF NOT EXISTS week_picks (
    recipe_id INTEGER PRIMARY KEY REFERENCES recipes(id) ON DELETE CASCADE,
    trip      INTEGER NOT NULL CHECK (trip IN (1, 2))
);

-- Ticks on the shopping list, keyed by "trip|ingredient key".
CREATE TABLE IF NOT EXISTS list_checks (
    item_key TEXT PRIMARY KEY,
    have     INTEGER NOT NULL DEFAULT 0,
    got      INTEGER NOT NULL DEFAULT 0,
    -- The amount on the list when "got" was ticked. If the amount changes
    -- later (another recipe added), the tick no longer counts.
    got_amount TEXT NOT NULL DEFAULT ''
);

-- Items added to the list by hand.
CREATE TABLE IF NOT EXISTS extra_items (
    id     INTEGER PRIMARY KEY,
    name   TEXT NOT NULL,
    amount TEXT NOT NULL DEFAULT '',
    trip   INTEGER NOT NULL CHECK (trip IN (1, 2))
);
"#;

/// One row in the recipe list.
pub struct RecipeSummary {
    pub id: i64,
    pub title: String,
    pub description: String,
    pub total_minutes: Option<i64>,
    pub ingredient_count: i64,
    pub thumbnail: Option<String>,
}

impl Db {
    pub fn open(path: &Path) -> Result<Db> {
        let conn = Connection::open(path).with_context(|| format!("opening database {}", path.display()))?;
        Db::init(conn)
    }

    pub fn open_in_memory() -> Result<Db> {
        Db::init(Connection::open_in_memory()?)
    }

    fn init(conn: Connection) -> Result<Db> {
        conn.execute_batch(SCHEMA).context("creating tables")?;
        Ok(Db { conn })
    }

    // ---------- recipes ----------

    /// Insert (id == 0) or update a recipe. Returns its id.
    pub fn save_recipe(&mut self, r: &Recipe) -> Result<i64> {
        let tx = self.conn.transaction()?;
        let steps = serde_json::to_string(&r.steps)?;
        let images = serde_json::to_string(&r.images)?;
        let id = if r.id == 0 {
            tx.execute(
                "INSERT INTO recipes (title, description, servings, total_minutes, source, steps, notes, images)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    r.title,
                    r.description,
                    r.servings,
                    r.total_minutes,
                    r.source,
                    steps,
                    r.notes,
                    images
                ],
            )?;
            tx.last_insert_rowid()
        } else {
            let n = tx.execute(
                "UPDATE recipes SET title=?1, description=?2, servings=?3, total_minutes=?4, source=?5,
                        steps=?6, notes=?7, images=?8, updated_at=datetime('now') WHERE id=?9",
                params![
                    r.title,
                    r.description,
                    r.servings,
                    r.total_minutes,
                    r.source,
                    steps,
                    r.notes,
                    images,
                    r.id
                ],
            )?;
            anyhow::ensure!(n == 1, "recipe {} not found", r.id);
            tx.execute("DELETE FROM ingredients WHERE recipe_id=?1", [r.id])?;
            r.id
        };
        for (i, ing) in r.ingredients.iter().enumerate() {
            tx.execute(
                "INSERT INTO ingredients (recipe_id, position, original, name, quantity, unit, category, estimated, note)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![id, i as i64, ing.original, ing.name, ing.quantity, ing.unit, ing.category.as_str(), ing.estimated, ing.note],
            )?;
        }
        let ingredient_names = r
            .ingredients
            .iter()
            .map(|i| i.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        let body = format!("{}\n{}\n{}", r.description, r.steps.join("\n"), r.notes);
        tx.execute("DELETE FROM recipe_search WHERE rowid=?1", [id])?;
        tx.execute(
            "INSERT INTO recipe_search (rowid, title, ingredients, body) VALUES (?1, ?2, ?3, ?4)",
            params![id, r.title, ingredient_names, body],
        )?;
        tx.commit()?;
        Ok(id)
    }

    pub fn get_recipe(&self, id: i64) -> Result<Option<Recipe>> {
        let row = self
            .conn
            .query_row(
                "SELECT id, title, description, servings, total_minutes, source, steps, notes, images FROM recipes WHERE id=?1",
                [id],
                |row| {
                    Ok((
                        row.get::<_, i64>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                        row.get::<_, i64>(3)?,
                        row.get::<_, Option<i64>>(4)?,
                        row.get::<_, String>(5)?,
                        row.get::<_, String>(6)?,
                        row.get::<_, String>(7)?,
                        row.get::<_, String>(8)?,
                    ))
                },
            )
            .optional()?;
        let Some((id, title, description, servings, total_minutes, source, steps, notes, images)) = row else {
            return Ok(None);
        };
        Ok(Some(Recipe {
            id,
            title,
            description,
            servings,
            total_minutes,
            source,
            ingredients: self.ingredients_for(id)?,
            steps: serde_json::from_str(&steps).unwrap_or_default(),
            notes,
            images: serde_json::from_str(&images).unwrap_or_default(),
        }))
    }

    fn ingredients_for(&self, recipe_id: i64) -> Result<Vec<Ingredient>> {
        let mut stmt = self.conn.prepare_cached(
            "SELECT original, name, quantity, unit, category, estimated, note
             FROM ingredients WHERE recipe_id=?1 ORDER BY position",
        )?;
        let rows = stmt.query_map([recipe_id], |row| {
            Ok(Ingredient {
                original: row.get(0)?,
                name: row.get(1)?,
                quantity: row.get(2)?,
                unit: row.get(3)?,
                category: Category::parse(&row.get::<_, String>(4)?),
                estimated: row.get(5)?,
                note: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn all_recipes(&self) -> Result<Vec<Recipe>> {
        let ids: Vec<i64> = {
            let mut stmt = self
                .conn
                .prepare("SELECT id FROM recipes ORDER BY title COLLATE NOCASE")?;
            stmt.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<_>>()?
        };
        ids.into_iter()
            .filter_map(|id| self.get_recipe(id).transpose())
            .collect()
    }

    pub fn delete_recipe(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM recipes WHERE id=?1", [id])?;
        self.conn.execute("DELETE FROM recipe_search WHERE rowid=?1", [id])?;
        Ok(())
    }

    pub fn title_exists(&self, title: &str) -> Result<bool> {
        Ok(self
            .conn
            .query_row(
                "SELECT 1 FROM recipes WHERE lower(title)=lower(?1) LIMIT 1",
                [title],
                |_| Ok(()),
            )
            .optional()?
            .is_some())
    }

    /// All recipes (search empty) or those matching the search words.
    pub fn list_recipes(&self, search: &str) -> Result<Vec<RecipeSummary>> {
        let fts = fts_query(search);
        let base = "SELECT r.id, r.title, r.description, r.total_minutes, r.images,
                           (SELECT count(*) FROM ingredients i WHERE i.recipe_id = r.id)
                    FROM recipes r";
        let map = |row: &rusqlite::Row| {
            let images: String = row.get(4)?;
            let images: Vec<String> = serde_json::from_str(&images).unwrap_or_default();
            Ok(RecipeSummary {
                id: row.get(0)?,
                title: row.get(1)?,
                description: row.get(2)?,
                total_minutes: row.get(3)?,
                thumbnail: images.into_iter().next(),
                ingredient_count: row.get(5)?,
            })
        };
        let rows = match fts {
            None => {
                let mut stmt = self.conn.prepare(&format!("{base} ORDER BY r.title COLLATE NOCASE"))?;
                stmt.query_map([], map)?.collect::<rusqlite::Result<Vec<_>>>()?
            }
            Some(q) => {
                let mut stmt = self.conn.prepare(&format!(
                    "{base} JOIN recipe_search s ON s.rowid = r.id WHERE recipe_search MATCH ?1 ORDER BY s.rank"
                ))?;
                stmt.query_map([q], map)?.collect::<rusqlite::Result<Vec<_>>>()?
            }
        };
        Ok(rows)
    }

    // ---------- this week ----------

    pub fn picks(&self) -> Result<Vec<Pick>> {
        let mut stmt = self.conn.prepare("SELECT recipe_id, trip FROM week_picks")?;
        let rows = stmt.query_map([], |r| {
            Ok(Pick {
                recipe_id: r.get(0)?,
                trip: Trip::from_i64(r.get(1)?).unwrap_or(Trip::One),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn set_picks(&mut self, picks: &[Pick]) -> Result<()> {
        let tx = self.conn.transaction()?;
        tx.execute("DELETE FROM week_picks", [])?;
        for p in picks {
            tx.execute(
                "INSERT OR REPLACE INTO week_picks (recipe_id, trip) VALUES (?1, ?2)",
                params![p.recipe_id, p.trip.num()],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// Ticks for every list item that has any.
    pub fn checks(&self) -> Result<std::collections::HashMap<String, Check>> {
        let mut stmt = self
            .conn
            .prepare("SELECT item_key, have, got, got_amount FROM list_checks")?;
        let rows = stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                Check {
                    have: r.get(1)?,
                    got: r.get(2)?,
                    got_amount: r.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// `amount` is the amount shown when ticking "got".
    pub fn set_check(&self, key: &str, field: CheckField, value: bool, amount: &str) -> Result<()> {
        match field {
            CheckField::Have => self.conn.execute(
                "INSERT INTO list_checks (item_key, have) VALUES (?1, ?2) ON CONFLICT(item_key) DO UPDATE SET have=excluded.have",
                params![key, value],
            )?,
            CheckField::Got => self.conn.execute(
                "INSERT INTO list_checks (item_key, got, got_amount) VALUES (?1, ?2, ?3)
                 ON CONFLICT(item_key) DO UPDATE SET got=excluded.got, got_amount=excluded.got_amount",
                params![key, value, amount],
            )?,
        };
        Ok(())
    }

    pub fn extra_items(&self) -> Result<Vec<ExtraItem>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, amount, trip FROM extra_items ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(ExtraItem {
                id: r.get(0)?,
                name: r.get(1)?,
                amount: r.get(2)?,
                trip: Trip::from_i64(r.get(3)?).unwrap_or(Trip::One),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    pub fn add_extra_item(&self, name: &str, amount: &str, trip: Trip) -> Result<()> {
        self.conn.execute(
            "INSERT INTO extra_items (name, amount, trip) VALUES (?1, ?2, ?3)",
            params![name, amount, trip.num()],
        )?;
        Ok(())
    }

    pub fn delete_extra_item(&self, id: i64) -> Result<()> {
        self.conn.execute("DELETE FROM extra_items WHERE id=?1", [id])?;
        self.conn
            .execute("DELETE FROM list_checks WHERE item_key=?1", [format!("extra|{id}")])?;
        Ok(())
    }

    /// Today's date (YYYY-MM-DD, local time), for file names.
    pub fn today(&self) -> Result<String> {
        Ok(self
            .conn
            .query_row("SELECT date('now', 'localtime')", [], |r| r.get(0))?)
    }

    /// Start a fresh week: clear picks, ticks and extra items.
    pub fn new_week(&self) -> Result<()> {
        self.conn
            .execute_batch("DELETE FROM week_picks; DELETE FROM list_checks; DELETE FROM extra_items;")?;
        Ok(())
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Check {
    pub have: bool,
    pub got: bool,
    pub got_amount: String,
}

#[derive(Debug, Clone, Copy)]
pub enum CheckField {
    Have,
    Got,
}

/// Turn what the user typed into a safe FTS5 query: every word must match,
/// and the last word may be a prefix ("chick" finds "chicken").
fn fts_query(search: &str) -> Option<String> {
    let words: Vec<String> = search
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| format!("\"{w}\"*"))
        .collect();
    if words.is_empty() { None } else { Some(words.join(" ")) }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::sample;

    #[test]
    fn save_load_update_delete() {
        let mut db = Db::open_in_memory().unwrap();
        let mut r = sample(
            "Balsamic Chicken",
            &[("chicken breast", 10.0, "oz", Category::MeatSeafood)],
        );
        let id = db.save_recipe(&r).unwrap();
        r.id = id;
        assert_eq!(db.get_recipe(id).unwrap().unwrap(), r);

        r.title = "Better Balsamic Chicken".into();
        r.ingredients.push(Ingredient {
            name: "balsamic vinegar".into(),
            estimated: true,
            ..r.ingredients[0].clone()
        });
        db.save_recipe(&r).unwrap();
        assert_eq!(db.get_recipe(id).unwrap().unwrap(), r);

        db.delete_recipe(id).unwrap();
        assert!(db.get_recipe(id).unwrap().is_none());
        assert!(db.list_recipes("balsamic").unwrap().is_empty());
    }

    #[test]
    fn search_finds_title_and_ingredients() {
        let mut db = Db::open_in_memory().unwrap();
        db.save_recipe(&sample("Pork Tacos", &[("pork", 1.0, "lb", Category::MeatSeafood)]))
            .unwrap();
        db.save_recipe(&sample(
            "Chicken Bowl",
            &[("jasmine rice", 1.0, "cup", Category::Pantry)],
        ))
        .unwrap();
        let titles = |q: &str| {
            db.list_recipes(q)
                .unwrap()
                .into_iter()
                .map(|r| r.title)
                .collect::<Vec<_>>()
        };
        assert_eq!(titles("chick"), vec!["Chicken Bowl"]);
        assert_eq!(titles("rice"), vec!["Chicken Bowl"]);
        assert_eq!(titles("tacos pork"), vec!["Pork Tacos"]);
        assert_eq!(titles("").len(), 2);
        assert_eq!(titles("\"(*").len(), 2, "punctuation-only search shows everything");
    }

    #[test]
    fn deleting_recipe_removes_pick() {
        let mut db = Db::open_in_memory().unwrap();
        let id = db.save_recipe(&sample("A", &[])).unwrap();
        db.set_picks(&[Pick {
            recipe_id: id,
            trip: Trip::Two,
        }])
        .unwrap();
        assert_eq!(db.picks().unwrap().len(), 1);
        db.delete_recipe(id).unwrap();
        assert!(db.picks().unwrap().is_empty());
    }
}
