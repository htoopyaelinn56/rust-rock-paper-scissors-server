use axum::routing::get;
use axum::Router;
use std::collections::{HashMap, HashSet};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, Mutex};
use uuid::Uuid;
use crate::server::{join_room, rooms_stream}; // ensure rooms_stream module is public

// Type alias for client sender
pub(crate) type Tx = mpsc::UnboundedSender<String>;
pub(crate) type Clients = HashMap<Uuid, Tx>;

// Room state
pub struct Room {
    pub clients: Clients,
    pub game_active: bool,
    // Stores each player's submitted move for the current round ("rock", "paper", or "scissors")
    pub moves: HashMap<Uuid, String>,
    // Current active participants (subset of clients) expected to play this round
    pub active_players: HashSet<Uuid>,
}

// Composite application state
pub struct AppState {
    pub rooms: HashMap<String, Room>,
    pub room_watchers: HashMap<Uuid, Tx>, // subscribers to room list updates
}

pub type SharedState = Arc<Mutex<AppState>>;

pub async fn start_server() {
    // Initialize tracing for logs
    tracing_subscriber::fmt::init();

    let state: SharedState = Arc::new(Mutex::new(AppState {
        rooms: HashMap::new(),
        room_watchers: HashMap::new(),
    }));

    // Build our Axum app with the WebSocket route
    let app = Router::new()
        .route("/join/{room_id}", get(join_room::join_room))
        .route("/rooms", get(rooms_stream::rooms_stream))
        .with_state(state);

    // Run server
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    tracing::info!("listening on {}", addr);
    axum::serve(tokio::net::TcpListener::bind(addr).await.unwrap(), app)
        .await
        .unwrap();
}