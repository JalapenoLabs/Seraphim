//! Heart attacks (dead-agent management): the operator clearing the alerts the
//! defibrillator raised.
//!
//! The incidents themselves are created by the orchestrator's defibrillator, not
//! by any request, and are delivered to the board via the board payload. The only
//! endpoint here is the operator acknowledging one once they have read its logs.

use axum::extract::{Path, State};
use axum::Json;
use uuid::Uuid;

use super::ApiResult;
use crate::db::models::HeartAttack;
use crate::db::queries;
use crate::state::AppState;

/// `POST /api/v1/heart-attacks/:id/ack` - the operator dismisses an incident once
/// they have seen it, clearing it from the board banner.
pub async fn acknowledge(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> ApiResult<Json<HeartAttack>> {
    let incident = queries::acknowledge_heart_attack(&state.db, id).await?;
    state.notify_board();
    Ok(Json(incident))
}
