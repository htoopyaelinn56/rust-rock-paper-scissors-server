use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use uuid::Uuid;
use crate::server::responses::{RoomInfo, RoomListResponse};
use crate::server::server::SharedState;

pub async fn rooms_stream(ws: WebSocketUpgrade, State(state): State<SharedState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_rooms_stream(socket, state))
}

async fn handle_rooms_stream(socket: WebSocket, state: SharedState) {
    let (mut sender, mut receiver) = socket.split();

    // Create a channel to send room list updates to this watcher
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let watcher_id = Uuid::new_v4();

    // Send initial snapshot
    {
        let app = state.lock().await;
        let snapshot: Vec<RoomInfo> = app
            .rooms
            .iter()
            .map(|(room_id, room)| RoomInfo { room_id: room_id.clone(), client_count: room.clients.len() })
            .collect();
        let init_msg = serde_json::to_string(&RoomListResponse { rooms: snapshot }).unwrap_or_else(|_| "{}".into());
        if sender.send(Message::Text(init_msg.into())).await.is_err() {
            return;
        }
    }

    // Register watcher for subsequent updates
    {
        let mut app = state.lock().await;
        app.room_watchers.insert(watcher_id, tx.clone());
    }

    // Forward updates to the WebSocket
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if sender.send(Message::Text(msg.into())).await.is_err() {
                break;
            }
        }
    });

    // Drain incoming messages (not used, but keeps connection alive and detects close)
    let recv_task = tokio::spawn(async move {
        while let Some(_msg) = receiver.next().await {
            // Ignore any incoming messages for now
        }
    });

    tokio::select! {
        _ = send_task => {},
        _ = recv_task => {},
    }

    // Cleanup watcher on disconnect
    let mut app = state.lock().await;
    app.room_watchers.remove(&watcher_id);
}
