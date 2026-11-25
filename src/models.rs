use serde::{Deserialize, Serialize};

/// Store aisle an ingredient belongs to. Decides grouping on the shopping
/// list and whether an item is "fresh" (bought on its recipe's trip) or
/// shelf-stable (always bought on Trip 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    Produce,
    MeatSeafood,
    DairyEggs,
    Bakery,
    Frozen,
    Pantry,
    Spices,
    Other,
}

impl Category {
    pub const ALL: [Category; 8] = [
        Category::Produce,
        Category::MeatSeafood,
        Category::DairyEggs,
        Category::Bakery,
        Category::Frozen,
        Category::Pantry,
        Category::Spices,
        Category::Other,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Category::Produce => "produce",
            Category::MeatSeafood => "meat_seafood",
            Category::DairyEggs => "dairy_eggs",
            Category::Bakery => "bakery",
            Category::Frozen => "frozen",
            Category::Pantry => "pantry",
            Category::Spices => "spices",
            Category::Other => "other",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Category::Produce => "Produce",
            Category::MeatSeafood => "Meat & seafood",
            Category::DairyEggs => "Dairy & eggs",
            Category::Bakery => "Bakery",
            Category::Frozen => "Frozen",
            Category::Pantry => "Pantry",
            Category::Spices => "Spices",
            Category::Other => "Other",
        }
    }

    pub fn parse(s: &str) -> Category {
        Category::ALL
            .into_iter()
            .find(|c| c.as_str() == s.trim())
            .unwrap_or(Category::Other)
    }

    /// Fresh items go bad quickly, so they are bought on the trip of the
    /// recipe that uses them. Everything else is bought once, on Trip 1.
    pub fn is_fresh(self) -> bool {
        matches!(
            self,
            Category::Produce | Category::MeatSeafood | Category::DairyEggs | Category::Bakery
        )
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Ingredient {
    /// Wording as it appeared on the card, e.g. "1 packet balsamic vinegar".
    pub original: String,
    /// Clean grocery name, e.g. "balsamic vinegar". Used to combine items.
    pub name: String,
    /// None means "as needed" / no amount given.
    pub quantity: Option<f64>,
    /// tsp, tbsp, cup, oz, lb, g, ml, clove, ... or "" for a plain count.
    pub unit: String,
    pub category: Category,
    /// True when the amount was estimated (e.g. a packet turned into tbsp).
    pub estimated: bool,
    /// How the estimate was made, e.g. "1 packet ≈ 2 tbsp".
    pub note: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Recipe {
    #[serde(default)]
    pub id: i64,
    pub title: String,
    #[serde(default)]
    pub description: String,
    pub servings: i64,
    #[serde(default)]
    pub total_minutes: Option<i64>,
    #[serde(default)]
    pub source: String,
    pub ingredients: Vec<Ingredient>,
    pub steps: Vec<String>,
    #[serde(default)]
    pub notes: String,
    /// File names inside the data/images folder.
    #[serde(default)]
    pub images: Vec<String>,
}

impl Recipe {
    pub fn empty(servings: i64) -> Recipe {
        Recipe {
            id: 0,
            title: String::new(),
            description: String::new(),
            servings,
            total_minutes: None,
            source: String::new(),
            ingredients: Vec::new(),
            steps: Vec::new(),
            notes: String::new(),
            images: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Trip {
    One = 1,
    Two = 2,
}

impl Trip {
    pub fn from_i64(n: i64) -> Option<Trip> {
        match n {
            1 => Some(Trip::One),
            2 => Some(Trip::Two),
            _ => None,
        }
    }
    pub fn num(self) -> i64 {
        self as i64
    }
}

/// A recipe chosen for this week.
#[derive(Debug, Clone)]
pub struct Pick {
    pub recipe_id: i64,
    pub trip: Trip,
}

/// Something added to the list by hand (paper towels, snacks, ...).
#[derive(Debug, Clone)]
pub struct ExtraItem {
    pub id: i64,
    pub name: String,
    pub amount: String,
    pub trip: Trip,
}
