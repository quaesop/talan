use failure::Error;
use log;
use reqwest;
use serde::{Deserialize, Serialize};
use serde_json;
use std::cmp::Ordering;
use std::collections::HashMap;

const XIVAPI_SEARCH_URL: &str = "https://xivapi.com/search";

#[derive(PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct Material {
    pub id: u32,
    pub count: u32,
    pub name: String,
}

// Top level structs to export out of the library
#[derive(PartialEq, Debug, Serialize, Deserialize, Default)]
pub struct Recipe {
    pub durability: u32,
    pub difficulty: u32,
    pub quality: u32,
    pub level: u32,
    pub id: u32,
    pub index: usize,
    pub job: u32,
    pub mats: Vec<Material>,
    pub name: String,
}

fn convert_ingredient(r: &mut Recipe, ii: &Option<ItemIngredient>, amount: u32) {
    if let Some(ref ii) = ii {
        r.mats.push(Material {
            id: ii.ID,
            name: ii.Name.clone(),
            count: amount,
        });
    }
}

// Rather than From, we probably need a method to find the right recipe and fill in the offset
impl Recipe {
    fn from_results(recipes: Vec<ApiRecipe>, item_name: &str, job: u32) -> Option<Recipe> {
        // Figure
        for (i, recipe) in recipes.iter().enumerate() {
            // Items like 'Cloud Pearl' also have 'Cloud Pearl Components' in the results,
            // and can have multiple jobs. If there's more than one job in the results then
            // we should match on the one requested. But if only result comes back it means
            // the user had the wrong job selected. For ease of use in that circumstance
            // we'll just add it to the task list.
            if recipe.Name.to_lowercase() == item_name.to_lowercase()
                && (recipes.len() == 1 || recipe.CraftType.ID as u32 == job)
            {
                let mut r = Recipe {
                    level: recipe.RecipeLevelTable.ClassJobLevel,
                    durability: (recipe.RecipeLevelTable.Durability * recipe.DurabilityFactor)
                        / 100,
                    difficulty: (recipe.RecipeLevelTable.Difficulty * recipe.DifficultyFactor)
                        / 100,
                    quality: (recipe.RecipeLevelTable.Quality * recipe.QualityFactor) / 100,
                    id: recipe.ID,
                    name: recipe.Name.clone(),
                    job: recipe.CraftType.ID as u32,
                    index: i,
                    mats: Vec::new(),
                };
                convert_ingredient(&mut r, &recipe.ItemIngredient0, recipe.AmountIngredient0);
                convert_ingredient(&mut r, &recipe.ItemIngredient1, recipe.AmountIngredient1);
                convert_ingredient(&mut r, &recipe.ItemIngredient2, recipe.AmountIngredient2);
                convert_ingredient(&mut r, &recipe.ItemIngredient3, recipe.AmountIngredient3);
                convert_ingredient(&mut r, &recipe.ItemIngredient4, recipe.AmountIngredient4);
                convert_ingredient(&mut r, &recipe.ItemIngredient5, recipe.AmountIngredient5);
                return Some(r);
            }
        }
        None
    }
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct ItemIngredient {
    Name: String,
    ID: u32,
}

// These structures match the XIVApi schemas
#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct CraftType {
    ID: u32,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
struct RecipeLevelTable {
    ClassJobLevel: u32,
    Difficulty: u32,
    Durability: u32,
    ID: u32,
    Quality: u32,
    Stars: u32,
    SuggestedControl: u32,
    SuggestedCraftsmanship: u32,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct GameContentLinks {
    RecipeNotebookList: HashMap<String, Vec<u32>>,
}

#[allow(non_snake_case)]
#[derive(Clone, Debug, Deserialize)]
pub struct ApiRecipe {
    ID: u32,
    Name: String,
    CraftType: CraftType,
    RecipeLevelTable: RecipeLevelTable,
    AmountIngredient0: u32,
    AmountIngredient1: u32,
    AmountIngredient2: u32,
    AmountIngredient3: u32,
    AmountIngredient4: u32,
    AmountIngredient5: u32,
    DifficultyFactor: u32,
    DurabilityFactor: u32,
    QualityFactor: u32,
    ItemIngredient0: Option<ItemIngredient>,
    ItemIngredient1: Option<ItemIngredient>,
    ItemIngredient2: Option<ItemIngredient>,
    ItemIngredient3: Option<ItemIngredient>,
    ItemIngredient4: Option<ItemIngredient>,
    ItemIngredient5: Option<ItemIngredient>,
    GameContentLinks: Option<GameContentLinks>,
}

impl PartialEq for ApiRecipe {
    fn eq(&self, other: &Self) -> bool {
        self.ID == other.ID
    }
}

impl Eq for ApiRecipe {}

// Ord is implemented
impl Ord for ApiRecipe {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.CraftType.ID != other.CraftType.ID {
            return self.CraftType.ID.cmp(&other.CraftType.ID);
        }

        // risky
        let self_links = self.GameContentLinks.clone().unwrap();
        let other_links = other.GameContentLinks.clone().unwrap();
        let self_page = self_links.RecipeNotebookList.keys().collect::<Vec<_>>()[0];
        let other_page = other_links.RecipeNotebookList.keys().collect::<Vec<_>>()[0];

        // This needs to extract the integer from page string of the form "Recipe22"
        if self_page != other_page {
            return self_page.cmp(&other_page);
        }

        let self_index = self_links.RecipeNotebookList.get(self_page).unwrap();
        let other_index = self_links.RecipeNotebookList.get(other_page).unwrap();

        if self_index != other_index {
            return self_index.cmp(&other_index);
        }

        // fall back on sorting by ID
        self.ID.cmp(&other.ID)
    }
}

impl PartialOrd for ApiRecipe {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Deserialize)]
struct ApiPagination {}

#[allow(non_snake_case)]
#[derive(Debug, Deserialize)]
struct ApiReply<T> {
    pub Pagination: ApiPagination,
    pub Results: Vec<T>,
}

pub fn query_recipe_by_name(item_name: &str) -> Result<Vec<ApiRecipe>, Error> {
    log::trace!("Looking up '{}'", item_name);
    let columns = [
        "AmountIngredient0",
        "AmountIngredient1",
        "AmountIngredient2",
        "AmountIngredient3",
        "AmountIngredient4",
        "AmountIngredient5",
        "CraftType.ID",
        "DifficultyFactor",
        "DurabilityFactor",
        "ID",
        "ItemIngredient0",
        "ItemIngredient1",
        "ItemIngredient2",
        "ItemIngredient3",
        "ItemIngredient4",
        "ItemIngredient5",
        "Name",
        "QualityFactor",
        "RecipeLevelTable",
        "GameContentLinks",
    ];
    let s: String = columns.iter().map(|e| e.to_string() + ",").collect();
    let body = reqwest::Client::new()
        .get(XIVAPI_SEARCH_URL)
        .query(&[
            ("indexes", "Recipe"),
            ("columns", &s),
            ("string", item_name),
            ("pretty", "1"),
        ])
        .send()?
        .text()?;
    let mut r: ApiReply<ApiRecipe> = serde_json::from_str(&body)?;
    r.Results.sort();
    log::trace!("{:#?}", r.Results);
    Ok(r.Results)
}

pub fn get_recipe_for_job(item_name: &str, job: u32) -> Result<Option<Recipe>, Error> {
    Ok(Recipe::from_results(
        query_recipe_by_name(item_name)?,
        item_name,
        job,
    ))
}

#[cfg(test)]
mod test {
    use std::sync::Once;
    static START: Once = Once::new();

    fn setup() {
        START.call_once(|| {
            env_logger::init();
        });
    }

    use super::*;
    #[test]
    fn basic_get_test() -> Result<(), Error> {
        setup();

        let api_results = query_recipe_by_name("Rakshasa Axe")?;
        let item = &api_results[0];
        log::trace!("item fetched: {:#?}", item);
        assert_eq!(item.Name, "Rakshasa Axe");
        assert_eq!(item.CraftType.ID, 1);
        assert_eq!(item.AmountIngredient0, 3);
        assert_eq!(item.AmountIngredient1, 1);
        assert_eq!(item.AmountIngredient2, 1);
        assert_eq!(item.AmountIngredient3, 3);
        assert_eq!(item.AmountIngredient4, 0);
        assert_eq!(item.AmountIngredient5, 0);
        Ok(())
    }

    #[test]
    fn triphane_test() -> Result<(), Error> {
        setup();

        let api_results = query_recipe_by_name("Triphane")?;
        let item = &api_results[0];
        log::trace!("item fetched: {:#?}", item);
        assert_eq!(item.Name, "Triphane");
        Ok(())
    }

    #[test]
    fn swalloskin_gloves_test() -> Result<(), Error> {
        setup();

        let names = vec![
            "Swallowskin Gloves of Fending",
            "Swallowskin Gloves of Maiming",
            "Swallowskin Gloves of Striking",
            "Swallowskin Gloves of Scouting",
            "Swallowskin Gloves of Aiming",
            "Swallowskin Gloves of Casting",
            "Swallowskin Gloves of Healing",
            "Swallowskin Gloves",
        ];

        let api_results = query_recipe_by_name("Swallowskin Gloves")?;

        log::trace!("results: {:#?}", api_results);
        assert_eq!(api_results.len(), names.len());
        for (i, recipe) in api_results.iter().enumerate() {
            assert_eq!(recipe.Name, names[i]);
        }
        Ok(())
    }

}
