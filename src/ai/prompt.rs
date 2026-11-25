//! What we ask the AI, and the exact answer format we require.

use serde_json::{Value, json};

pub const SYSTEM: &str = r#"You read photos of recipe cards (often HelloFresh, front and back) and write the recipe out as data for a home cook's shopping list.

Rules:
- Copy the title, a one-line description (subtitle on the card, or ""), total time in minutes (null if not shown), the ingredients and the numbered steps.
- Steps: one string per step, in order, without the step number. Keep the card's wording; you may join a step's sub-points into one paragraph.
- Ingredients: include every ingredient, including the "you will need" / "bust out" pantry items such as salt, pepper, olive oil, butter, sugar or eggs. Each ingredient appears once.
  - original: the ingredient exactly as written on the card, including its amount.
  - name: a plain grocery-store name in lowercase, without amounts or packaging words. "1 packet Balsamic Vinegar" -> "balsamic vinegar"; "2 Roma Tomatoes" -> "roma tomatoes"; "Chicken Breast Strips" -> "chicken breast strips".
  - quantity and unit: the amount for the requested number of servings. Cards often show two columns (2 and 4 people); use the requested one.
    unit must be one of: tsp, tbsp, cup, fl oz, oz, lb, g, ml, or a counting word such as clove, can, slice, bunch, sprig, or "" for a plain count (2 limes -> quantity 2, unit "").
    If there is no amount (e.g. "salt and pepper"), quantity is null and unit is "".
  - Packets, pouches, sachets, containers, tubs and "Spice Blend" style items must be converted into tsp, tbsp or cup (for sauces, spices, oils, cheese) or oz (for solid foods), using your best estimate of a typical meal-kit portion. Set estimated to true and explain in estimate_note, e.g. "1 packet ≈ 2 tbsp". Examples of typical sizes: a sauce/vinegar/dressing packet ≈ 1-2 tbsp; a spice blend sachet ≈ 1-2 tsp; a stock concentrate ≈ 1 tbsp; a sour cream or cream cheese tub ≈ 2-4 tbsp; a shredded cheese pouch ≈ ¼-½ cup.
    When a spice blend is a named mix (e.g. "Fry Seasoning"), keep the mix as one ingredient; don't split it into its parts.
  - For any amount you had to guess or could not read clearly, set estimated to true and say why in estimate_note. Otherwise estimated is false and estimate_note is "".
  - category is the store aisle: produce (fresh fruit, vegetables, fresh herbs), meat_seafood, dairy_eggs (milk, cheese, butter, eggs, sour cream, yogurt), bakery (bread, buns, tortillas, naan), frozen, pantry (oils, vinegars, sauces, rice, pasta, canned goods, stock, flour, sugar, nuts), spices (dried spices, salt, pepper, seasoning blends), other.
- notes: anything else useful (allergens, tips), or "".
- If something is unreadable, make your best guess and mark it estimated. Never invent a recipe that isn't in the photos.
"#;

pub fn user_message(servings: i64, photos: usize) -> String {
    let photos = if photos == 1 { "this photo".to_string() } else { format!("these {photos} photos (all are the same recipe)") };
    format!("Read the recipe in {photos}. Use the amounts for {servings} servings.")
}

/// JSON schema for the answer. Strict mode: every property listed as required,
/// optional values expressed as nullable types.
pub fn schema() -> Value {
    let categories = ["produce", "meat_seafood", "dairy_eggs", "bakery", "frozen", "pantry", "spices", "other"];
    json!({
        "type": "object",
        "additionalProperties": false,
        "required": ["title", "description", "servings", "total_minutes", "ingredients", "steps", "notes"],
        "properties": {
            "title": { "type": "string" },
            "description": { "type": "string" },
            "servings": { "type": ["integer", "null"] },
            "total_minutes": { "type": ["integer", "null"] },
            "ingredients": {
                "type": "array",
                "items": {
                    "type": "object",
                    "additionalProperties": false,
                    "required": ["original", "name", "quantity", "unit", "category", "estimated", "estimate_note"],
                    "properties": {
                        "original": { "type": "string" },
                        "name": { "type": "string" },
                        "quantity": { "type": ["number", "null"] },
                        "unit": { "type": "string" },
                        "category": { "type": "string", "enum": categories },
                        "estimated": { "type": "boolean" },
                        "estimate_note": { "type": "string" }
                    }
                }
            },
            "steps": { "type": "array", "items": { "type": "string" } },
            "notes": { "type": "string" }
        }
    })
}
