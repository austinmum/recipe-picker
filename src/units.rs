//! Parsing amounts ("1 1/2", "½"), recognising units, and adding amounts
//! together across recipes (2 tbsp + 1 tsp = 2⅓ tbsp).

use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    /// Base unit: teaspoon.
    Volume,
    /// Base unit: gram.
    Weight,
}

struct UnitDef {
    canonical: &'static str,
    kind: Kind,
    /// How many base units one of this unit is.
    factor: f64,
    metric: bool,
    aliases: &'static [&'static str],
}

const UNITS: &[UnitDef] = &[
    UnitDef {
        canonical: "tsp",
        kind: Kind::Volume,
        factor: 1.0,
        metric: false,
        aliases: &["tsp", "tsps", "teaspoon", "teaspoons", "t"],
    },
    UnitDef {
        canonical: "tbsp",
        kind: Kind::Volume,
        factor: 3.0,
        metric: false,
        aliases: &["tbsp", "tbsps", "tbs", "tablespoon", "tablespoons", "T"],
    },
    UnitDef {
        canonical: "fl oz",
        kind: Kind::Volume,
        factor: 6.0,
        metric: false,
        aliases: &["fl oz", "fl. oz", "fl. oz.", "fluid ounce", "fluid ounces", "floz"],
    },
    UnitDef {
        canonical: "cup",
        kind: Kind::Volume,
        factor: 48.0,
        metric: false,
        aliases: &["cup", "cups", "c"],
    },
    UnitDef {
        canonical: "ml",
        kind: Kind::Volume,
        factor: 0.202_884,
        metric: true,
        aliases: &["ml", "mls", "milliliter", "milliliters", "millilitre", "millilitres"],
    },
    UnitDef {
        canonical: "l",
        kind: Kind::Volume,
        factor: 202.884,
        metric: true,
        aliases: &["l", "liter", "liters", "litre", "litres"],
    },
    UnitDef {
        canonical: "oz",
        kind: Kind::Weight,
        factor: 28.349_5,
        metric: false,
        aliases: &["oz", "ounce", "ounces", "oz."],
    },
    UnitDef {
        canonical: "lb",
        kind: Kind::Weight,
        factor: 453.592,
        metric: false,
        aliases: &["lb", "lbs", "pound", "pounds", "lb."],
    },
    UnitDef {
        canonical: "g",
        kind: Kind::Weight,
        factor: 1.0,
        metric: true,
        aliases: &["g", "gram", "grams", "gr"],
    },
    UnitDef {
        canonical: "kg",
        kind: Kind::Weight,
        factor: 1000.0,
        metric: true,
        aliases: &["kg", "kilogram", "kilograms", "kgs"],
    },
];

fn lookup(unit: &str) -> Option<&'static UnitDef> {
    let u = unit.trim();
    // "T" (tablespoon) and "t" (teaspoon) are case sensitive; everything else isn't.
    if let Some(d) = UNITS.iter().find(|d| d.aliases.contains(&u)) {
        return Some(d);
    }
    let lower = u.to_lowercase();
    UNITS
        .iter()
        .find(|d| d.aliases.iter().any(|a| a.len() > 1 && a.to_lowercase() == lower))
}

/// Tidy a unit the user or the AI typed: "Tablespoons" -> "tbsp",
/// "Cloves" -> "clove". Unknown units are kept, lower-cased and singular.
pub fn normalize_unit(unit: &str) -> String {
    match lookup(unit) {
        Some(d) => d.canonical.to_string(),
        None => {
            let u = unit.trim().to_lowercase();
            if matches!(
                u.as_str(),
                "unit" | "units" | "each" | "ea" | "whole" | "count" | "pc" | "pcs" | "piece" | "pieces"
            ) {
                String::new()
            } else {
                singular(&u)
            }
        }
    }
}

/// Very small singulariser, only used for grouping and units.
pub fn singular(word: &str) -> String {
    let w = word.trim();
    if w.len() > 4 && w.ends_with("ies") {
        format!("{}y", &w[..w.len() - 3])
    } else if w.len() > 4 && (w.ends_with("oes") || w.ends_with("ches") || w.ends_with("shes")) {
        w[..w.len() - 2].to_string()
    } else if w.len() > 3 && w.ends_with('s') && !w.ends_with("ss") && !w.ends_with("us") {
        w[..w.len() - 1].to_string()
    } else {
        w.to_string()
    }
}

/// Key used to decide that two ingredients are "the same thing" on the list.
pub fn name_key(name: &str) -> String {
    let cleaned: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() || c == ' ' { c } else { ' ' })
        .collect();
    cleaned.split_whitespace().map(singular).collect::<Vec<_>>().join(" ")
}

/// Parse "2", "1.5", "1/2", "1 1/2", "½", "1½", "2-3" (takes the larger).
/// Returns None for blank or unreadable input.
pub fn parse_quantity(input: &str) -> Option<f64> {
    let s = input.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((a, b)) = s.split_once(['-', '–']) {
        let (a, b) = (parse_quantity(a), parse_quantity(b));
        return match (a, b) {
            (Some(a), Some(b)) => Some(a.max(b)),
            (a, b) => a.or(b),
        };
    }
    let mut total = 0.0;
    let mut any = false;
    for part in s.split_whitespace() {
        let mut part = part.to_string();
        // Split a trailing unicode fraction off a whole number: "1½" -> "1" + "½".
        let mut frac = 0.0;
        if let Some(last) = part.chars().last()
            && let Some(v) = unicode_fraction(last)
        {
            frac = v;
            part.pop();
        }
        let value = if part.is_empty() {
            0.0
        } else if let Some((n, d)) = part.split_once('/') {
            let (n, d): (f64, f64) = (n.trim().parse().ok()?, d.trim().parse().ok()?);
            if d == 0.0 {
                return None;
            }
            n / d
        } else {
            part.parse::<f64>().ok()?
        };
        total += value + frac;
        any = true;
    }
    if any && total.is_finite() && total >= 0.0 {
        Some(total)
    } else {
        None
    }
}

fn unicode_fraction(c: char) -> Option<f64> {
    Some(match c {
        '¼' => 0.25,
        '½' => 0.5,
        '¾' => 0.75,
        '⅓' => 1.0 / 3.0,
        '⅔' => 2.0 / 3.0,
        '⅛' => 0.125,
        '⅜' => 0.375,
        '⅝' => 0.625,
        '⅞' => 0.875,
        _ => return None,
    })
}

/// Format a number the way a recipe would: 2, 2½, ⅓, 1.4 (if no nice fraction is close).
pub fn format_number(v: f64) -> String {
    if v <= 0.0 {
        return "0".into();
    }
    let whole = v.floor();
    let rest = v - whole;
    const FRACS: [(f64, &str); 9] = [
        (0.0, ""),
        (0.125, "⅛"),
        (0.25, "¼"),
        (1.0 / 3.0, "⅓"),
        (0.5, "½"),
        (2.0 / 3.0, "⅔"),
        (0.75, "¾"),
        (0.875, "⅞"),
        (1.0, ""),
    ];
    let (best, sym) = FRACS
        .iter()
        .min_by(|a, b| (a.0 - rest).abs().total_cmp(&(b.0 - rest).abs()))
        .copied()
        .unwrap();
    if (best - rest).abs() > 0.06 {
        // No fraction is close; show one decimal place.
        let s = format!("{:.1}", v);
        return s.trim_end_matches('0').trim_end_matches('.').to_string();
    }
    let whole = if best == 1.0 { whole + 1.0 } else { whole };
    match (whole as i64, sym) {
        (0, "") => "0".into(),
        (0, s) => s.into(),
        (w, s) => format!("{w}{s}"),
    }
}

/// Running total of one ingredient's amounts, possibly in several units.
#[derive(Debug, Default, Clone)]
pub struct Amount {
    volume_tsp: f64,
    volume_metric_only: bool,
    has_volume: bool,
    weight_g: f64,
    weight_metric_only: bool,
    has_weight: bool,
    /// Counts by unit ("" = plain count, "clove", "can", ...).
    counts: BTreeMap<String, f64>,
    /// Some recipe said "as needed" without an amount.
    unspecified: bool,
}

impl Amount {
    pub fn add(&mut self, quantity: Option<f64>, unit: &str) {
        let Some(q) = quantity else {
            self.unspecified = true;
            return;
        };
        match lookup(unit) {
            Some(d) if d.kind == Kind::Volume => {
                self.volume_metric_only = if self.has_volume {
                    self.volume_metric_only && d.metric
                } else {
                    d.metric
                };
                self.has_volume = true;
                self.volume_tsp += q * d.factor;
            }
            Some(d) => {
                self.weight_metric_only = if self.has_weight {
                    self.weight_metric_only && d.metric
                } else {
                    d.metric
                };
                self.has_weight = true;
                self.weight_g += q * d.factor;
            }
            None => *self.counts.entry(normalize_unit(unit)).or_default() += q,
        }
    }

    /// Human-readable total, e.g. "2⅓ tbsp", "1½ lb + 2", "as needed".
    pub fn display(&self) -> String {
        let mut parts = Vec::new();
        if self.has_volume {
            parts.push(display_volume(self.volume_tsp, self.volume_metric_only));
        }
        if self.has_weight {
            parts.push(display_weight(self.weight_g, self.weight_metric_only));
        }
        for (unit, n) in &self.counts {
            let num = format_number(*n);
            if unit.is_empty() {
                parts.push(num);
            } else if *n > 1.0 {
                parts.push(format!("{num} {}", plural(unit)));
            } else {
                parts.push(format!("{num} {unit}"));
            }
        }
        match (parts.is_empty(), self.unspecified) {
            (true, _) => "as needed".into(),
            (false, true) => format!("{} + a little extra", parts.join(" + ")),
            (false, false) => parts.join(" + "),
        }
    }
}

fn plural(unit: &str) -> String {
    if unit.ends_with('s') || unit.ends_with("sh") || unit.ends_with("ch") {
        format!("{unit}es")
    } else {
        format!("{unit}s")
    }
}

fn display_volume(tsp: f64, metric: bool) -> String {
    if metric {
        let ml = tsp / 0.202_884;
        return if ml >= 1000.0 {
            format!("{} l", format_number(ml / 1000.0))
        } else {
            format!("{} ml", ml.round())
        };
    }
    if tsp < 3.0 - 1e-9 {
        format!("{} tsp", format_number(tsp))
    } else if tsp < 12.0 - 1e-9 {
        // Under ¼ cup: tablespoons read best.
        format!("{} tbsp", format_number(tsp / 3.0))
    } else {
        format!("{} cup", format_number(tsp / 48.0))
    }
}

fn display_weight(g: f64, metric: bool) -> String {
    if metric {
        return if g >= 1000.0 {
            format!("{} kg", format_number(g / 1000.0))
        } else {
            format!("{} g", g.round())
        };
    }
    let oz = g / 28.349_5;
    if oz < 16.0 - 1e-6 {
        format!("{} oz", format_number(oz))
    } else {
        format!("{} lb", format_number(oz / 16.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn total(items: &[(Option<f64>, &str)]) -> String {
        let mut a = Amount::default();
        for (q, u) in items {
            a.add(*q, u);
        }
        a.display()
    }

    #[test]
    fn parses_quantities() {
        assert_eq!(parse_quantity("2"), Some(2.0));
        assert_eq!(parse_quantity("1.5"), Some(1.5));
        assert_eq!(parse_quantity("1/2"), Some(0.5));
        assert_eq!(parse_quantity("1 1/2"), Some(1.5));
        assert_eq!(parse_quantity("½"), Some(0.5));
        assert_eq!(parse_quantity("1½"), Some(1.5));
        assert_eq!(parse_quantity("2-3"), Some(3.0));
        assert_eq!(parse_quantity(""), None);
        assert_eq!(parse_quantity("some"), None);
        assert_eq!(parse_quantity("1/0"), None);
    }

    #[test]
    fn formats_numbers() {
        assert_eq!(format_number(2.0), "2");
        assert_eq!(format_number(2.5), "2½");
        assert_eq!(format_number(1.0 / 3.0), "⅓");
        assert_eq!(format_number(7.0 / 3.0), "2⅓");
        assert_eq!(format_number(0.98), "1");
        assert_eq!(format_number(1.4), "1.4");
    }

    #[test]
    fn normalizes_units() {
        assert_eq!(normalize_unit("Tablespoons"), "tbsp");
        assert_eq!(normalize_unit("T"), "tbsp");
        assert_eq!(normalize_unit("t"), "tsp");
        assert_eq!(normalize_unit("Cloves"), "clove");
        assert_eq!(normalize_unit("each"), "");
        assert_eq!(normalize_unit("OZ"), "oz");
    }

    #[test]
    fn adds_volumes() {
        assert_eq!(total(&[(Some(2.0), "tbsp"), (Some(1.0), "tsp")]), "2⅓ tbsp");
        assert_eq!(total(&[(Some(1.0), "tsp"), (Some(1.0), "tsp")]), "2 tsp");
        assert_eq!(total(&[(Some(2.0), "tbsp"), (Some(2.0), "tbsp")]), "¼ cup");
        assert_eq!(total(&[(Some(0.5), "cup"), (Some(4.0), "tbsp")]), "¾ cup");
    }

    #[test]
    fn adds_weights() {
        assert_eq!(total(&[(Some(8.0), "oz"), (Some(1.0), "lb")]), "1½ lb");
        assert_eq!(total(&[(Some(5.0), "oz"), (Some(5.0), "oz")]), "10 oz");
        assert_eq!(total(&[(Some(200.0), "g"), (Some(300.0), "g")]), "500 g");
    }

    #[test]
    fn adds_counts_and_mixed() {
        assert_eq!(total(&[(Some(1.0), ""), (Some(2.0), "")]), "3");
        assert_eq!(total(&[(Some(2.0), "clove"), (Some(1.0), "cloves")]), "3 cloves");
        assert_eq!(total(&[(Some(1.0), "lb"), (Some(2.0), "")]), "1 lb + 2");
        assert_eq!(total(&[(None, "")]), "as needed");
        assert_eq!(total(&[(Some(1.0), "tbsp"), (None, "")]), "1 tbsp + a little extra");
    }

    #[test]
    fn name_keys_match_plurals() {
        assert_eq!(name_key("Roma Tomatoes"), name_key("roma tomato"));
        assert_eq!(name_key("Scallions"), name_key("scallion"));
        assert_eq!(name_key("Cherries"), "cherry");
        assert_eq!(name_key("Balsamic Vinegar"), "balsamic vinegar");
    }
}
