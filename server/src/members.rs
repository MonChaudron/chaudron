//! Membres du foyer (ceux qui mangent) : barème de portions, présence aux repas, exclusions.

use std::collections::{HashMap, HashSet};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::StatusCode,
    routing::get,
};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, PgPool};
use utoipa::ToSchema;
use uuid::Uuid;

use crate::{
    AppState,
    auth::CurrentUser,
    error::{ApiError, ApiResult, ErrorBody},
    households::role_in,
    types::{Diet, ExclusionKind, Meal, MemberKind},
    validate,
};

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/households/{id}/members", get(list).post(create))
        .route(
            "/households/{id}/members/{member_id}",
            get(read).put(replace).delete(remove),
        )
        .route("/portions/defaults", get(defaults))
}

/// Barème : multiplicateur par rôle d'ingrédient, appliqué aux quantités d'une portion standard.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Portions {
    pub protein: f32,
    pub starch: f32,
    pub veg: f32,
    pub fat: f32,
    pub other: f32,
}

impl Portions {
    /// Barème par défaut (design §3) : l'ado prend double féculent, l'enfant 0,6 sauf féculent.
    pub fn default_for(kind: MemberKind) -> Self {
        match kind {
            MemberKind::Adult => Self::uniform(1.0),
            MemberKind::Teen => Self {
                starch: 2.0,
                ..Self::uniform(1.0)
            },
            MemberKind::Child => Self {
                starch: 1.0,
                ..Self::uniform(0.6)
            },
        }
    }

    fn uniform(factor: f32) -> Self {
        Self {
            protein: factor,
            starch: factor,
            veg: factor,
            fat: factor,
            other: factor,
        }
    }

    fn entries(&self) -> [(&'static str, f32); 5] {
        [
            ("protein", self.protein),
            ("starch", self.starch),
            ("veg", self.veg),
            ("fat", self.fat),
            ("other", self.other),
        ]
    }

    fn set(&mut self, role: &str, factor: f32) {
        match role {
            "protein" => self.protein = factor,
            "starch" => self.starch = factor,
            "veg" => self.veg = factor,
            "fat" => self.fat = factor,
            _ => self.other = factor,
        }
    }
}

/// Présence aux repas, du lundi (indice 0) au dimanche (indice 6).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Presence {
    #[schema(min_items = 7, max_items = 7)]
    pub lunch: Vec<bool>,
    #[schema(min_items = 7, max_items = 7)]
    pub dinner: Vec<bool>,
}

impl Presence {
    fn always() -> Self {
        Self {
            lunch: vec![true; 7],
            dinner: vec![true; 7],
        }
    }

    fn day_mut(&mut self, meal: Meal) -> &mut Vec<bool> {
        match meal {
            Meal::Lunch => &mut self.lunch,
            Meal::Dinner => &mut self.dinner,
        }
    }
}

/// Ingrédient à éviter pour ce membre.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, ToSchema)]
pub struct Exclusion {
    pub ingredient_id: Uuid,
    pub kind: ExclusionKind,
}

#[derive(Serialize, ToSchema)]
pub struct Member {
    pub id: Uuid,
    pub name: String,
    pub kind: MemberKind,
    pub diet: Diet,
    /// Compte associé, si ce membre se connecte (les enfants n'en ont pas forcément).
    pub user_id: Option<Uuid>,
    pub portions: Portions,
    pub presence: Presence,
    pub exclusions: Vec<Exclusion>,
}

/// Nouveau membre : barème par défaut de son type, présent à tous les repas, sans exclusion.
#[derive(Deserialize, ToSchema)]
pub struct NewMember {
    pub name: String,
    pub kind: MemberKind,
    /// `omnivore` par défaut.
    pub diet: Option<Diet>,
    pub user_id: Option<Uuid>,
}

/// Membre complet, remplacé tel quel.
#[derive(Deserialize, ToSchema)]
pub struct MemberInput {
    pub name: String,
    pub kind: MemberKind,
    pub diet: Diet,
    pub user_id: Option<Uuid>,
    pub portions: Portions,
    pub presence: Presence,
    pub exclusions: Vec<Exclusion>,
}

#[derive(FromRow)]
struct MemberRow {
    id: Uuid,
    name: String,
    #[sqlx(try_from = "String")]
    kind: MemberKind,
    #[sqlx(try_from = "String")]
    diet: Diet,
    user_id: Option<Uuid>,
}

/// Membres du foyer (ou un seul), avec barème, présence et exclusions.
async fn load(db: &PgPool, household_id: Uuid, member_id: Option<Uuid>) -> ApiResult<Vec<Member>> {
    let rows: Vec<MemberRow> = sqlx::query_as(
        "SELECT id, name, kind, diet, user_id FROM members
         WHERE household_id = $1 AND ($2::uuid IS NULL OR id = $2) ORDER BY created_at, id",
    )
    .bind(household_id)
    .bind(member_id)
    .fetch_all(db)
    .await?;
    let ids: Vec<Uuid> = rows.iter().map(|m| m.id).collect();

    let portions: Vec<(Uuid, String, f32)> = sqlx::query_as(
        "SELECT member_id, role, factor FROM member_portions WHERE member_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let presence: Vec<(Uuid, i16, String, bool)> = sqlx::query_as(
        "SELECT member_id, weekday, meal, present FROM member_presence WHERE member_id = ANY($1)",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;
    let exclusions: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT member_id, ingredient_id, kind FROM member_exclusions
         WHERE member_id = ANY($1) ORDER BY created_at, ingredient_id",
    )
    .bind(&ids)
    .fetch_all(db)
    .await?;

    let mut members: Vec<Member> = rows
        .into_iter()
        .map(|row| Member {
            portions: Portions::default_for(row.kind),
            presence: Presence::always(),
            exclusions: Vec::new(),
            id: row.id,
            name: row.name,
            kind: row.kind,
            diet: row.diet,
            user_id: row.user_id,
        })
        .collect();
    let index: HashMap<Uuid, usize> = members.iter().enumerate().map(|(i, m)| (m.id, i)).collect();
    for (member_id, role, factor) in portions {
        members[index[&member_id]].portions.set(&role, factor);
    }
    for (member_id, weekday, meal, present) in presence {
        let meal = Meal::try_from(meal).map_err(|_| ApiError::internal())?;
        let day = usize::try_from(weekday - 1).map_err(|_| ApiError::internal())?;
        members[index[&member_id]].presence.day_mut(meal)[day] = present;
    }
    for (member_id, ingredient_id, kind) in exclusions {
        let kind = ExclusionKind::try_from(kind).map_err(|_| ApiError::internal())?;
        members[index[&member_id]].exclusions.push(Exclusion {
            ingredient_id,
            kind,
        });
    }
    Ok(members)
}

async fn load_one(db: &PgPool, household_id: Uuid, member_id: Uuid) -> ApiResult<Member> {
    load(db, household_id, Some(member_id))
        .await?
        .pop()
        .ok_or_else(ApiError::not_found)
}

fn check(input: &MemberInput) -> ApiResult<()> {
    validate::name(&input.name)?;
    let factor_ok = |f: f32| f.is_finite() && (0.0..=5.0).contains(&f);
    if !input.portions.entries().iter().all(|(_, f)| factor_ok(*f)) {
        return Err(ApiError::invalid("Les facteurs du barème vont de 0 à 5."));
    }
    if input.presence.lunch.len() != 7 || input.presence.dinner.len() != 7 {
        return Err(ApiError::invalid(
            "La présence compte 7 jours, du lundi au dimanche.",
        ));
    }
    let mut seen = HashSet::new();
    if !input
        .exclusions
        .iter()
        .all(|e| seen.insert(e.ingredient_id))
    {
        return Err(ApiError::invalid("Ingrédient exclu en double."));
    }
    Ok(())
}

/// Écrit le membre et remplace son barème, sa présence et ses exclusions.
async fn write(
    conn: &mut PgConnection,
    household_id: Uuid,
    member_id: Uuid,
    input: &MemberInput,
) -> ApiResult<()> {
    if let Some(user_id) = input.user_id {
        let (in_household, taken): (bool, bool) = sqlx::query_as(
            "SELECT
               EXISTS (SELECT 1 FROM household_users WHERE household_id = $1 AND user_id = $2),
               EXISTS (SELECT 1 FROM members WHERE household_id = $1 AND user_id = $2 AND id <> $3)",
        )
        .bind(household_id)
        .bind(user_id)
        .bind(member_id)
        .fetch_one(&mut *conn)
        .await?;
        if !in_household {
            return Err(ApiError::invalid("Ce compte ne fait pas partie du foyer."));
        }
        if taken {
            return Err(ApiError::conflict(
                "user_taken",
                "Ce compte est déjà associé à un autre membre.",
            ));
        }
    }

    sqlx::query(
        "INSERT INTO members (id, household_id, name, kind, diet, user_id) VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT (id) DO UPDATE SET name = $3, kind = $4, diet = $5, user_id = $6, updated_at = now()",
    )
    .bind(member_id)
    .bind(household_id)
    .bind(validate::name(&input.name)?)
    .bind(input.kind.as_str())
    .bind(input.diet.as_str())
    .bind(input.user_id)
    .execute(&mut *conn)
    .await?;

    for delete in [
        "DELETE FROM member_portions WHERE member_id = $1",
        "DELETE FROM member_presence WHERE member_id = $1",
        "DELETE FROM member_exclusions WHERE member_id = $1",
    ] {
        sqlx::query(delete)
            .bind(member_id)
            .execute(&mut *conn)
            .await?;
    }

    let (roles, factors): (Vec<&str>, Vec<f32>) = input.portions.entries().into_iter().unzip();
    sqlx::query(
        "INSERT INTO member_portions (member_id, role, factor)
         SELECT $1, * FROM UNNEST($2::text[], $3::real[])",
    )
    .bind(member_id)
    .bind(roles)
    .bind(factors)
    .execute(&mut *conn)
    .await?;

    let mut weekdays = Vec::with_capacity(14);
    let mut meals = Vec::with_capacity(14);
    let mut present = Vec::with_capacity(14);
    for (meal, days) in [
        (Meal::Lunch, &input.presence.lunch),
        (Meal::Dinner, &input.presence.dinner),
    ] {
        for (day, here) in (1i16..).zip(days) {
            weekdays.push(day);
            meals.push(meal.as_str());
            present.push(*here);
        }
    }
    sqlx::query(
        "INSERT INTO member_presence (member_id, weekday, meal, present)
         SELECT $1, * FROM UNNEST($2::smallint[], $3::text[], $4::boolean[])",
    )
    .bind(member_id)
    .bind(weekdays)
    .bind(meals)
    .bind(present)
    .execute(&mut *conn)
    .await?;

    let ingredients: Vec<Uuid> = input.exclusions.iter().map(|e| e.ingredient_id).collect();
    let kinds: Vec<&str> = input.exclusions.iter().map(|e| e.kind.as_str()).collect();
    sqlx::query(
        "INSERT INTO member_exclusions (member_id, ingredient_id, kind)
         SELECT $1, * FROM UNNEST($2::uuid[], $3::text[])",
    )
    .bind(member_id)
    .bind(ingredients)
    .bind(kinds)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/members",
    tag = "membres",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, body = Vec<Member>),
        (status = 404, body = ErrorBody),
    )
)]
async fn list(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<Vec<Member>>> {
    role_in(&state.db, id, user_id).await?;
    load(&state.db, id, None).await.map(Json)
}

/// Ajoute un membre, avec le barème par défaut de son type et présent à tous les repas.
#[utoipa::path(
    post,
    path = "/api/v1/households/{id}/members",
    tag = "membres",
    params(("id" = Uuid, Path)),
    request_body = NewMember,
    responses(
        (status = 201, body = Member),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
async fn create(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path(id): Path<Uuid>,
    Json(new): Json<NewMember>,
) -> ApiResult<(StatusCode, Json<Member>)> {
    role_in(&state.db, id, user_id).await?;
    let input = MemberInput {
        name: new.name,
        kind: new.kind,
        diet: new.diet.unwrap_or(Diet::Omnivore),
        user_id: new.user_id,
        portions: Portions::default_for(new.kind),
        presence: Presence::always(),
        exclusions: Vec::new(),
    };
    check(&input)?;
    let member_id = Uuid::now_v7();
    let mut tx = state.db.begin().await?;
    write(&mut tx, id, member_id, &input).await?;
    tx.commit().await?;
    Ok((
        StatusCode::CREATED,
        Json(load_one(&state.db, id, member_id).await?),
    ))
}

#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/members/{member_id}",
    tag = "membres",
    params(("id" = Uuid, Path), ("member_id" = Uuid, Path)),
    responses(
        (status = 200, body = Member),
        (status = 404, body = ErrorBody),
    )
)]
async fn read(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, member_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<Json<Member>> {
    role_in(&state.db, id, user_id).await?;
    load_one(&state.db, id, member_id).await.map(Json)
}

/// Remplace le membre : nom, type, régime, compte, barème, présence et exclusions.
#[utoipa::path(
    put,
    path = "/api/v1/households/{id}/members/{member_id}",
    tag = "membres",
    params(("id" = Uuid, Path), ("member_id" = Uuid, Path)),
    request_body = MemberInput,
    responses(
        (status = 200, body = Member),
        (status = 404, body = ErrorBody),
        (status = 409, body = ErrorBody),
        (status = 422, body = ErrorBody),
    )
)]
async fn replace(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, member_id)): Path<(Uuid, Uuid)>,
    Json(input): Json<MemberInput>,
) -> ApiResult<Json<Member>> {
    role_in(&state.db, id, user_id).await?;
    check(&input)?;
    let mut tx = state.db.begin().await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS (SELECT 1 FROM members WHERE id = $1 AND household_id = $2)",
    )
    .bind(member_id)
    .bind(id)
    .fetch_one(&mut *tx)
    .await?;
    if !exists {
        return Err(ApiError::not_found());
    }
    write(&mut tx, id, member_id, &input).await?;
    tx.commit().await?;
    load_one(&state.db, id, member_id).await.map(Json)
}

#[utoipa::path(
    delete,
    path = "/api/v1/households/{id}/members/{member_id}",
    tag = "membres",
    params(("id" = Uuid, Path), ("member_id" = Uuid, Path)),
    responses(
        (status = 204, description = "Supprimé"),
        (status = 404, body = ErrorBody),
    )
)]
async fn remove(
    State(state): State<AppState>,
    CurrentUser(user_id): CurrentUser,
    Path((id, member_id)): Path<(Uuid, Uuid)>,
) -> ApiResult<StatusCode> {
    role_in(&state.db, id, user_id).await?;
    let deleted = sqlx::query("DELETE FROM members WHERE id = $1 AND household_id = $2")
        .bind(member_id)
        .bind(id)
        .execute(&state.db)
        .await?
        .rows_affected();
    if deleted == 0 {
        return Err(ApiError::not_found());
    }
    Ok(StatusCode::NO_CONTENT)
}

/// Barèmes par défaut, par type de membre (pour « remettre le barème par défaut »).
#[derive(Serialize, ToSchema)]
pub struct DefaultPortions {
    pub adult: Portions,
    pub teen: Portions,
    pub child: Portions,
}

#[utoipa::path(
    get,
    path = "/api/v1/portions/defaults",
    tag = "membres",
    responses((status = 200, body = DefaultPortions))
)]
async fn defaults(_: CurrentUser) -> Json<DefaultPortions> {
    Json(DefaultPortions {
        adult: Portions::default_for(MemberKind::Adult),
        teen: Portions::default_for(MemberKind::Teen),
        child: Portions::default_for(MemberKind::Child),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bareme_par_defaut_du_design() {
        let teen = Portions::default_for(MemberKind::Teen);
        assert_eq!((teen.protein, teen.starch, teen.veg), (1.0, 2.0, 1.0));
        let child = Portions::default_for(MemberKind::Child);
        assert_eq!(
            (
                child.protein,
                child.starch,
                child.veg,
                child.fat,
                child.other
            ),
            (0.6, 1.0, 0.6, 0.6, 0.6)
        );
        assert_eq!(
            Portions::default_for(MemberKind::Adult),
            Portions::uniform(1.0)
        );
    }
}
