use axum::routing::get;
use axum::Router;
use futures::StreamExt;
use std::collections::HashMap;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;

// Type alias for shared room state
type Tx = mpsc::UnboundedSender<String>;
type Room = Arc<Mutex<HashMap<Uuid, Tx>>>;

pub async fn start_server() {
    // Initialize tracing for logs
    tracing_subscriber::fmt::init();

    let room: Room = Arc::new(Mutex::new(HashMap::new()));

    // Build our Axum app with the WebSocket route
    let app = Router::new()
        .route("/join", get(join_room_logic::join_room))
        .with_state(room);

    // Run server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}

mod join_room_logic {
    use super::*;
    use axum::extract::ws::{Message, WebSocket};
    use axum::extract::{State, WebSocketUpgrade};
    use axum::response::IntoResponse;
    use futures::SinkExt;

    pub async fn join_room(ws: WebSocketUpgrade, State(room): State<Room>) -> impl IntoResponse {
        ws.on_upgrade(move |socket| handle_join_room(socket, room))
    }

    // Handle the actual WebSocket connection
    async fn handle_join_room(socket: WebSocket, room: Room) {
        let (mut sender, mut receiver) = socket.split();

        // Create a channel to send messages to this client
        let (tx, mut rx) = mpsc::unbounded_channel::<String>();
        let client_id = Uuid::new_v4();

        // Add client to the room
        {
            let mut clients = room.lock().await;
            clients.insert(client_id, tx.clone());

            // Broadcast join message to all other clients
            for (id, client_tx) in clients.iter() {
                if *id != client_id {
                    let _ = client_tx.send(format!("Client {:?} joined the room", client_id));
                }
            }
        }

        println!("Client {:?} joined the room", client_id);

        // Task to forward messages from room to client
        let send_task = tokio::spawn(async move {
            while let Some(msg) = rx.recv().await {
                if sender.send(Message::Text(msg.into())).await.is_err() {
                    break;
                }
            }
        });

        // Task to receive messages from this client
        let room_clone = room.clone();
        let receive_task = tokio::spawn(async move {
            while let Some(Ok(msg)) = receiver.next().await {
                if let Message::Text(text) = msg {
                    // Broadcast to all clients
                    let clients = room_clone.lock().await;
                    for (id, client_tx) in clients.iter() {
                        if *id != client_id {
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
        room.lock().await.remove(&client_id);
        println!("Client {:?} left the room", client_id);

        // Optionally broadcast leave message
        let clients = room.lock().await;
        for (_id, client_tx) in clients.iter() {
            let _ = client_tx.send(format!("Client {:?} left the room", client_id));
        }
    }
}
