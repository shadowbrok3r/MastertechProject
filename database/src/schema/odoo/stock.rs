//! On-hand Odoo stock per store cage and warehouse shelf, and catalog search with per-store counts.

use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use super::parts::{self, words_or_fields_domain};
use crate::schema::Store;

/// Odoo location id of `WAR/Stock`.
pub const WAR_STOCK_LOCATION: i32 = 8;

/// Products a catalog search returns at most.
const CATALOG_LIMIT: u32 = 8;

/// Products an on-hand listing names at most.
const LISTING_LIMIT: usize = 50;

/// Whether `name` is a shelf bin such as `A1-S04-R2` or `A1-EC-R3`.
pub fn is_shelf_bin(name: &str) -> bool {
    let number = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let mut parts = name.split('-');
    match (parts.next(), parts.next(), parts.next(), parts.next()) {
        (Some(aisle), Some(section), Some(row), None) => {
            aisle.strip_prefix('A').is_some_and(number)
                && (section == "EC" || section.strip_prefix('S').is_some_and(number))
                && row.strip_prefix('R').is_some_and(number)
        }
        _ => false,
    }
}

/// Whether a location counts as warehouse stock: `WAR/Stock` or one of its shelf bins.
pub fn is_warehouse_location(location_id: i32, complete_name: &str) -> bool {
    location_id == WAR_STOCK_LOCATION || complete_name.strip_prefix("WAR/Stock/").is_some_and(is_shelf_bin)
}

/// The store a location's stock counts toward: a store cage, or [`Store::WAR`] for the warehouse shelves.
pub fn location_store(location_id: i32, complete_name: &str) -> Option<Store> {
    Store::try_from_odoo_store_id(&location_id.to_string())
        .or_else(|| is_warehouse_location(location_id, complete_name).then_some(Store::WAR))
}

/// Where an on-hand listing counts stock.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scope {
    /// One store cage, or the warehouse shelves for [`Store::WAR`].
    Store(Store),
    /// Every store cage and the warehouse shelves.
    All,
}

impl Scope {
    /// RIV, LTN, MUR, SAN, ORE, WAR or ALL, in any case.
    pub fn parse(code: &str) -> Option<Self> {
        if code.trim().eq_ignore_ascii_case("all") { Some(Self::All) } else { Store::from_code(code).map(Self::Store) }
    }

    fn label(&self) -> &str {
        match self {
            Self::Store(store) => store.as_str(),
            Self::All => "ALL",
        }
    }

    /// The locations counted, in words.
    fn counted(self) -> String {
        match self {
            Self::Store(Store::WAR) => "WAR/Stock and its shelf bins".to_string(),
            Self::Store(store) => format!("the {} store cage", store.as_str()),
            Self::All => "the five store cages and the warehouse shelves (WAR)".to_string(),
        }
    }

    fn covers(self, store: Store) -> bool {
        match self {
            Self::Store(s) => s == store,
            Self::All => true,
        }
    }

    /// Domain terms selecting this scope's locations; [`location_store`] drops the non-shelf warehouse bins.
    fn location_terms(self) -> Vec<Value> {
        let warehouse = json!(["location_id", "child_of", WAR_STOCK_LOCATION]);
        match self {
            Self::Store(Store::WAR) => vec![warehouse],
            Self::Store(store) => vec![json!(["location_id", "=", store.into_odoo_store_id().unwrap_or(0)])],
            Self::All => {
                let cages: Vec<i32> = Store::RETAIL.iter().filter_map(Store::into_odoo_store_id).collect();
                vec![json!("|"), json!(["location_id", "in", cages]), warehouse]
            }
        }
    }
}

/// An Odoo product category.
#[derive(Clone, Debug, PartialEq)]
pub struct Category {
    pub id: i64,
    /// Full path, e.g. `All / Saleable Stock / GPUs`.
    pub name: String,
}

/// Categories whose own name contains `name`.
async fn categories_named(name: &str) -> anyhow::Result<Vec<Category>> {
    let rows = parts::search_read(
        "product.category",
        json!([[["name", "ilike", name.trim()]]]),
        json!(["id", "complete_name"]),
        None,
    )
    .await?;
    Ok(rows
        .iter()
        .filter_map(|r| {
            Some(Category { id: r.get("id")?.as_i64()?, name: r.get("complete_name")?.as_str()?.to_string() })
        })
        .collect())
}

/// Every category's own name, sorted, without repeats.
async fn category_names() -> anyhow::Result<Vec<String>> {
    let rows = parts::search_read("product.category", json!([[]]), json!(["name"]), None).await?;
    let mut names: Vec<String> =
        rows.iter().filter_map(|r| r.get("name")?.as_str().map(str::to_string)).filter(|n| n != "All").collect();
    names.sort_by_key(|n| n.to_lowercase());
    names.dedup();
    Ok(names)
}

/// Positive quants in `scope`, narrowed to `category_ids` and to products matching `query`.
pub fn listing_domain(scope: Scope, category_ids: &[i64], query: &str) -> Value {
    let mut terms = vec![json!(["quantity", ">", 0])];
    terms.extend(scope.location_terms());
    if !category_ids.is_empty() {
        terms.push(json!(["product_id.categ_id", "child_of", category_ids]));
    }
    if !query.trim().is_empty() {
        terms.extend(words_or_fields_domain(
            "product_id.name",
            query,
            &["product_id.default_code", "product_id.barcode"],
        ));
    }
    Value::Array(terms)
}

/// On-hand units of one product, per store.
#[derive(Clone, Debug, PartialEq)]
pub struct Holding {
    pub product_id: i64,
    /// Odoo display name, `[CODE] Name`.
    pub product: String,
    pub on_hand: BTreeMap<Store, f64>,
    pub reserved: f64,
}

impl Holding {
    pub fn total(&self) -> f64 {
        self.on_hand.values().sum()
    }
}

/// The id and name of a many2one `[id, "name"]` pair.
fn m2o(v: Option<&Value>) -> Option<(i64, &str)> {
    let pair = v?.as_array()?;
    Some((pair.first()?.as_i64()?, pair.get(1)?.as_str()?))
}

/// Sums quant rows grouped by product and location over the locations `scope` counts; most units first.
pub fn tally(scope: Scope, rows: &[Value]) -> Vec<Holding> {
    let mut by_product: BTreeMap<i64, Holding> = BTreeMap::new();
    for row in rows {
        let (Some((product_id, product)), Some((location_id, location))) =
            (m2o(row.get("product_id")), m2o(row.get("location_id")))
        else {
            continue;
        };
        let Some(store) =
            i32::try_from(location_id).ok().and_then(|id| location_store(id, location)).filter(|s| scope.covers(*s))
        else {
            continue;
        };
        let sum = |field: &str| row.get(field).and_then(Value::as_f64).unwrap_or(0.0);
        let holding = by_product.entry(product_id).or_insert_with(|| Holding {
            product_id,
            product: product.to_string(),
            on_hand: BTreeMap::new(),
            reserved: 0.0,
        });
        *holding.on_hand.entry(store).or_default() += sum("quantity");
        holding.reserved += sum("reserved_quantity");
    }
    let mut holdings: Vec<Holding> = by_product.into_values().filter(|h| h.total() > 0.0).collect();
    holdings.sort_by(|a, b| b.total().total_cmp(&a.total()).then_with(|| a.product.cmp(&b.product)));
    holdings
}

/// A quantity as JSON; whole numbers carry no fraction.
fn qty(n: f64) -> Value {
    if n.fract() == 0.0 && n.abs() < 1e15 { json!(n as i64) } else { json!(n) }
}

/// Units per store code.
fn store_units(units: &BTreeMap<Store, f64>) -> Value {
    Value::Object(units.iter().map(|(store, n)| (store.as_str().to_string(), qty(*n))).collect())
}

/// Category paths and the query echoed into a report.
fn criteria(out: &mut Map<String, Value>, categories: &[Category], query: &str) {
    if !categories.is_empty() {
        out.insert("category".into(), json!(categories.iter().map(|c| c.name.as_str()).collect::<Vec<_>>()));
    }
    if !query.trim().is_empty() {
        out.insert("query".into(), json!(query.trim()));
    }
}

/// An on-hand listing as tool output: totals over every match, then the `limit` products with the most units.
pub fn listing_report(scope: Scope, categories: &[Category], query: &str, holdings: &[Holding], limit: usize) -> Value {
    let mut out = Map::new();
    out.insert("store".into(), json!(scope.label()));
    out.insert("counted".into(), json!(scope.counted()));
    criteria(&mut out, categories, query);
    let shown = &holdings[..holdings.len().min(limit)];
    match scope {
        Scope::Store(_) => {
            out.insert("units".into(), qty(holdings.iter().map(Holding::total).sum()));
            out.insert("products".into(), json!(holdings.len()));
            let items: Vec<Value> = shown
                .iter()
                .map(|h| {
                    json!({
                        "product_id": h.product_id,
                        "product": h.product,
                        "on_hand": qty(h.total()),
                        "reserved": qty(h.reserved),
                    })
                })
                .collect();
            out.insert("items".into(), Value::Array(items));
        }
        Scope::All => {
            let mut units: BTreeMap<Store, f64> = BTreeMap::new();
            for (store, n) in holdings.iter().flat_map(|h| h.on_hand.iter()) {
                *units.entry(*store).or_default() += n;
            }
            out.insert("units".into(), store_units(&units));
            out.insert("total_units".into(), qty(units.values().sum()));
            out.insert("products".into(), json!(holdings.len()));
            let items: Vec<Value> = shown
                .iter()
                .map(|h| {
                    json!({
                        "product_id": h.product_id,
                        "product": h.product,
                        "on_hand": store_units(&h.on_hand),
                        "total": qty(h.total()),
                        "reserved": qty(h.reserved),
                    })
                })
                .collect();
            out.insert("items".into(), Value::Array(items));
        }
    }
    if holdings.len() > shown.len() {
        out.insert(
            "note".into(),
            json!(format!(
                "Showing the {} of {} products with the most units; narrow with category or query.",
                shown.len(),
                holdings.len()
            )),
        );
    }
    Value::Object(out)
}

/// A catalog product with its company-wide quantities.
#[derive(Clone, Debug, PartialEq)]
pub struct CatalogProduct {
    pub id: i64,
    /// Odoo display name, `[CODE] Name`.
    pub product: String,
    pub price: f64,
    pub cost: f64,
    /// Every Odoo location, build and RMA bins included.
    pub on_hand: f64,
    pub forecast: f64,
}

/// `product.product` domain terms for a catalog search.
fn catalog_terms(query: &str, category_ids: &[i64]) -> Vec<Value> {
    let mut terms = Vec::new();
    if !category_ids.is_empty() {
        terms.push(json!(["categ_id", "child_of", category_ids]));
    }
    if !query.trim().is_empty() {
        terms.extend(words_or_fields_domain("name", query, &["default_code", "barcode"]));
    }
    terms
}

fn catalog_product(r: &Value) -> Option<CatalogProduct> {
    let num = |k: &str| r.get(k).and_then(Value::as_f64).unwrap_or(0.0);
    Some(CatalogProduct {
        id: r.get("id")?.as_i64()?,
        product: r.get("display_name")?.as_str()?.to_string(),
        price: num("lst_price"),
        cost: num("standard_price"),
        on_hand: num("qty_available"),
        forecast: num("virtual_available"),
    })
}

/// Products matching `query` and `category_ids`, those with stock anywhere first; at most `limit`.
async fn catalog_search(query: &str, category_ids: &[i64], limit: u32) -> anyhow::Result<Vec<CatalogProduct>> {
    let fields = json!(["id", "display_name", "lst_price", "standard_price", "qty_available", "virtual_available"]);
    let base = catalog_terms(query, category_ids);
    let mut stocked = base.clone();
    stocked.push(json!(["qty_available", ">", 0]));
    let mut rows = parts::search_read("product.product", json!([stocked]), fields.clone(), Some(limit)).await?;
    let found = u32::try_from(rows.len()).unwrap_or(limit);
    if found < limit {
        let seen: Vec<i64> = rows.iter().filter_map(|r| r.get("id")?.as_i64()).collect();
        let mut rest = base;
        rest.push(json!(["id", "not in", seen]));
        rows.extend(parts::search_read("product.product", json!([rest]), fields, Some(limit - found)).await?);
    }
    Ok(rows.iter().filter_map(catalog_product).collect())
}

/// Catalog matches as tool output, each with on-hand per store cage and the warehouse shelves.
pub fn catalog_report(
    categories: &[Category],
    query: &str,
    products: &[CatalogProduct],
    holdings: &[Holding],
) -> Value {
    let items: Vec<Value> = products
        .iter()
        .map(|p| {
            let held = holdings.iter().find(|h| h.product_id == p.id);
            let on_hand: BTreeMap<Store, f64> = Store::VALUES
                .iter()
                .map(|s| (*s, held.and_then(|h| h.on_hand.get(s)).copied().unwrap_or(0.0)))
                .collect();
            json!({
                "product_id": p.id,
                "product": p.product,
                "price": p.price,
                "cost": p.cost,
                "on_hand": store_units(&on_hand),
                "company_on_hand": qty(p.on_hand),
                "company_forecast": qty(p.forecast),
            })
        })
        .collect();
    let mut out = Map::new();
    criteria(&mut out, categories, query);
    out.insert("count".into(), json!(items.len()));
    out.insert("products".into(), Value::Array(items));
    Value::Object(out)
}

/// Quant rows summed per product and location.
async fn grouped_quants(domain: Value) -> anyhow::Result<Vec<Value>> {
    parts::read_group(
        "stock.quant",
        domain,
        json!(["quantity:sum", "reserved_quantity:sum"]),
        json!(["product_id", "location_id"]),
    )
    .await
}

/// On-hand units of `product_ids` at every store cage and the warehouse shelves.
async fn holdings_of(product_ids: &[i64]) -> anyhow::Result<Vec<Holding>> {
    if product_ids.is_empty() {
        return Ok(Vec::new());
    }
    let mut terms = vec![json!(["quantity", ">", 0]), json!(["product_id", "in", product_ids])];
    terms.extend(Scope::All.location_terms());
    Ok(tally(Scope::All, &grouped_quants(Value::Array(terms)).await?))
}

/// Why an inventory lookup has no answer.
#[derive(Debug, thiserror::Error)]
pub enum LookupError {
    /// Unknown store or category, or nothing to look for.
    #[error("{0}")]
    Invalid(String),
    #[error("Odoo search failed: {0:#}")]
    Odoo(anyhow::Error),
}

impl From<anyhow::Error> for LookupError {
    fn from(e: anyhow::Error) -> Self {
        Self::Odoo(e)
    }
}

/// With `store`, what is on hand there; without, a catalog search with per-store counts.
pub async fn lookup(store: Option<&str>, category: Option<&str>, query: &str) -> Result<Value, LookupError> {
    let scope = match store.map(str::trim).filter(|s| !s.is_empty()) {
        Some(code) => Some(Scope::parse(code).ok_or_else(|| {
            LookupError::Invalid(format!("unknown store `{code}`; use RIV, LTN, MUR, SAN, ORE, WAR or ALL"))
        })?),
        None => None,
    };
    let categories = match category.map(str::trim).filter(|c| !c.is_empty()) {
        Some(name) => {
            let found = categories_named(name).await?;
            if found.is_empty() {
                let names = category_names().await?;
                return Err(LookupError::Invalid(format!(
                    "no Odoo category is named like `{name}`; categories: {}",
                    names.join(", ")
                )));
            }
            found
        }
        None => Vec::new(),
    };
    let category_ids: Vec<i64> = categories.iter().map(|c| c.id).collect();
    let query = query.trim();
    let unfiltered = query.is_empty() && category_ids.is_empty();
    match scope {
        Some(Scope::All | Scope::Store(Store::WAR)) if unfiltered => {
            Err(LookupError::Invalid("give a category or a query to list WAR or ALL".into()))
        }
        Some(scope) => {
            let rows = grouped_quants(listing_domain(scope, &category_ids, query)).await?;
            Ok(listing_report(scope, &categories, query, &tally(scope, &rows), LISTING_LIMIT))
        }
        None if unfiltered => Err(LookupError::Invalid("give a query, a category or a store".into())),
        None => {
            let products = catalog_search(query, &category_ids, CATALOG_LIMIT).await?;
            let ids: Vec<i64> = products.iter().map(|p| p.id).collect();
            Ok(catalog_report(&categories, query, &products, &holdings_of(&ids).await?))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(product: (i64, &str), location: (i64, &str), quantity: f64, reserved: f64) -> Value {
        json!({
            "product_id": [product.0, product.1],
            "location_id": [location.0, location.1],
            "quantity": quantity,
            "reserved_quantity": reserved,
            "__count": 1,
        })
    }

    #[test]
    fn shelf_bins_are_aisle_section_row_names() {
        for name in ["A0-S01-R1", "A1-S04-R2", "A7-S07-R3", "A1-EC-R3"] {
            assert!(is_shelf_bin(name), "{name}");
        }
        for name in [
            "AMD",
            "Accesories",
            "BSDB-BUILT",
            "OREM-CASES",
            "A1-S04",
            "A1-S04-R2-X",
            "A1-SX-R2",
            "B1-S04-R2",
            "A1-S04-R",
        ] {
            assert!(!is_shelf_bin(name), "{name}");
        }
    }

    #[test]
    fn warehouse_is_war_stock_and_its_shelf_bins() {
        assert!(is_warehouse_location(8, "WAR/Stock"));
        assert!(is_warehouse_location(212, "WAR/Stock/A1-S04-R2"));
        assert!(!is_warehouse_location(84, "WAR/Stock"));
        assert!(!is_warehouse_location(90, "WAR/Stock/Build"));
        assert!(!is_warehouse_location(347, "WAR/LTN Cage/A0-S01-R1"));
        assert!(!is_warehouse_location(76, "WAR/RIV Cage"));
    }

    #[test]
    fn locations_count_toward_their_cage_or_the_warehouse() {
        assert_eq!(location_store(76, "WAR/RIV Cage"), Some(Store::RIV));
        assert_eq!(location_store(77, "WAR/Sandy Cage"), Some(Store::SAN));
        assert_eq!(location_store(8, "WAR/Stock"), Some(Store::WAR));
        assert_eq!(location_store(212, "WAR/Stock/A3-S06-R2"), Some(Store::WAR));
        assert_eq!(location_store(192, "WAR/RIV Cage/Debit"), None);
        assert_eq!(location_store(90, "WAR/Stock/Build"), None);
    }

    #[test]
    fn scopes_parse_store_codes_and_all() {
        assert_eq!(Scope::parse(" riv "), Some(Scope::Store(Store::RIV)));
        assert_eq!(Scope::parse("war"), Some(Scope::Store(Store::WAR)));
        assert_eq!(Scope::parse("ALL"), Some(Scope::All));
        assert_eq!(Scope::parse("Riverdale"), None);
        assert_eq!(Scope::parse("Unknown"), None);
    }

    #[test]
    fn listing_domains_select_the_scope_category_and_query() {
        assert_eq!(
            listing_domain(Scope::Store(Store::RIV), &[513], ""),
            json!([["quantity", ">", 0], ["location_id", "=", 76], ["product_id.categ_id", "child_of", [513]]])
        );
        assert_eq!(
            listing_domain(Scope::Store(Store::WAR), &[], "RTX 5070"),
            json!([
                ["quantity", ">", 0],
                ["location_id", "child_of", 8],
                "|",
                "|",
                "&",
                ["product_id.name", "ilike", "RTX"],
                ["product_id.name", "ilike", "5070"],
                ["product_id.default_code", "ilike", "RTX 5070"],
                ["product_id.barcode", "ilike", "RTX 5070"]
            ])
        );
        assert_eq!(
            listing_domain(Scope::All, &[513, 539], ""),
            json!([
                ["quantity", ">", 0],
                "|",
                ["location_id", "in", [76, 73, 74, 75, 77]],
                ["location_id", "child_of", 8],
                ["product_id.categ_id", "child_of", [513, 539]]
            ])
        );
    }

    #[test]
    fn tally_sums_per_product_over_counted_locations_most_units_first() {
        let rtx5070 = (12633, "[GPU/RTX5070] NVIDIA GEFORCE RTX 5070 12GB");
        let rtx5050 = (12832, "[GPU/RTX5050] NVIDIA RTX 5050 8GB");
        let rows = vec![
            row(rtx5050, (76, "WAR/RIV Cage"), 4.0, 1.0),
            row(rtx5070, (76, "WAR/RIV Cage"), 2.0, 0.0),
            row(rtx5070, (192, "WAR/RIV Cage/Debit"), 5.0, 0.0),
            row(rtx5070, (8, "WAR/Stock"), 3.0, 0.0),
            row(rtx5070, (212, "WAR/Stock/A3-S06-R2"), 4.0, 2.0),
            row(rtx5070, (90, "WAR/Stock/Build"), 9.0, 0.0),
            json!({ "product_id": false, "location_id": [76, "WAR/RIV Cage"], "quantity": 9.0 }),
        ];

        let riv = tally(Scope::Store(Store::RIV), &rows);
        assert_eq!(
            riv.iter().map(|h| (h.product_id, h.total(), h.reserved)).collect::<Vec<_>>(),
            [(12832, 4.0, 1.0), (12633, 2.0, 0.0)]
        );

        let warehouse = tally(Scope::Store(Store::WAR), &rows);
        assert_eq!(warehouse.len(), 1);
        assert_eq!(warehouse[0].on_hand, BTreeMap::from([(Store::WAR, 7.0)]));
        assert_eq!(warehouse[0].reserved, 2.0);

        let all = tally(Scope::All, &rows);
        assert_eq!(all[0].product_id, 12633);
        assert_eq!(all[0].on_hand, BTreeMap::from([(Store::RIV, 2.0), (Store::WAR, 7.0)]));
        assert_eq!(all[1].on_hand, BTreeMap::from([(Store::RIV, 4.0)]));
    }

    #[test]
    fn a_store_listing_leads_with_totals_and_caps_the_items() {
        let rows = vec![
            row((1, "[GPU/A] A"), (76, "WAR/RIV Cage"), 4.0, 0.0),
            row((2, "[GPU/B] B"), (76, "WAR/RIV Cage"), 3.0, 1.0),
            row((3, "[GPU/C] C"), (76, "WAR/RIV Cage"), 1.0, 0.0),
        ];
        let gpus = [Category { id: 513, name: "All / Saleable Stock / GPUs".into() }];
        let scope = Scope::Store(Store::RIV);
        let report = listing_report(scope, &gpus, "", &tally(scope, &rows), 2);
        assert_eq!(report["store"], "RIV");
        assert_eq!(report["category"], json!(["All / Saleable Stock / GPUs"]));
        assert_eq!(report["units"], json!(8));
        assert_eq!(report["products"], json!(3));
        assert_eq!(report["items"][0], json!({ "product_id": 1, "product": "[GPU/A] A", "on_hand": 4, "reserved": 0 }));
        assert_eq!(report["items"].as_array().map(Vec::len), Some(2));
        assert!(report["note"].as_str().is_some_and(|n| n.contains("2 of 3")), "{report}");
        assert!(report.get("query").is_none());

        let everything = listing_report(scope, &gpus, "", &tally(scope, &rows), 50);
        assert!(everything.get("note").is_none());
    }

    #[test]
    fn an_all_stores_listing_counts_units_per_store() {
        let rows = vec![
            row((1, "[GPU/A] A"), (76, "WAR/RIV Cage"), 4.0, 0.0),
            row((1, "[GPU/A] A"), (8, "WAR/Stock"), 10.0, 2.0),
            row((2, "[GPU/B] B"), (77, "WAR/Sandy Cage"), 1.5, 0.0),
        ];
        let report = listing_report(Scope::All, &[], "RTX", &tally(Scope::All, &rows), 50);
        assert_eq!(report["units"], json!({ "RIV": 4, "SAN": 1.5, "WAR": 10 }));
        assert_eq!(report["total_units"], json!(15.5));
        assert_eq!(report["query"], "RTX");
        assert_eq!(report["items"][0]["on_hand"], json!({ "RIV": 4, "WAR": 10 }));
        assert_eq!(report["items"][0]["total"], json!(14));
        assert_eq!(report["items"][0]["reserved"], json!(2));
    }

    #[test]
    fn catalog_rows_list_every_store_with_zeros() {
        let products = [CatalogProduct {
            id: 7,
            product: "[GPU/RTX3060] Nvidia RTX 3060 12GB".into(),
            price: 299.0,
            cost: 210.5,
            on_hand: 50.0,
            forecast: 80.0,
        }];
        let held = tally(
            Scope::All,
            &[row((7, "[GPU/RTX3060] Nvidia RTX 3060 12GB"), (212, "WAR/Stock/A3-S06-R2"), 41.0, 0.0)],
        );
        let report = catalog_report(&[], "RTX 3060", &products, &held);
        assert_eq!(report["count"], json!(1));
        let first = &report["products"][0];
        assert_eq!(first["on_hand"], json!({ "RIV": 0, "LTN": 0, "MUR": 0, "ORE": 0, "SAN": 0, "WAR": 41 }));
        assert_eq!(first["company_on_hand"], json!(50));
        assert_eq!(first["company_forecast"], json!(80));
    }

    #[test]
    fn catalog_terms_add_the_category_and_the_name_match() {
        assert_eq!(catalog_terms("", &[513]), vec![json!(["categ_id", "child_of", [513]])]);
        assert_eq!(
            catalog_terms("5070", &[]),
            vec![
                json!("|"),
                json!("|"),
                json!(["name", "ilike", "5070"]),
                json!(["default_code", "ilike", "5070"]),
                json!(["barcode", "ilike", "5070"])
            ]
        );
    }

    #[tokio::test]
    async fn a_bad_store_or_an_empty_request_is_refused_before_odoo() {
        let bad = lookup(Some("Riverdale"), None, "").await.unwrap_err();
        assert!(matches!(&bad, LookupError::Invalid(m) if m.contains("RIV, LTN")), "{bad}");
        assert!(matches!(lookup(None, None, "  ").await, Err(LookupError::Invalid(_))));
        assert!(matches!(lookup(Some("all"), None, "").await, Err(LookupError::Invalid(_))));
        assert!(matches!(lookup(Some("WAR"), Some(" "), "").await, Err(LookupError::Invalid(_))));
    }

    #[tokio::test]
    #[ignore = "reads live Odoo"]
    async fn live_gpus_on_hand() {
        let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();
        for (store, category, query) in
            [(Some("RIV"), Some("GPUs"), ""), (Some("ALL"), Some("GPU"), "5070"), (None, None, "RTX 3060")]
        {
            let report = lookup(store, category, query).await.expect("lookup");
            println!("{store:?} {category:?} {query:?}\n{report:#}\n");
        }
        let miss = lookup(Some("RIV"), Some("graphics card"), "").await.unwrap_err();
        println!("{miss}");
        assert!(matches!(miss, LookupError::Invalid(m) if m.contains("GPUs")));
    }
}
