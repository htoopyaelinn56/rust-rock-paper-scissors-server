use axum::extract::ws::{Message, WebSocket};
use axum::extract::{State, WebSocketUpgrade, Path};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use std::collections::{HashMap, HashSet};
use tokio::sync::mpsc;
use uuid::Uuid;
use crate::server::responses::{JoinRoomResponse, RoomInfo, RoomListResponse, GameStartedResponse, RoundResultResponse, ErrorResponse, RematchResponse};

const MAX_PLAYERS_PER_ROOM: usize = 10;

// Outcome type for a completed round among active players
enum Outcome {
    Tie { moves: HashMap<String, String> },
    MultiWinners { winners: Vec<Uuid>, moves: HashMap<String, String> },
    SingleWinner { winner: Uuid, moves: HashMap<String, String> },
}

// Compute outcome for current active players
fn compute_round_outcome(active_players: &HashSet<Uuid>, moves: &HashMap<Uuid, String>) -> Outcome {
    // Consider only active players' moves
    let mut unique: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for pid in active_players {
        if let Some(mv) = moves.get(pid) { unique.insert(mv.as_str()); }
    }
    // Prepare moves map for payload
    let mut moves_map: HashMap<String, String> = HashMap::new();
    for pid in active_players {
        if let Some(mv) = moves.get(pid) { moves_map.insert(pid.to_string(), mv.clone()); }
    }

    if unique.len() == 1 || unique.len() == 3 {
        return Outcome::Tie { moves: moves_map };
    }

    // unique.len() == 2: find winning move
    let has_rock = unique.contains("rock");
    let has_paper = unique.contains("paper");
    let has_scissors = unique.contains("scissors");
    let winning_move = if has_rock && has_scissors {
        Some("rock")
    } else if has_paper && has_rock {
        Some("paper")
    } else if has_scissors && has_paper {
        Some("scissors")
    } else {
        None
    };

    if let Some(win) = winning_move {
        let mut winners: Vec<Uuid> = vec![];
        for pid in active_players {
            if let Some(mv) = moves.get(pid) { if mv == win { winners.push(*pid); } }
        }
        if winners.len() == 1 {
            Outcome::SingleWinner { winner: winners[0], moves: moves_map }
        } else {
            Outcome::MultiWinners { winners, moves: moves_map }
        }
    } else {
        Outcome::Tie { moves: moves_map }
    }
}

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
        let room = app.rooms.entry(room_id.clone()).or_insert_with(|| crate::server::server::Room {
            clients: HashMap::new(),
            game_active: false,
            moves: HashMap::new(),
            active_players: HashSet::new(),
        });
        if room.clients.len() >= MAX_PLAYERS_PER_ROOM {
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
        room.clients.insert(client_id, tx.clone());

        // Broadcast join message to all clients in room as JSON (include recipient's my_id)
        for (id, client_tx) in room.clients.iter() {
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
        let rooms_snapshot: Vec<RoomInfo> = app.rooms.iter().map(|(rid, room)| RoomInfo { room_id: rid.clone(), client_count: room.clients.len() }).collect();
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
    let my_id_clone = client_id;
    let my_tx = tx.clone();
    let receive_task = tokio::spawn(async move {
        while let Some(Ok(msg)) = receiver.next().await {
            if let Message::Text(text) = msg {
                // Try to parse as JSON command
                let maybe_val: Result<serde_json::Value, _> = serde_json::from_str(&text);
                if let Ok(val) = maybe_val {
                    let action = val.get("action").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                    match action.as_str() {
                        "start" | "start_game" => {
                            let mut app = state_clone.lock().await;
                            if let Some(room) = app.rooms.get_mut(&room_id_clone) {
                                if room.game_active {
                                    let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "Game already active".into(), my_id: Some(my_id_clone.to_string()) };
                                    if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                                } else if room.clients.len() < 2 {
                                    let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "Need at least 2 players to start".into(), my_id: Some(my_id_clone.to_string()) };
                                    if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                                } else {
                                    room.game_active = true;
                                    room.moves.clear();
                                    room.active_players = room.clients.keys().cloned().collect();
                                    // Snapshot current active players
                                    let players: Vec<String> = room.active_players.iter().map(|id| id.to_string()).collect();
                                    let start_msg = GameStartedResponse { event: "game_started", room_id: room_id_clone.clone(), players };
                                    if let Ok(json) = serde_json::to_string(&start_msg) {
                                        for (_id, client_tx) in room.clients.iter() { let _ = client_tx.send(json.clone()); }
                                    }
                                }
                            }
                        }
                        "move" => {
                            let choice = val.get("choice").and_then(|v| v.as_str()).unwrap_or("").to_lowercase();
                            if !matches!(choice.as_str(), "rock" | "paper" | "scissors") {
                                let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "Invalid choice, use rock|paper|scissors".into(), my_id: Some(my_id_clone.to_string()) };
                                if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                                continue;
                            }
                            {
                                let mut app = state_clone.lock().await;
                                if let Some(room) = app.rooms.get_mut(&room_id_clone) {
                                    if !room.game_active {
                                        let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "Game not active".into(), my_id: Some(my_id_clone.to_string()) };
                                        if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                                    } else if !room.active_players.contains(&my_id_clone) {
                                        let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "You are not active in this round".into(), my_id: Some(my_id_clone.to_string()) };
                                        if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                                    } else {
                                        room.moves.insert(my_id_clone, choice.clone());
                                        // If all active players submitted, compute outcome
                                        if room.moves.len() == room.active_players.len() {
                                            match compute_round_outcome(&room.active_players, &room.moves) {
                                                Outcome::Tie { moves } => {
                                                    // Rematch with same active players
                                                    let next_players: Vec<String> = room.active_players.iter().map(|id| id.to_string()).collect();
                                                    let rem = RematchResponse { event: "rematch", room_id: room_id_clone.clone(), next_players, reason: "tie_all".into(), moves };
                                                    if let Ok(json) = serde_json::to_string(&rem) {
                                                        for (_id, client_tx) in room.clients.iter() { let _ = client_tx.send(json.clone()); }
                                                    }
                                                    room.moves.clear();
                                                    // keep game_active and active_players as-is
                                                }
                                                Outcome::MultiWinners { winners, moves } => {
                                                    // Only winners continue
                                                    let next_players: Vec<String> = winners.iter().map(|id| id.to_string()).collect();
                                                    let rem = RematchResponse { event: "rematch", room_id: room_id_clone.clone(), next_players: next_players.clone(), reason: "multiple_winners".into(), moves };
                                                    if let Ok(json) = serde_json::to_string(&rem) {
                                                        for (_id, client_tx) in room.clients.iter() { let _ = client_tx.send(json.clone()); }
                                                    }
                                                    room.active_players = winners.into_iter().collect();
                                                    room.moves.clear();
                                                }
                                                Outcome::SingleWinner { winner, moves } => {
                                                    let result = RoundResultResponse { event: "round_result", room_id: room_id_clone.clone(), tie: false, winners: vec![winner.to_string()], moves };
                                                    if let Ok(json) = serde_json::to_string(&result) {
                                                        for (_id, client_tx) in room.clients.iter() { let _ = client_tx.send(json.clone()); }
                                                    }
                                                    // End game
                                                    room.game_active = false;
                                                    room.active_players.clear();
                                                    room.moves.clear();
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                            // Optionally could broadcast acknowledgement, but result broadcast covers end of (sub)round
                        }
                        _ => {
                            // Unknown action; ignore or echo
                            let err = ErrorResponse { event: "error", room_id: Some(room_id_clone.clone()), message: "Unknown action".into(), my_id: Some(my_id_clone.to_string()) };
                            if let Ok(json) = serde_json::to_string(&err) { let _ = my_tx.send(json); }
                        }
                    }
                    continue;
                }

                // Default: Broadcast plain text to all clients in this room only
                let app = state_clone.lock().await;
                if let Some(room) = app.rooms.get(&room_id_clone) {
                    for (_, client_tx) in room.clients.iter() {
                        let _ = client_tx.send(text.to_string());
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
        if let Some(room) = app.rooms.get_mut(&room_id) {
            room.clients.remove(&client_id);
            println!("Client {:?} left room {}", client_id, room_id);
            for (id, client_tx) in room.clients.iter() {
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
            // If game was active and a player leaves, end the game
            if room.game_active {
                room.game_active = false;
                room.moves.clear();
                room.active_players.clear();
            }
            // Remove room entirely if empty
            if room.clients.is_empty() {
                app.rooms.remove(&room_id);
            }
        }
        // Notify room watchers about updated rooms list
        let rooms_snapshot: Vec<RoomInfo> = app.rooms.iter().map(|(rid, room)| RoomInfo { room_id: rid.clone(), client_count: room.clients.len() }).collect();
        let payload = serde_json::to_string(&RoomListResponse { rooms: rooms_snapshot }).unwrap_or_else(|_| "{}".into());
        for (_wid, watcher_tx) in app.room_watchers.iter() {
            let _ = watcher_tx.send(payload.clone());
        }
    }
}

