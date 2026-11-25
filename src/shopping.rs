//! Builds the week's shopping list from the picked recipes.
//!
//! Rules:
//! - Fresh items (produce, meat, dairy, bakery) are bought on the trip of the
//!   recipe that uses them, so they don't go bad.
//! - Everything else (pantry, spices, frozen, other) is bought once, on Trip 1,
//!   with the total for the whole week.
//! - The same ingredient from several recipes is added up into one line.

use std::collections::{BTreeMap, HashMap};

use crate::{
    db::Check,
    models::{Category, ExtraItem, Recipe, Trip},
    units::{Amount, name_key},
};

#[derive(Debug, Clone)]
pub struct ListItem {
    /// Stable id for saving ticks; survives rebuilding the list.
    pub key: String,
    pub name: String,
    pub amount: String,
    /// At least one of the amounts added up was an estimate.
    pub estimated: bool,
    /// Which recipes need it.
    pub recipes: Vec<String>,
    pub have: bool,
    pub got: bool,
    /// Set for items added by hand (so they can be removed).
    pub extra_id: Option<i64>,
}

#[derive(Debug, Clone)]
pub struct Aisle {
    pub category: Category,
    pub label: &'static str,
    pub items: Vec<ListItem>,
}

#[derive(Debug, Clone)]
pub struct TripList {
    pub trip: Trip,
    /// Titles of the recipes assigned to this trip.
    pub recipes: Vec<String>,
    pub aisles: Vec<Aisle>,
    pub extras: Vec<ListItem>,
}

impl TripList {
    pub fn item_count(&self) -> usize {
        self.aisles.iter().map(|a| a.items.len()).sum::<usize>() + self.extras.len()
    }
}

struct Acc {
    name: String,
    category: Category,
    amount: Amount,
    estimated: bool,
    recipes: Vec<String>,
}

pub fn item_key(trip: Trip, name: &str) -> String {
    format!("{}|{}", trip.num(), name_key(name))
}

pub fn extra_key(id: i64) -> String {
    format!("extra|{id}")
}

/// Is this item ticked (have, got)? "got" only counts if the amount hasn't
/// changed since it was ticked.
fn ticks(checks: &HashMap<String, Check>, key: &str, amount: &str) -> (bool, bool) {
    match checks.get(key) {
        Some(c) => (c.have, c.got && c.got_amount == amount),
        None => (false, false),
    }
}

pub fn build(picked: &[(Recipe, Trip)], extras: &[ExtraItem], checks: &HashMap<String, Check>) -> Vec<TripList> {
    let mut acc: BTreeMap<String, (Trip, Acc)> = BTreeMap::new();
    for (recipe, recipe_trip) in picked {
        for ing in &recipe.ingredients {
            let trip = if ing.category.is_fresh() { *recipe_trip } else { Trip::One };
            let key = item_key(trip, &ing.name);
            let (_, a) = acc.entry(key).or_insert_with(|| {
                (trip, Acc { name: ing.name.clone(), category: ing.category, amount: Amount::default(), estimated: false, recipes: Vec::new() })
            });
            a.amount.add(ing.quantity, &ing.unit);
            a.estimated |= ing.estimated;
            if !a.recipes.contains(&recipe.title) {
                a.recipes.push(recipe.title.clone());
            }
        }
    }

    [Trip::One, Trip::Two]
        .into_iter()
        .map(|trip| {
            let mut by_aisle: BTreeMap<Category, Vec<ListItem>> = BTreeMap::new();
            for (key, (t, a)) in &acc {
                if *t != trip {
                    continue;
                }
                let amount = a.amount.display();
                let (have, got) = ticks(checks, key, &amount);
                by_aisle.entry(a.category).or_default().push(ListItem {
                    key: key.clone(),
                    name: a.name.clone(),
                    amount,
                    estimated: a.estimated,
                    recipes: a.recipes.clone(),
                    have,
                    got,
                    extra_id: None,
                });
            }
            let aisles = by_aisle
                .into_iter()
                .map(|(category, mut items)| {
                    items.sort_by_key(|i| i.name.to_lowercase());
                    Aisle { category, label: category.label(), items }
                })
                .collect();
            let extras = extras
                .iter()
                .filter(|e| e.trip == trip)
                .map(|e| {
                    let key = extra_key(e.id);
                    let (have, got) = ticks(checks, &key, &e.amount);
                    ListItem { key, name: e.name.clone(), amount: e.amount.clone(), estimated: false, recipes: vec![], have, got, extra_id: Some(e.id) }
                })
                .collect();
            let recipes = picked.iter().filter(|(_, t)| *t == trip).map(|(r, _)| r.title.clone()).collect();
            TripList { trip, recipes, aisles, extras }
        })
        .collect()
}

/// Plain-text version of the list (for copying somewhere else).
pub fn as_text(trips: &[TripList]) -> String {
    let mut out = String::new();
    for t in trips {
        out.push_str(&format!("TRIP {}\n", t.trip.num()));
        for a in &t.aisles {
            let needed: Vec<_> = a.items.iter().filter(|i| !i.have).collect();
            if needed.is_empty() {
                continue;
            }
            out.push_str(&format!("\n{}\n", a.label.to_uppercase()));
            for i in needed {
                out.push_str(&format!("[{}] {}{} {}\n", if i.got { "x" } else { " " }, if i.estimated { "~" } else { "" }, i.amount, i.name));
            }
        }
        for i in t.extras.iter().filter(|i| !i.have) {
            out.push_str(&format!("[{}] {} {}\n", if i.got { "x" } else { " " }, i.amount, i.name).replace("]  ", "] "));
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{Category::*, sample};

    fn find<'a>(t: &'a TripList, name: &str) -> Option<&'a ListItem> {
        t.aisles.iter().flat_map(|a| &a.items).find(|i| i.name.eq_ignore_ascii_case(name))
    }

    #[test]
    fn fresh_follows_recipe_trip_and_pantry_goes_on_trip_one() {
        let a = sample("Tacos", &[("lime", 1.0, "", Produce), ("cumin", 1.0, "tsp", Spices), ("ground beef", 10.0, "oz", MeatSeafood)]);
        let b = sample("Bowl", &[("limes", 2.0, "", Produce), ("cumin", 2.0, "tsp", Spices), ("rice", 0.5, "cup", Pantry)]);
        let trips = build(&[(a, Trip::One), (b, Trip::Two)], &[], &HashMap::new());
        let (one, two) = (&trips[0], &trips[1]);

        // Limes are fresh: one on each trip, not combined.
        assert_eq!(find(one, "lime").unwrap().amount, "1");
        assert_eq!(find(two, "limes").unwrap().amount, "2");
        // Cumin is shelf-stable: combined, on Trip 1, for both recipes.
        let cumin = find(one, "cumin").unwrap();
        assert_eq!(cumin.amount, "1 tbsp");
        assert_eq!(cumin.recipes, vec!["Tacos", "Bowl"]);
        assert!(find(two, "cumin").is_none());
        // Rice comes from a Trip 2 recipe but is pantry -> Trip 1.
        assert!(find(one, "rice").is_some());
        assert!(find(two, "rice").is_none());
        assert_eq!(one.recipes, vec!["Tacos"]);
        assert_eq!(two.recipes, vec!["Bowl"]);
    }

    #[test]
    fn same_trip_fresh_items_combine_and_aisles_are_ordered() {
        let a = sample("A", &[("Roma tomatoes", 2.0, "", Produce), ("chicken breast", 10.0, "oz", MeatSeafood)]);
        let b = sample("B", &[("roma tomato", 1.0, "", Produce), ("chicken breast", 10.0, "oz", MeatSeafood)]);
        let trips = build(&[(a, Trip::One), (b, Trip::One)], &[], &HashMap::new());
        let one = &trips[0];
        assert_eq!(find(one, "roma tomatoes").unwrap().amount, "3");
        assert_eq!(find(one, "chicken breast").unwrap().amount, "1¼ lb");
        let order: Vec<_> = one.aisles.iter().map(|a| a.category).collect();
        assert_eq!(order, vec![Produce, MeatSeafood]);
        assert_eq!(trips[1].item_count(), 0);
    }

    #[test]
    fn estimates_and_ticks_carry_through() {
        let mut a = sample("A", &[("balsamic vinegar", 2.0, "tbsp", Pantry)]);
        a.ingredients[0].estimated = true;
        let tick = |have, got, amt: &str| Check { have, got, got_amount: amt.into() };
        let checks = HashMap::from([(item_key(Trip::One, "Balsamic Vinegar"), tick(true, false, "")), (extra_key(7), tick(false, true, "2"))]);
        let extras = vec![ExtraItem { id: 7, name: "paper towels".into(), amount: "2".into(), trip: Trip::Two }];
        let trips = build(&[(a, Trip::Two)], &extras, &checks);
        let item = find(&trips[0], "balsamic vinegar").unwrap();
        assert!(item.estimated && item.have && !item.got);
        assert!(trips[1].extras[0].got);
    }

    #[test]
    fn got_tick_resets_when_amount_changes() {
        let a = sample("A", &[("lime", 2.0, "", Produce)]);
        let b = sample("B", &[("lime", 1.0, "", Produce)]);
        let checks = HashMap::from([(item_key(Trip::One, "lime"), Check { have: false, got: true, got_amount: "2".into() })]);
        let only_a = build(&[(a.clone(), Trip::One)], &[], &checks);
        assert!(find(&only_a[0], "lime").unwrap().got);
        let both = build(&[(a, Trip::One), (b, Trip::One)], &[], &checks);
        let lime = find(&both[0], "lime").unwrap();
        assert_eq!(lime.amount, "3");
        assert!(!lime.got, "need one more lime, so it is no longer ticked");
    }
}
