//! Server-sent-event streams for live board and task updates.

use std::convert::Infallible;

use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use futures::Stream;
use tokio::sync::broadcast::error::RecvError;
use uuid::Uuid;

use crate::state::{AppState, ServerEvent};

/// `GET /api/v1/board/stream` - emits a tick whenever the board changes so the
/// UI can refetch.
pub async fn board_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(ServerEvent::Board) => {
                    yield Ok(Event::default().event("board").data("{}"));
                }
                Ok(ServerEvent::Task { .. }) => {}
                // A lagged consumer just resyncs; a closed channel ends the stream.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `GET /api/v1/tasks/:id/stream` - the live agent event feed for one task.
pub async fn task_stream(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(ServerEvent::Task { task_id, payload }) if task_id == id => {
                    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("task").data(data));
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
