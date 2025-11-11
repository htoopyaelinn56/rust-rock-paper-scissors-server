use serde::{Serialize};

#[derive(Debug, Serialize)]
pub struct JoinRoomResponse {
    pub success: bool,
    pub room_id: Option<String>,
    pub error_message: Option<String>,
}