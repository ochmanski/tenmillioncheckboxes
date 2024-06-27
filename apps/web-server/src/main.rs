use env_logger;
use futures_util::{sink::SinkExt, StreamExt};
use log::{error, info};
use redis::aio::MultiplexedConnection;
use redis::AsyncCommands;
use redis::Client as RedisClient;
use std::env;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::broadcast;
use tokio::sync::Mutex;
use tokio_tungstenite::accept_async;
use tokio_tungstenite::tungstenite::protocol::Message;

#[tokio::main]
async fn main() {
    env_logger::init();

    let port = env::var("PORT").unwrap_or("8080".to_string());
    let addr = format!("0.0.0.0:{}", port);
    let listener = TcpListener::bind(&addr).await.expect("Failed to bind");

    info!("Listening on: {}", addr);

    let redis_url = env::var("REDIS_URL").expect("REDIS_URL must be set");
    let redis_client = RedisClient::open(redis_url).expect("Invalid Redis URL");
    let redis_conn = redis_client
        .get_multiplexed_async_connection()
        .await
        .unwrap();

    let (tx, _rx) = broadcast::channel(100);

    tokio::spawn(subscribe_and_push_changes(redis_client.clone(), tx.clone()));

    while let Ok((stream, _)) = listener.accept().await {
        let redis_conn1 = redis_conn.clone();
        let rx = tx.subscribe();
        tokio::spawn(handle_connection(stream, redis_conn1, rx));
    }
}

async fn handle_connection(
    stream: tokio::net::TcpStream,
    redis_conn: MultiplexedConnection,
    mut rx: broadcast::Receiver<String>,
) {
    let ws_stream = accept_async(stream)
        .await
        .expect("Error during the websocket handshake");

    info!("New WebSocket connection established");

    let (write, mut read) = ws_stream.split();
    let write = Arc::new(Mutex::new(write));
    let write_for_incoming_handle = Arc::clone(&write);

    tokio::spawn(async move {
        let redis_conn = redis_conn.clone();

        while let Some(message) = read.next().await {
            match message {
                Ok(msg) => {
                    if msg.is_text() {
                        let text = msg.into_text().unwrap();

                        if text.starts_with("get") {
                            let (start, end) = parse_get_message(&text).unwrap();
                            info!("Getting checkboxes from {} to {}", start, end);

                            let mut response = String::new();
                            response.push_str("get,");
                            let list: Vec<(String, u8)> = redis_conn
                                .clone()
                                .zrange_withscores("checkboxes", start as isize, end as isize)
                                .await
                                .unwrap();

                            let mut response = String::new();
                            response += "get";

                            for (index, checked) in list {
                                response.push_str(&format!(",{}:{}", index, checked));
                            }

                            let mut write = write_for_incoming_handle.lock().await;
                            write.send(Message::Text(response)).await.unwrap();
                            continue;
                        }

                        let (index, action) = parse_change_message(&text).unwrap();
                        info!("{:#?} {} ", action, index);

                        let checked: bool = action.clone().into();
                        let _: () = redis_conn
                            .clone()
                            .zadd("checkboxes", index, checked as u8)
                            .await
                            .unwrap();

                        let action_str: String = action.clone().into();
                        let msg = format!("{},{}", action_str, index);
                        let _: () = redis_conn
                            .clone()
                            .publish::<_, String, ()>("checkbox_changes", msg)
                            .await
                            .unwrap();

                        let mut write = write_for_incoming_handle.lock().await; // Lock the writer
                        write.send(Message::Text(text)).await.unwrap();
                    }
                }
                Err(e) => {
                    error!("Error processing message: {}", e);
                    break;
                }
            }
        }

        info!("WebSocket connection closed");
    });

    let write_for_broadcast_handle = Arc::clone(&write);

    // Listen for broadcasts from the Redis subscription
    tokio::spawn(async move {
        while let Ok(payload) = rx.recv().await {
            info!("Broadcast message received: {}", payload);
            let mut write = write_for_broadcast_handle.lock().await;
            write
                .send(Message::Text(payload))
                .await
                .unwrap_or_else(|e| {
                    error!("Error sending WebSocket message: {}", e);
                });
        }
    });
}

async fn subscribe_and_push_changes(redis_client: RedisClient, tx: broadcast::Sender<String>) {
    let mut pubsub = redis_client.get_async_pubsub().await.unwrap();
    pubsub.subscribe("checkbox_changes").await.unwrap();

    while let Some(msg) = pubsub.on_message().next().await {
        let payload: String = msg.get_payload().unwrap();
        let _ = tx.send(payload);
    }
}

/// this is of type `c,checkboxIndex` or `u,checkboxIndex`
/// where `c` is for checked and `u` is for unchecked
/// and `checkboxIndex` is the index of the checkbox in the list (0 - 10 000 000)
/// e.g. `c,123` means the checkbox with index 123 was checked
/// e.g. `u,123` means the checkbox with index 123 was unchecked
fn parse_change_message(msg: &str) -> Option<(u32, ChangeAction)> {
    if msg.len() < 3 {
        return None;
    }

    let parts: Vec<&str> = msg.split(',').collect();
    if parts.len() != 2 {
        return None;
    }

    let action = parts[0];
    let index = parts[1];

    if action != "c" && action != "u" {
        return None;
    }

    if index.parse::<u32>().is_err() {
        return None;
    }

    Some((index.parse().unwrap(), action.to_string().into()))
}

/// this is of type `get,checkboxIndexStart,checkboxIndexEnd`
/// where `get` is the command to get the status of checkboxes
/// and `checkboxIndexStart` and `checkboxIndexEnd` are the range of checkboxes to get the status of
/// e.g. `get,123,456` means get the status of checkboxes from 123 to 456
/// e.g. `get,0,10000000` means get the status of all checkboxes
fn parse_get_message(msg: &str) -> Option<(u32, u32)> {
    if msg.len() < 3 {
        return None;
    }

    let parts: Vec<&str> = msg.split(',').collect();
    if parts.len() != 3 {
        return None;
    }

    let command = parts[0];
    let start = parts[1];
    let end = parts[2];

    if command != "get" {
        return None;
    }

    if start.parse::<u32>().is_err() {
        return None;
    }

    if end.parse::<u32>().is_err() {
        return None;
    }

    Some((start.parse().unwrap(), end.parse().unwrap()))
}

#[derive(Debug, Clone)]
enum ChangeAction {
    Check,
    Uncheck,
}

impl From<String> for ChangeAction {
    fn from(s: String) -> Self {
        match s.as_str() {
            "c" => ChangeAction::Check,
            "u" => ChangeAction::Uncheck,
            _ => unreachable!(),
        }
    }
}

impl Into<bool> for ChangeAction {
    fn into(self) -> bool {
        match self {
            ChangeAction::Check => true,
            ChangeAction::Uncheck => false,
        }
    }
}

impl Into<String> for ChangeAction {
    fn into(self) -> String {
        match self {
            ChangeAction::Check => "c".to_string(),
            ChangeAction::Uncheck => "u".to_string(),
        }
    }
}
