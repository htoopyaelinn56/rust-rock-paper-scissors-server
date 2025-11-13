# Rust Rock-Paper-Scissors WebSocket Server

This repository implements a simple WebSocket-based multiplayer Rock-Paper-Scissors game server using Axum and Tokio. This README documents the WebSocket endpoints, the JSON payloads the server expects, the events it emits, and example client usage.

## Quick start

Build and run the server (requires Rust toolchain):

```bash
cargo run --release
```

By default the server listens on 0.0.0.0:3000, so the WebSocket base URLs are:

- Join a room (play): `ws://localhost:3000/join/{room_id}`
- Watch available rooms (room list updates): `ws://localhost:3000/rooms`

Use `wss://` if you run the server behind TLS or a reverse-proxy that terminates TLS.

## Concepts / contract

- A "room" is identified by a string `{room_id}`.
- Up to 10 players can join a room (server constant `MAX_PLAYERS_PER_ROOM = 10`).
- Join the room via the `/join/{room_id}` WebSocket endpoint. Each client receives a `my_id` (UUID string) that identifies them in the room.
- To start a game, any connected client may send a `start` command. A game requires at least 2 players.
- During a round, the server expects each active player to submit a move: `rock`, `paper`, or `scissors`.
- When all active players have submitted moves, the server computes the outcome and emits either a rematch event (tie or multiple winners) or a round result (single winner). The server also ends the game when a single winner is determined.

## WebSocket endpoints

1) Join a room (game play):

    ws://localhost:3000/join/{room_id}

    - Connect to a room by substituting `{room_id}` with your chosen room identifier.
    - After opening the connection you'll immediately receive a JSON JoinRoomResponse and the server will broadcast join/leave notifications to clients in the same room.

2) Room list stream (watcher):

    ws://localhost:3000/rooms

    - Connect to receive an initial snapshot of currently active rooms and periodic updates when rooms change (client join/leave or room removal).
    - The server sends a `RoomListResponse` JSON message as the initial payload and whenever the room list changes.

## Client -> Server messages (requests)

Clients should send JSON text messages to the `/join/{room_id}` socket. The server reads JSON and looks for an `action` field (string). Known actions:

- Start a game

```json
{ "action": "start" }
```

- Start (alternate name)

```json
{ "action": "start_game" }
```

- Submit a move

```json
{ "action": "move", "choice": "rock" }
```

Valid choices for `choice` are: `"rock"`, `"paper"`, `"scissors"` (case-insensitive). If a non-JSON text is sent, the server will broadcast the raw text to all clients in the same room.

Any unknown action or invalid payload will produce an ErrorResponse from the server.

## Server -> Client messages (responses/events)

Server messages are JSON text. The main response/event types are described below (field names match the server structs):

1) JoinRoomResponse

Emitted when a client joins/leaves a room or when a join fails (e.g. room full).

Example:

```json
{
  "success": true,
  "room_id": "lobby-1",
  "message": "Client 123e4567-e89b-12d3-a456-426614174000 joined room lobby-1",
  "my_id": "123e4567-e89b-12d3-a456-426614174000"
}
```

Fields:
- success: boolean
- room_id: string | null
- message: optional human readable message
- my_id: your assigned UUID (string)

2) RoomListResponse

Sent on `/rooms` watcher connections and whenever rooms change.

Example:

```json
{
  "rooms": [
    { "room_id": "lobby-1", "client_count": 2 },
    { "room_id": "game-42", "client_count": 3 }
  ]
}
```

3) GameStartedResponse

Sent when a game successfully starts. Contains the list of active player IDs for the first round.

Example:

```json
{
  "event": "game_started",
  "room_id": "lobby-1",
  "players": ["uuid1","uuid2","uuid3"]
}
```

4) RematchResponse

Sent when the round results in a tie between all active players, or when multiple winners remain and they play again. Server includes the `moves` map of the last round and `next_players` who should participate in the next round.

Example (tie or multiple winners):

```json
{
  "event": "rematch",
  "room_id": "lobby-1",
  "next_players": ["uuid1","uuid2"],
  "reason": "multiple_winners",
  "moves": { "uuid1": "rock", "uuid2": "rock", "uuid3": "scissors" }
}
```

5) RoundResultResponse

Sent when a single winner is determined. The server will end the game after this event.

Example:

```json
{
  "event": "round_result",
  "room_id": "lobby-1",
  "tie": false,
  "winners": ["uuid2"],
  "moves": { "uuid1": "rock", "uuid2": "paper", "uuid3": "rock" }
}
```

6) ErrorResponse

Sent when an invalid command or state is encountered (e.g., invalid choice, game not active, not active player, game already active).

Example:

```json
{
  "event": "error",
  "room_id": "lobby-1",
  "message": "Game not active",
  "my_id": "uuid1"
}
```

## Example client (browser / Node.js)

Browser or Node example using the standard WebSocket API:

```js
// Join a room
const ws = new WebSocket('ws://localhost:3000/join/lobby-1');

ws.onopen = () => console.log('connected');

ws.onmessage = (ev) => {
  try {
    const msg = JSON.parse(ev.data);
    console.log('received', msg);
  } catch (err) {
    console.log('received text:', ev.data);
  }
};

// Start a game (send when at least 2 players are present)
ws.send(JSON.stringify({ action: 'start' }));

// Send a move when prompted (rock/paper/scissors)
ws.send(JSON.stringify({ action: 'move', choice: 'rock' }));

// Send plain text (will be broadcast to the room as-is)
ws.send('Hello everyone');
```

Watcher example to track rooms:

```js
const rws = new WebSocket('ws://localhost:3000/rooms');
rws.onmessage = (ev) => console.log('rooms update', JSON.parse(ev.data));
```

## Notes, edge cases and behavior

- Max players per room: 10. If you try to join a full room the server will send a `JoinRoomResponse` with `success: false` and then close the connection.
- Starting a game requires at least 2 connected players. If not, you'll receive an `ErrorResponse`.
- If a player disconnects during an active game the server ends the game and clears round state.
- The server treats any non-JSON text message as a raw broadcast to the room. JSON messages without a recognized `action` will receive an `ErrorResponse`.
- Moves are compared only among `active_players` (the subset of clients expected to play the current round). After a `rematch` with `next_players`, only the listed `next_players` are expected to submit moves.
- The server computes ties when all active players choose the same move or when all three choices are present. When two distinct choices are present, winners are determined by the usual rock-paper-scissors rules.

## Troubleshooting & development

- The server logs to stdout (uses `tracing`).
- To run in debug mode during development:

```bash
cargo run
```

- If you want me to also add a small automated test or a TypeScript client wrapper, tell me and I can add it.

---

If anything in this README doesn't match your running server, I can update the docs — tell me which parts you'd like changed or which example clients you prefer (Python, Rust, or TypeScript). 
