//! Erreurs de l'API : un statut HTTP et un corps JSON `{ "error": code, "message": texte }`.

use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Serialize;
use utoipa::ToSchema;

/// Corps des réponses d'erreur.
#[derive(Serialize, ToSchema)]
pub struct ErrorBody {
    /// Code stable, pour le client : `unauthenticated`, `not_found`, `forbidden`, `invalid`, `last_owner`…
    pub error: &'static str,
    /// Message en français, affichable tel quel.
    pub message: String,
}

#[derive(Debug)]
pub struct ApiError {
    status: StatusCode,
    code: &'static str,
    message: String,
}

pub type ApiResult<T> = Result<T, ApiError>;

impl ApiError {
    pub fn new(status: StatusCode, code: &'static str, message: impl Into<String>) -> Self {
        Self {
            status,
            code,
            message: message.into(),
        }
    }

    pub fn unauthenticated() -> Self {
        Self::new(
            StatusCode::UNAUTHORIZED,
            "unauthenticated",
            "Connexion requise.",
        )
    }

    /// Aussi pour une ressource d'un autre foyer : on ne révèle pas qu'elle existe.
    pub fn not_found() -> Self {
        Self::new(StatusCode::NOT_FOUND, "not_found", "Introuvable.")
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, "forbidden", message)
    }

    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNPROCESSABLE_ENTITY, "invalid", message)
    }

    pub fn conflict(code: &'static str, message: impl Into<String>) -> Self {
        Self::new(StatusCode::CONFLICT, code, message)
    }

    pub fn internal() -> Self {
        Self::new(
            StatusCode::INTERNAL_SERVER_ERROR,
            "internal",
            "Erreur interne.",
        )
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        (
            self.status,
            Json(ErrorBody {
                error: self.code,
                message: self.message,
            }),
        )
            .into_response()
    }
}

impl From<sqlx::Error> for ApiError {
    fn from(err: sqlx::Error) -> Self {
        if let sqlx::Error::Database(db) = &err {
            // 23503 : clé étrangère (ingrédient, équipement, compte inconnus).
            if db.code().as_deref() == Some("23503") {
                return Self::invalid("Référence inconnue.");
            }
            // 23505 : unicité (deux écritures concurrentes).
            if db.code().as_deref() == Some("23505") {
                return Self::conflict("conflict", "Existe déjà.");
            }
        }
        tracing::error!(%err, "base de données");
        Self::internal()
    }
}
