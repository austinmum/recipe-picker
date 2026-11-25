//! Export everything to one .zip, and import it back.
//!
//! The zip holds:
//! - recipes.json      every recipe, in a form this app can import again
//! - recipes/*.md      one readable file per recipe (opens in any text editor)
//! - images/*          the original photos

use std::collections::HashSet;
use std::io::{Cursor, Read, Write};
use std::path::Path;

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use zip::write::SimpleFileOptions;

use crate::{db::Db, models::Recipe, units::format_number, web::is_safe_file_name};

#[derive(Serialize, Deserialize)]
struct ExportFile {
    app: String,
    format_version: u32,
    recipes: Vec<Recipe>,
}

pub fn export_zip(db: &Db, images_dir: &Path) -> Result<Vec<u8>> {
    let recipes = db.all_recipes()?;
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default();

    let file = ExportFile {
        app: "recipe-picker".into(),
        format_version: 1,
        recipes,
    };
    zip.start_file("recipes.json", opts)?;
    zip.write_all(serde_json::to_string_pretty(&file)?.as_bytes())?;

    let mut used_names = HashSet::new();
    for r in &file.recipes {
        let mut name = slug(&r.title);
        if !used_names.insert(name.clone()) {
            name = format!("{name}-{}", r.id);
            used_names.insert(name.clone());
        }
        zip.start_file(format!("recipes/{name}.md"), opts)?;
        zip.write_all(markdown(r).as_bytes())?;
    }

    let mut done = HashSet::new();
    for img in file.recipes.iter().flat_map(|r| &r.images) {
        if !is_safe_file_name(img) || !done.insert(img.clone()) {
            continue;
        }
        if let Ok(bytes) = std::fs::read(images_dir.join(img)) {
            // Photos are already compressed; storing them as-is is faster.
            zip.start_file(
                format!("images/{img}"),
                opts.compression_method(zip::CompressionMethod::Stored),
            )?;
            zip.write_all(&bytes)?;
        }
    }
    Ok(zip.finish()?.into_inner())
}

#[derive(Debug, Default)]
pub struct ImportSummary {
    pub added: usize,
    /// Titles skipped because a recipe with that title already exists.
    pub skipped: Vec<String>,
}

/// Import a zip made by `export_zip` (or just its recipes.json).
/// Recipes whose title already exists are skipped, so importing the same
/// file twice doesn't make duplicates.
pub fn import(db: &mut Db, images_dir: &Path, bytes: &[u8]) -> Result<ImportSummary> {
    let is_zip = bytes.starts_with(b"PK");
    let mut archive = if is_zip {
        Some(zip::ZipArchive::new(Cursor::new(bytes)).context("That file isn't a readable .zip")?)
    } else {
        None
    };

    let json = match archive.as_mut() {
        Some(a) => {
            let mut f = a
                .by_name("recipes.json")
                .context("This .zip has no recipes.json, so it isn't a Recipe Picker export.")?;
            let mut s = String::new();
            f.read_to_string(&mut s)?;
            s
        }
        None => String::from_utf8(bytes.to_vec()).context("Choose the .zip from Export (or its recipes.json).")?,
    };
    let file: ExportFile = serde_json::from_str(&json).context("recipes.json couldn't be read")?;
    if file.app != "recipe-picker" {
        bail!("This file wasn't made by Recipe Picker.");
    }

    let mut summary = ImportSummary::default();
    for mut r in file.recipes {
        if r.title.trim().is_empty() {
            continue;
        }
        if db.title_exists(&r.title)? {
            summary.skipped.push(r.title);
            continue;
        }
        r.images.retain(|i| is_safe_file_name(i));
        if let Some(a) = archive.as_mut() {
            for img in &r.images {
                let dest = images_dir.join(img);
                if dest.exists() {
                    continue;
                }
                if let Ok(mut f) = a.by_name(&format!("images/{img}")) {
                    let mut data = Vec::new();
                    f.read_to_end(&mut data)?;
                    std::fs::write(&dest, data)?;
                }
            }
        }
        r.images.retain(|i| images_dir.join(i).exists());
        r.id = 0;
        db.save_recipe(&r)?;
        summary.added += 1;
    }
    Ok(summary)
}

fn slug(title: &str) -> String {
    let s: String = title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let s = s.split('-').filter(|p| !p.is_empty()).collect::<Vec<_>>().join("-");
    if s.is_empty() { "recipe".into() } else { s }
}

pub fn markdown(r: &Recipe) -> String {
    let mut md = format!("# {}\n\n", r.title);
    if !r.description.is_empty() {
        md += &format!("_{}_\n\n", r.description);
    }
    let mut facts = vec![format!("Serves {}", r.servings)];
    if let Some(m) = r.total_minutes {
        facts.push(format!("{m} min"));
    }
    if !r.source.is_empty() {
        facts.push(r.source.clone());
    }
    md += &format!("{}\n\n## Ingredients\n\n", facts.join(" · "));
    for i in &r.ingredients {
        let amount = i
            .quantity
            .map(|q| format!("{}{} {} ", if i.estimated { "~" } else { "" }, format_number(q), i.unit))
            .unwrap_or_default();
        let amount = amount.replace("  ", " ");
        let note = if i.estimated && !i.note.is_empty() {
            format!(" _(estimated: {})_", i.note)
        } else {
            String::new()
        };
        md += &format!("- {amount}{}{note}\n", i.name);
    }
    md += "\n## Steps\n\n";
    for (n, s) in r.steps.iter().enumerate() {
        md += &format!("{}. {s}\n", n + 1);
    }
    if !r.notes.is_empty() {
        md += &format!("\n## Notes\n\n{}\n", r.notes);
    }
    if !r.images.is_empty() {
        md += "\n## Photos\n\n";
        for img in &r.images {
            md += &format!("![](../images/{img})\n");
        }
    }
    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category, sample};

    #[test]
    fn export_then_import_gives_same_recipes() {
        let src_dir = tempfile::tempdir().unwrap();
        let mut db = Db::open_in_memory().unwrap();
        let mut a = sample(
            "Balsamic Chicken",
            &[("balsamic vinegar", 2.0, "tbsp", Category::Pantry)],
        );
        a.ingredients[0].estimated = true;
        a.ingredients[0].note = "1 packet ≈ 2 tbsp".into();
        a.images = vec!["abc.jpg".into()];
        std::fs::write(src_dir.path().join("abc.jpg"), b"photo").unwrap();
        db.save_recipe(&a).unwrap();
        db.save_recipe(&sample("Tacos", &[("lime", 1.0, "", Category::Produce)]))
            .unwrap();

        let zip = export_zip(&db, src_dir.path()).unwrap();

        let dest_dir = tempfile::tempdir().unwrap();
        let mut fresh = Db::open_in_memory().unwrap();
        let s = import(&mut fresh, dest_dir.path(), &zip).unwrap();
        assert_eq!(s.added, 2);
        let strip = |mut v: Vec<Recipe>| {
            v.iter_mut().for_each(|r| r.id = 0);
            v
        };
        assert_eq!(strip(fresh.all_recipes().unwrap()), strip(db.all_recipes().unwrap()));
        assert_eq!(std::fs::read(dest_dir.path().join("abc.jpg")).unwrap(), b"photo");

        // Importing again adds nothing.
        let again = import(&mut fresh, dest_dir.path(), &zip).unwrap();
        assert_eq!(again.added, 0);
        assert_eq!(again.skipped.len(), 2);
    }

    #[test]
    fn markdown_reads_well() {
        let mut r = sample(
            "Tacos",
            &[
                ("taco seasoning", 1.5, "tbsp", Category::Spices),
                ("lime", 2.0, "", Category::Produce),
            ],
        );
        r.ingredients[0].estimated = true;
        r.ingredients[0].note = "1 packet".into();
        let md = markdown(&r);
        assert!(md.contains("- ~1½ tbsp taco seasoning _(estimated: 1 packet)_"), "{md}");
        assert!(md.contains("- 2 lime\n"), "{md}");
        assert!(md.contains("1. Cook it."));
    }

    #[test]
    fn rejects_other_files() {
        let mut db = Db::open_in_memory().unwrap();
        let dir = tempfile::tempdir().unwrap();
        assert!(import(&mut db, dir.path(), b"hello").is_err());
        assert!(
            import(
                &mut db,
                dir.path(),
                br#"{"app":"other","format_version":1,"recipes":[]}"#
            )
            .is_err()
        );
    }
}
