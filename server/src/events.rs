//! Flux SSE du foyer : signale qu'il y a du nouveau (courses cochées, menu modifié). Les clients relisent ensuite
//! ce qui a changé ; le flux ne transporte pas les données. Un seul serveur : un canal `broadcast` suffit.

use std::{convert::Infallible, time::Duration};

use axum::{
    Router,
    extract::{Path, State},
    response::sse::{Event as SseEvent, KeepAlive, Sse},
    routing::get,
};
use futures_util::Stream;
use serde::Serialize;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::{AppState, auth::Caller, error::ApiResult, menus::Day, types::Scope};

pub fn router() -> Router<AppState> {
    Router::new().route("/households/{id}/events", get(events))
}

/// Un événement du foyer.
#[derive(Clone, Debug, Serialize)]
pub struct Event {
    #[serde(skip)]
    pub household: Uuid,
    /// `shopping` : une liste de courses a changé ; `menu` : un menu a changé (et sa liste avec) ; `cooking` : la
    /// cuisson en cours a commencé, changé de place ou s'est terminée.
    #[serde(skip)]
    pub kind: &'static str,
    pub week_start: Option<Day>,
    /// Dernier numéro du journal des courses.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cursor: Option<i64>,
}

pub fn channel() -> broadcast::Sender<Event> {
    broadcast::channel(64).0
}

impl AppState {
    /// Prévient les clients du foyer ; sans abonné, rien à faire.
    pub fn notify(
        &self,
        household: Uuid,
        kind: &'static str,
        week_start: Option<Day>,
        cursor: Option<i64>,
    ) {
        let _ = self.events.send(Event {
            household,
            kind,
            week_start,
            cursor,
        });
    }
}

/// Flux SSE : `event: shopping`, `event: menu` ou `event: cooking`, données `{"week_start": …, "cursor": …}` ;
/// battement toutes les 30 s (Cloudflare coupe une connexion muette au bout de 100 s). `event: resync` : des événements ont été perdus,
/// tout relire.
#[utoipa::path(
    get,
    path = "/api/v1/households/{id}/events",
    tag = "courses",
    params(("id" = Uuid, Path)),
    responses(
        (status = 200, description = "Flux d'événements", content_type = "text/event-stream"),
        (status = 404, body = crate::error::ErrorBody),
    )
)]
pub async fn events(
    State(state): State<AppState>,
    caller: Caller,
    Path(id): Path<Uuid>,
) -> ApiResult<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>> {
    caller.require(&state.db, id, Scope::Read).await?;
    let receiver = state.events.subscribe();
    let stream = futures_util::stream::unfold(receiver, move |mut receiver| async move {
        loop {
            match receiver.recv().await {
                Ok(event) if event.household == id => {
                    let sse = SseEvent::default()
                        .event(event.kind)
                        .json_data(&event)
                        .unwrap_or_default();
                    return Some((Ok(sse), receiver));
                }
                Ok(_) => continue,
                Err(broadcast::error::RecvError::Lagged(_)) => {
                    return Some((Ok(SseEvent::default().event("resync").data("{}")), receiver));
                }
                Err(broadcast::error::RecvError::Closed) => return None,
            }
        }
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(30))))
}
