use axum::routing::get;
use axum::Router;
use std::collections::HashMap;
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;
use crate::server::join_room;

// Type alias for shared room state
type Tx = mpsc::UnboundedSender<String>;
pub(crate) type Room = Arc<Mutex<HashMap<Uuid, Tx>>>;

pub async fn start_server() {
    // Initialize tracing for logs
    tracing_subscriber::fmt::init();

    let room: Room = Arc::new(Mutex::new(HashMap::new()));

    // Build our Axum app with the WebSocket route
    let app = Router::new()
        .route("/join", get(join_room::join_room))
        .with_state(room);

    // Run server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}