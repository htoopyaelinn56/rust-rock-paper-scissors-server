use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade, Path};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::collections::HashMap;
use tokio::sync::mpsc;
use uuid::Uuid;
use crate::server::responses::JoinRoomResponse;

pub async fn join_room(Path(room_id): Path<String>, ws: WebSocketUpgrade, State(rooms): State<crate::server::server::Rooms>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_join_room(room_id, socket, rooms))
}

// Handle the actual WebSocket connection
async fn handle_join_room(room_id: String, socket: WebSocket, rooms: crate::server::server::Rooms) {
    let (mut sender, mut receiver) = socket.split();

    // Create a channel to send messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    let client_id = Uuid::new_v4();

    // Validate and add client to the specified room (max 2 players)
    {
        let mut rooms_guard = rooms.lock().await;
        let clients = rooms_guard.entry(room_id.clone()).or_insert_with(HashMap::new);
        if clients.len() >= 2 {
            // Room is full: inform client with JSON and close connection
            let response = JoinRoomResponse {
                success: false,
                room_id: Some(room_id.clone()),
                error_message: Some("Room is full (max 2 players)".into()),
            };
            if let Ok(json) = serde_json::to_string(&response) {
                let _ = sender.send(Message::Text(json.into())).await;
            }
            let _ = sender.send(Message::Close(None)).await;
            return;
        }
        clients.insert(client_id, tx.clone());

        // Broadcast join message to all other clients in room as JSON
        for (id, client_tx) in clients.iter() {
            if *id != client_id {
                let response = JoinRoomResponse {
                    success: true,
                    room_id: Some(room_id.clone()),
                    error_message: None,
                };
                if let Ok(json) = serde_json::to_string(&response) {
                    let _ = client_tx.send(json);
                }
            }
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
    let rooms_clone = rooms.clone();
    let room_id_clone = room_id.clone();
    let receive_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                // Broadcast to all clients in this room only
                let rooms_locked = rooms_clone.lock().await;
                if let Some(clients) = rooms_locked.get(&room_id_clone) {
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
        let mut rooms_guard = rooms.lock().await;
        if let Some(clients) = rooms_guard.get_mut(&room_id) {
            clients.remove(&client_id);
            println!("Client {:?} left room {}", client_id, room_id);
            for (_id, client_tx) in clients.iter() {
                let _ = client_tx.send(format!("Client {:?} left room {}", client_id, room_id));
            }
            // Remove room entirely if empty
            if clients.is_empty() {
                rooms_guard.remove(&room_id);
            }
        }
    }
}