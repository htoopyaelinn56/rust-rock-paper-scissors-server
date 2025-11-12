use serde::{Serialize};

#[derive(Debug, Serialize)]
pub struct JoinRoomResponse {
    pub success: bool,
    pub room_id: Option<String>,
    pub message: Option<String>,
    pub my_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RoomListResponse {
    pub rooms: Vec<RoomInfo>,
}

#[derive(Debug, Serialize)]
pub struct RoomInfo {
    pub room_id: String,
    pub client_count: usize,
}

// Game-related responses
#[derive(Debug, Serialize)]
pub struct GameStartedResponse {
    pub event: &'static str, // "game_started"
    pub room_id: String,
    pub players: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct RoundResultResponse {
    pub event: &'static str, // "round_result"
    pub room_id: String,
    pub tie: bool,
    pub winners: Vec<String>,
    pub moves: std::collections::HashMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct ErrorResponse {
    pub event: &'static str, // "error"
    pub room_id: Option<String>,
    pub message: String,
    pub my_id: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct RematchResponse {
    pub event: &'static str, // "rematch"
    pub room_id: String,
    pub next_players: Vec<String>,
    pub reason: String, // e.g., "multiple_winners" or "tie_all"
    pub moves: std::collections::HashMap<String, String>,
}
