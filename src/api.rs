use axum::extract::{Json, State};
use axum::http::StatusCode;
use serde::Deserialize;
use std::str::FromStr;
use std::sync::Arc;
use uuid::Uuid;

use crate::challenge::Challenge;
use crate::{analysis, AppState};

#[derive(Debug, Deserialize)]
pub struct AnalyzeRequest {
    program: String,
    uuid: String,
}

pub async fn analyze(
    State(state): State<Arc<AppState>>,
    Json(request): Json<AnalyzeRequest>,
) -> Result<String, StatusCode> {
    let uuid = Uuid::from_str(&request.uuid).map_err(|_| StatusCode::BAD_REQUEST)?;

    let challenge = Challenge::load(&state.database_pool, &state.data_repository, uuid)
        .await
        .map_err(|_| StatusCode::NOT_FOUND)?;

    state
        .analysis_engine
        .lock()
        .unwrap()
        .run_program(&request.program, analysis::Level::Basic, challenge)
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    Ok("ok".into())
}
