use axum::routing::get;
use axum::Router;
use std::collections::{HashMap, HashSet};
use std::{net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, Mutex, Notify};
use uuid::Uuid;
use crate::server::{join_room, rooms_stream};
use std::sync::OnceLock;

// Global shutdown notifier for graceful stop from FFI or other callers
static SHUTDOWN_NOTIFY: OnceLock<Notify> = OnceLock::new();

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
    // Initialize tracing for logs (ignore error if already set up)
    let _ = tracing_subscriber::fmt::try_init();

    let state: SharedState = Arc::new(Mutex::new(AppState {
        rooms: HashMap::new(),
        room_watchers: HashMap::new(),
    }));

    // Build our Axum app with the WebSocket route
    let app = Router::new()
        .route("/join/{room_id}", get(join_room::join_room))
        .route("/rooms", get(rooms_stream::rooms_stream))
        .with_state(state);

    // Run server with graceful shutdown support
    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    tracing::info!("listening on {}", addr);

    let notify = SHUTDOWN_NOTIFY.get_or_init(|| Notify::new()).clone();
    axum::serve(listener, app)
        .with_graceful_shutdown(async move {
            // Wait until stop_server() is called
            notify.notified().await;
            tracing::info!("shutdown signal received, stopping server...");
        })
        .await
        .unwrap();
}

// Public function to stop the server from FFI or other callers
pub fn stop_server() {
    if let Some(n) = SHUTDOWN_NOTIFY.get() {
        // Wake the single graceful-shutdown waiter if running
        n.notify_one();
    } else {
        // Server not started; nothing to stop
    }
}
