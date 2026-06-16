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
                // A throttled nudge that the in-progress turn's usage advanced, so
                // the global stats banner refetches and the counter ticks live.
                Ok(ServerEvent::Usage { .. }) => {
                    yield Ok(Event::default().event("usage").data("{}"));
                }
                Ok(
                    ServerEvent::Task { .. }
                    | ServerEvent::Notification { .. }
                    | ServerEvent::HeartAttack { .. }
                    | ServerEvent::TaskFinished { .. }
                    | ServerEvent::Compose { .. }
                    | ServerEvent::ComposeChanged,
                ) => {}
                // A lagged consumer just resyncs; a closed channel ends the stream.
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `GET /api/v1/notifications/stream` - app-wide stream for the notifications
/// sidebar: a `notification` event when the agent asks something (driving toasts
/// and native notifications), and a `refresh` tick on any board change so the
/// pending list stays current as questions are answered.
pub async fn notification_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(ServerEvent::Notification { task_id, task_title, prompt }) => {
                    let payload = serde_json::json!({
                        "task_id": task_id,
                        "task_title": task_title,
                        "prompt": prompt,
                    });
                    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("notification").data(data));
                }
                Ok(ServerEvent::HeartAttack { task_id, task_title, summary }) => {
                    let payload = serde_json::json!({
                        "task_id": task_id,
                        "task_title": task_title,
                        "summary": summary,
                    });
                    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("heart_attack").data(data));
                }
                Ok(ServerEvent::TaskFinished { task_id, task_title }) => {
                    let payload = serde_json::json!({
                        "task_id": task_id,
                        "task_title": task_title,
                    });
                    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("task_finished").data(data));
                }
                Ok(ServerEvent::Board) => {
                    yield Ok(Event::default().event("refresh").data("{}"));
                }
                Ok(
                    ServerEvent::Task { .. }
                    | ServerEvent::Usage { .. }
                    | ServerEvent::Compose { .. }
                    | ServerEvent::ComposeChanged,
                ) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `GET /api/v1/compose/stream` - the compose assistant's live chat events plus a
/// `compose_changed` tick when its drafts or stats change (issue #181).
pub async fn compose_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(ServerEvent::Compose { payload }) => {
                    let data = serde_json::to_string(&payload).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("compose").data(data));
                }
                Ok(ServerEvent::ComposeChanged) => {
                    yield Ok(Event::default().event("compose_changed").data("{}"));
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// `GET /api/v1/activity/stream` - the live agent event feed across *every* task,
/// each line tagged with its `task_id`. Powers the full-screen watch page's
/// combined activity view.
pub async fn activity_stream(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut receiver = state.events.subscribe();
    let stream = async_stream::stream! {
        loop {
            match receiver.recv().await {
                Ok(ServerEvent::Task { task_id, payload }) => {
                    let envelope = serde_json::json!({ "task_id": task_id, "event": payload });
                    let data = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
                    yield Ok(Event::default().event("activity").data(data));
                }
                Ok(_) => {}
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
                // A throttled tick that this task's live token usage advanced; the
                // Stats panel refetches without the partials reaching the feed.
                Ok(ServerEvent::Usage { task_id }) if task_id == id => {
                    yield Ok(Event::default().event("usage").data("{}"));
                }
                Ok(_) => {}
                Err(RecvError::Lagged(_)) => continue,
                Err(RecvError::Closed) => break,
            }
        }
    };
    Sse::new(stream).keep_alive(KeepAlive::default())
}
