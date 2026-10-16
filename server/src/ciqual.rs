//! Table Ciqual (Anses, Licence Ouverte Etalab 2.0) : extrait embarqué dans le binaire, chargé au démarrage
//! quand sa version change. L'extrait se régénère avec `scripts/ciqual-extract.mjs`.

use sqlx::PgPool;

use crate::text::search_key;

const EXTRACT: &str = include_str!("../data/ciqual.tsv");

/// Nombre de teneurs par ligne : énergie, protéines, glucides, sucres, lipides, AG saturés, fibres, sel.
const AMOUNTS: usize = 8;

/// Version de l'extrait (ligne `version`).
pub fn version() -> &'static str {
    EXTRACT
        .lines()
        .find_map(|line| line.strip_prefix("version\t"))
        .expect("version de l'extrait Ciqual")
}

#[derive(Default)]
struct Columns {
    code: Vec<i32>,
    name: Vec<String>,
    search_key: Vec<String>,
    group: Vec<String>,
    subgroup: Vec<String>,
    amounts: [Vec<Option<f32>>; AMOUNTS],
}

fn parse() -> Columns {
    let mut columns = Columns::default();
    for line in EXTRACT
        .lines()
        .filter(|l| l.starts_with(|c: char| c.is_ascii_digit()))
    {
        let fields: Vec<&str> = line.split('\t').collect();
        assert_eq!(fields.len(), 4 + AMOUNTS, "ligne Ciqual : {line}");
        columns.code.push(fields[0].parse().expect("code Ciqual"));
        columns.name.push(fields[1].to_owned());
        columns.search_key.push(search_key(fields[1]));
        columns.group.push(fields[2].to_owned());
        columns.subgroup.push(fields[3].to_owned());
        for (i, amount) in fields[4..].iter().enumerate() {
            columns.amounts[i].push(amount.parse().ok());
        }
    }
    columns
}

/// Charge l'extrait si sa version n'est pas déjà en base. Renvoie `true` s'il a été chargé.
/// Les lignes qui ont disparu de la nouvelle version sont retirées, sauf celles encore reliées à un ingrédient.
pub async fn sync(db: &PgPool) -> Result<bool, sqlx::Error> {
    let version = version();
    let loaded: Option<String> =
        sqlx::query_scalar("SELECT version FROM reference_data WHERE name = 'ciqual'")
            .fetch_optional(db)
            .await?;
    if loaded.as_deref() == Some(version) {
        return Ok(false);
    }

    let c = parse();
    let mut tx = db.begin().await?;
    sqlx::query(
        "INSERT INTO foods (code, name, search_key, group_code, subgroup_code, energy_kcal, protein_g, carbs_g,
                            sugars_g, fat_g, sat_fat_g, fiber_g, salt_g)
         SELECT * FROM UNNEST($1::int[], $2::text[], $3::text[], $4::text[], $5::text[], $6::real[], $7::real[],
                              $8::real[], $9::real[], $10::real[], $11::real[], $12::real[], $13::real[])
         ON CONFLICT (code) DO UPDATE SET
            name = excluded.name, search_key = excluded.search_key, group_code = excluded.group_code,
            subgroup_code = excluded.subgroup_code, energy_kcal = excluded.energy_kcal,
            protein_g = excluded.protein_g, carbs_g = excluded.carbs_g, sugars_g = excluded.sugars_g,
            fat_g = excluded.fat_g, sat_fat_g = excluded.sat_fat_g, fiber_g = excluded.fiber_g,
            salt_g = excluded.salt_g",
    )
    .bind(&c.code)
    .bind(&c.name)
    .bind(&c.search_key)
    .bind(&c.group)
    .bind(&c.subgroup)
    .bind(&c.amounts[0])
    .bind(&c.amounts[1])
    .bind(&c.amounts[2])
    .bind(&c.amounts[3])
    .bind(&c.amounts[4])
    .bind(&c.amounts[5])
    .bind(&c.amounts[6])
    .bind(&c.amounts[7])
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "DELETE FROM foods WHERE code <> ALL($1)
         AND NOT EXISTS (SELECT 1 FROM ingredient_foods WHERE food_code = foods.code)",
    )
    .bind(&c.code)
    .execute(&mut *tx)
    .await?;
    sqlx::query(
        "INSERT INTO reference_data (name, version) VALUES ('ciqual', $1)
         ON CONFLICT (name) DO UPDATE SET version = excluded.version, loaded_at = now()",
    )
    .bind(version)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    tracing::info!(version, foods = c.code.len(), "table Ciqual chargée");
    Ok(true)
}
