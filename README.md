# recipe-picker

An app to hold all of our recipes and game-ify picking them every week.

Snap a photo of a recipe card (HelloFresh front and back works well). AI reads it into a recipe you can check and fix, and it's saved on this computer. Each week, pick the recipes you want and assign each one to **Trip 1** or **Trip 2**, and you get a shopping list split into two trips so produce doesn't go bad. Tick things off as you shop.

Everything runs locally in your browser. The only thing that goes over the internet is the photos you choose to have read by AI.

## Start it

You need Rust (`brew install rust`).

```bash
cp config.example.toml config.toml   # first time only
cargo run --release
```

Then open **http://localhost:3000**. Press `Ctrl+C` in the terminal to stop it.

The first build takes a few minutes. After that it starts in seconds.

## Set up the AI (for reading photos)

Pick **one** of these and set `provider` under `[ai]` in `config.toml`. You can switch any time: changes apply straight away, no restart needed. **Settings** in the app shows what's active and has a **Test connection** button.

API keys go in a `.env` file (copy `.env.example` to `.env`). `config.toml` and `.env` are both ignored by git, so keys never get committed. A key in `.env` is used before any key with the same name set in your terminal.

### OpenAI with an API key

```toml
[ai]
provider = "openai"

[ai.openai]
model = "gpt-4.1"        # any model that can read images
```

In `.env`:

```
OPENAI_API_KEY=sk-...
```

### Azure OpenAI with an API key

```toml
[ai]
provider = "azure_key"

[ai.azure]
endpoint = "https://YOUR-RESOURCE.openai.azure.com"
deployment = "YOUR-DEPLOYMENT-NAME"
```

In `.env`:

```
AZURE_OPENAI_API_KEY=...
```

### Azure OpenAI with your Azure login (Entra ID / RBAC)

No key needed. Fill in `endpoint` and `deployment` as above with `provider = "azure_entra"`, then sign in:

```bash
az login
```

Your account needs the **Cognitive Services OpenAI User** role on the Azure OpenAI resource. If you have several tenants, set `tenant = "..."` under `[ai.azure]`.

> The app uses whichever account `az` is signed in to. Check **Settings** to see which one that is.

You don't need AI at all to type recipes in by hand (**Add manually**).

## Using it

| Screen | What it's for |
|---|---|
| **Recipes** | All your recipes. Search by title, ingredient or step. |
| **Add from photo** | Upload photos. Pick 2 or 4 people (which column of the card to read). Then check what the AI read and save. Amounts it **estimated** (e.g. "1 packet" turned into "~2 tbsp") are highlighted. |
| **Add manually** | Type a recipe in. Amounts can be `2`, `1.5`, `1/2` or `1 1/2`. |
| **This week** | Choose recipes and give each one Trip 1 or Trip 2. |
| **Shopping list** | Two trips, grouped by aisle. Tick **have** for what's already at home and **got** as it goes in the cart. Add your own items too. |
| **Export** | Download everything as a `.zip` (JSON, readable Markdown and photos), or import one back. |

**How the two trips work:**
- **Fresh items** (produce, meat & seafood, dairy & eggs, bakery) are bought on the trip of the recipe that uses them.
- **Everything else** (pantry, spices, frozen) is bought once, on Trip 1, with the week's total. You see e.g. "~3 tbsp balsamic vinegar" and decide whether to buy a bottle.
- **Matching ingredients are added up**, with unit conversion (2 tbsp + 1 tsp = 2⅓ tbsp, 8 oz + 1 lb = 1½ lb).
- **The aisle on each ingredient decides its trip.** Change it on the recipe's edit screen if the AI got it wrong.

**Start a new week** (on This week) clears the picks, ticks and added items. Your recipes are kept.

## Your data

- **Where it lives:** `data/recipes.db` holds the recipes and `data/images/` holds the photos. Both stay on this computer and are ignored by git.
- **Backups:** use **Export** now and then. Importing skips recipes you already have, so importing the same file twice is safe.

## For development

```bash
cargo test
```

`RECIPE_PICKER_PORT` and `RECIPE_PICKER_DATA_DIR` override the port and data folder, which is handy for trying things without touching your real recipes.

Code map: `src/web/` (one file per screen, templates in `templates/`), `src/shopping.rs` (two-trip list), `src/units.rs` (amounts and unit math), `src/ai/` (photo reading, the prompt and answer format), `src/export.rs`, `src/db.rs`.
