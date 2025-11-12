use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade, Path};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;
use crate::server::responses::{JoinRoomResponse, RoomInfo, RoomListResponse};

const MAX_PLAYERS_PER_ROOM: usize = 10;

pub async fn join_room(Path(room_id): Path<String>, ws: WebSocketUpgrade, State(state): State<crate::server::server::SharedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_join_room(room_id, socket, state))
}

// Handle the actual WebSocket connection
async fn handle_join_room(room_id: String, socket: WebSocket, state: crate::server::server::SharedState) {
    let (mut sender, mut receiver) = socket.split();

    // Create a channel to send messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let client_id = Uuid::new_v4();

    // Validate and add client to the specified room
    {
        let mut app = state.lock().await;
        let clients = app.rooms.entry(room_id.clone()).or_insert_with(HashMap::new);
        if clients.len() >= MAX_PLAYERS_PER_ROOM {
            // Room is full: inform client with JSON and close connection
            let response = JoinRoomResponse {
                success: false,
                room_id: Some(room_id.clone()),
                message: Some(format!("Room is full (max {} players)", MAX_PLAYERS_PER_ROOM).into()),
                my_id: Some(client_id.to_string()),
            };
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = sender.send(Message::Text(json.into())).await;
            }
            let _ = sender.send(Message::Close(None)).await;
            return;
        }
        clients.insert(client_id, tx.clone());

        // Broadcast join message to all clients in room as JSON (include recipient's my_id)
        for (id, client_tx) in clients.iter() {
            let response = JoinRoomResponse {
                success: true,
                room_id: Some(room_id.clone()),
                message: Some(format!("Client {:?} joined room {}", client_id, room_id).into()),
                my_id: Some(id.to_string()),
            };
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = client_tx.send(json);
            }
        }

        // Notify room watchers about updated rooms list
        let rooms_snapshot: Vec<RoomInfo> = app.rooms.iter().map(|(rid, cs)| RoomInfo { room_id: rid.clone(), client_count: cs.len() }).collect();
        let payload = serde_json::to_string(&RoomListResponse { rooms: rooms_snapshot }).unwrap_or_else(|_| "{}".into());
        for (_wid, watcher_tx) in app.room_watchers.iter() {
            let _ = watcher_tx.send(payload.clone());
        }
    }

    println!("Client {:?} joined room {}", client_id, room_id);

    // Task to forward messages from room to client
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Task to receive messages from this client
    let state_clone = state.clone();
    let room_id_clone = room_id.clone();
    let receive_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                // Broadcast to all clients in this room only
                let app = state_clone.lock().await;
                if let Some(clients) = app.rooms.get(&room_id_clone) {
                    for (_, client_tx) in clients.iter() {
                        let _ = client_tx.send(text.clone().to_string());
                    }
                }
            }
        }
    });

    // Wait for either task to complete (disconnect)
    tokio::select! {
        _ = send_task => {},
        _ = receive_task => {},
    }

    // Remove client from room on disconnect
    {
        let mut app = state.lock().await;
        if let Some(clients) = app.rooms.get_mut(&room_id) {
            clients.remove(&client_id);
            println!("Client {:?} left room {}", client_id, room_id);
            for (id, client_tx) in clients.iter() {
                let response = JoinRoomResponse {
                    success: true,
                    room_id: Some(room_id.clone()),
                    message: Some(format!("Client {:?} left room {}", client_id, room_id)),
                    my_id: Some(id.to_string()),
                };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = client_tx.send(json);
                }
            }
            // Remove room entirely if empty
            if clients.is_empty() {
                app.rooms.remove(&room_id);
            }
        }
        // Notify room watchers about updated rooms list
        let rooms_snapshot: Vec<RoomInfo> = app.rooms.iter().map(|(rid, cs)| RoomInfo { room_id: rid.clone(), client_count: cs.len() }).collect();
        let payload = serde_json::to_string(&RoomListResponse { rooms: rooms_snapshot }).unwrap_or_else(|_| "{}".into());
        for (_wid, watcher_tx) in app.room_watchers.iter() {
            let _ = watcher_tx.send(payload.clone());
        }
    }
}