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