//! Open Prices (Open Food Facts, ODbL) : relevés de prix en magasin, en vrac par catégorie, ou de produits à
//! code-barres d'une catégorie.

use std::time::Duration;

use serde_json::Value;

use crate::{
    http::{HttpClient, get_json_within},
    menus::Day,
};

use super::{PriceLink, PriceUnit, crossing::Observation};

/// Relevés lus par page, et pages lues au plus pour une catégorie.
const PAGE_SIZE: u32 = 50;
const MAX_PAGES: u32 = 20;

/// Relevés en France depuis `since`, au kilo, au litre ou à la pièce (les produits ramenés par leur contenance).
/// `pause` sépare deux requêtes.
pub async fn observations(
    http: &HttpClient,
    base: &str,
    link: &PriceLink,
    since: Day,
    pause: Duration,
) -> Result<Vec<Observation>, String> {
    let mut found = Vec::new();
    let searches = [
        ("category_tag", link.off_category.as_deref()),
        (
            "product__categories_tags__contains",
            link.off_product_category.as_deref(),
        ),
    ];
    for (param, tag) in searches {
        let Some(tag) = tag else { continue };
        for page in 1..=MAX_PAGES {
            let query = serde_urlencoded::to_string([
                (param, tag),
                ("currency", "EUR"),
                ("date__gte", &since.to_string()),
                ("order_by", "-date"),
                ("size", &PAGE_SIZE.to_string()),
                ("page", &page.to_string()),
            ])
            .map_err(|err| err.to_string())?;
            let body = get_json_within(
                http,
                &format!("{base}/prices?{query}"),
                None,
                super::SOURCE_TIMEOUT,
            )
            .await?;
            tokio::time::sleep(pause).await;
            let items = body["items"].as_array().cloned().unwrap_or_default();
            found.extend(items.iter().filter_map(observation));
            let pages = body["pages"].as_u64().unwrap_or(1);
            if items.is_empty() || u64::from(page) >= pages {
                break;
            }
        }
    }
    Ok(found)
}

/// Un relevé de l'API, s'il est en France et qu'on sait le ramener au kilo, au litre ou à la pièce. Un prix en
/// promotion compte pour son prix d'avant la promotion quand il est donné.
fn observation(item: &Value) -> Option<Observation> {
    if item["location"]["osm_address_country"].as_str() != Some("France") {
        return None;
    }
    let euros = match item["price_without_discount"].as_f64() {
        Some(before) if item["price_is_discounted"].as_bool() == Some(true) => before,
        _ => item["price"].as_f64()?,
    };
    let date = Day::parse(item["date"].as_str()?)?;
    let product = &item["product"];
    let labels = [&item["labels_tags"], &product["labels_tags"]]
        .into_iter()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|l| l.as_str().map(str::to_owned))
        .collect();
    let (cents, per) = if product.is_object() {
        let qty = product["product_quantity"].as_f64().filter(|q| *q > 0.0)?;
        let (per, per_unit) = match product["product_quantity_unit"].as_str()? {
            "g" => (PriceUnit::Kg, 1000.0),
            "kg" => (PriceUnit::Kg, 1.0),
            "ml" => (PriceUnit::L, 1000.0),
            "cl" => (PriceUnit::L, 100.0),
            "l" | "L" => (PriceUnit::L, 1.0),
            _ => return None,
        };
        (euros * 100.0 * per_unit / qty, per)
    } else {
        let per = match item["price_per"].as_str()? {
            "KILOGRAM" => PriceUnit::Kg,
            "LITER" | "LITRE" => PriceUnit::L,
            "UNIT" => PriceUnit::Piece,
            _ => return None,
        };
        (euros * 100.0, per)
    };
    Some(Observation {
        cents,
        per,
        date,
        labels,
    })
}
