//! L'INSEE : prix moyens de vente de détail et indices des prix, par idbank, depuis l'API SDMX de la Banque de
//! données macro-économiques (XML lu sans bibliothèque : seuls les attributs des `<Obs …/>` servent).

use crate::{
    http::{HttpClient, get_text},
    menus::Day,
};

use super::crossing::IndexPoint;

/// Les `last` derniers points d'une série, du plus récent au plus ancien.
pub async fn series(
    http: &HttpClient,
    base: &str,
    idbank: &str,
    last: u32,
) -> Result<Vec<IndexPoint>, String> {
    let xml = get_text(
        http,
        &format!("{base}/data/SERIES_BDM/{idbank}?lastNObservations={last}"),
        super::SOURCE_TIMEOUT,
    )
    .await?;
    Ok(points(&xml))
}

fn points(xml: &str) -> Vec<IndexPoint> {
    let mut points: Vec<IndexPoint> = xml
        .split("<Obs ")
        .skip(1)
        .filter_map(|rest| {
            let tag = &rest[..rest.find('>')?];
            let month = Day::parse(&format!("{}-01", attribute(tag, "TIME_PERIOD")?))?;
            let value = attribute(tag, "OBS_VALUE")?.parse::<f64>().ok()?;
            value.is_finite().then_some(IndexPoint { month, value })
        })
        .collect();
    points.sort_by_key(|p| std::cmp::Reverse(p.month));
    points
}

fn attribute<'a>(tag: &'a str, name: &str) -> Option<&'a str> {
    let start = tag.find(&format!("{name}=\""))? + name.len() + 2;
    let len = tag[start..].find('"')?;
    Some(&tag[start..start + len])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lit_les_observations() {
        let xml = r#"<Series IDBANK="000641422"><Obs TIME_PERIOD="2026-08" OBS_VALUE="2.21" OBS_STATUS="A"/><Obs TIME_PERIOD="2026-09" OBS_VALUE="2.3" OBS_STATUS="A"/><Obs TIME_PERIOD="2026-07" OBS_VALUE="NaN"/></Series>"#;
        let points = points(xml);
        assert_eq!(points.len(), 2);
        assert_eq!(points[0].month, Day::parse("2026-09-01").unwrap());
        assert_eq!(points[0].value, 2.3);
    }
}
