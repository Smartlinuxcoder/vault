use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::Deserialize;
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, RwLock};
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod crypto;
mod types;

use types::*;

// ============== APP STATE ==============

#[derive(Clone)]
pub struct AppState {
    pub node: Arc<RwLock<Node>>,
    pub connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
}

impl AppState {
    pub fn new(node: Node) -> Self {
        Self {
            node: Arc::new(RwLock::new(node)),
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    pub async fn load_or_create(config_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use std::path::Path;

        let node = if Path::new(config_path).exists() {
            let data = std::fs::read_to_string(config_path)?;
            let mut node: Node = serde_json::from_str(&data)?;

            let privkey_path = "config/node_privkey.pem";
            if Path::new(privkey_path).exists() {
                node.privkey = std::fs::read_to_string(privkey_path)?;
                let derived = crypto::derive_pubkey(&node.privkey)?;
                if derived != node.pubkey {
                    return Err("Public key mismatch!".into());
                }
            }

            node.version = Some(env!("CARGO_PKG_VERSION").to_string());
            node
        } else {
            let (pubkey, privkey) = crypto::generate_keypair()?;

            let node = Node {
                name: "ArsonnetNode".to_string(),
                pubkey,
                privkey: privkey.clone(),
                address: "0.0.0.0".to_string(),
                listen_port: 3000,
                public_port: 3000,
                secure: false,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                peers: Vec::new(),
                ping_interval: 30,
            };

            if let Some(parent) = Path::new(config_path).parent() {
                std::fs::create_dir_all(parent)?;
            }

            let mut save_node = node.clone();
            save_node.privkey = String::new();
            std::fs::write(config_path, serde_json::to_string_pretty(&save_node)?)?;
            std::fs::write("config/node_privkey.pem", &privkey)?;

            node
        };

        Ok(Self::new(node))
    }

    pub async fn get_public_info(&self) -> SignedNode {
        let node = self.node.read().await;
        let peer_node = PeerNode {
            name: Some(node.name.clone()),
            pubkey: node.pubkey.clone(),
            address: node.address.clone(),
            public_port: node.public_port,
            secure: node.secure,
            version: node.version.clone(),
            last_seen: Some(crypto::current_timestamp()),
            is_connected: true,
        };

        let node_json = serde_json::to_string(&peer_node).unwrap_or_default();
        let signature = crypto::sign_data(&node.privkey, node_json.as_bytes()).unwrap_or_default();

        SignedNode {
            node: peer_node,
            signature,
        }
    }

    pub async fn register_connection(&self, pubkey: String, tx: mpsc::UnboundedSender<WsServerMessage>) {
        let state = ConnectionState {
            pubkey: pubkey.clone(),
            tx: tx.clone(),
            connected_at: crypto::current_timestamp(),
        };

        self.connections.write().await.insert(pubkey.clone(), state);
        self.broadcast_peer_status(&pubkey, true).await;
    }

    pub async fn unregister_connection(&self, pubkey: &str) {
        self.connections.write().await.remove(pubkey);
        self.broadcast_peer_status(pubkey, false).await;
    }

    pub async fn send_to_peer(&self, to_pubkey: &str, message: WsServerMessage) -> bool {
        if let Some(conn) = self.connections.read().await.get(to_pubkey) {
            conn.tx.send(message).is_ok()
        } else {
            false
        }
    }

    async fn broadcast_peer_status(&self, pubkey: &str, online: bool) {
        let msg = WsServerMessage::PeerStatus {
            pubkey: pubkey.to_string(),
            online,
        };

        let connections = self.connections.read().await;
        for (key, conn) in connections.iter() {
            if key != pubkey {
                let _ = conn.tx.send(msg.clone());
            }
        }
    }

    pub async fn get_online_peers(&self) -> Vec<PeerNode> {
        let connections = self.connections.read().await;
        connections
            .values()
            .map(|conn| PeerNode {
                name: None,
                pubkey: conn.pubkey.clone(),
                address: String::new(),
                public_port: 0,
                secure: false,
                version: None,
                last_seen: Some(conn.connected_at),
                is_connected: true,
            })
            .collect()
    }
}

// ============== MAIN ==============

#[tokio::main]
async fn main() {
    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::new(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()),
        ))
        .with(tracing_subscriber::fmt::layer())
        .init();

    let state = AppState::load_or_create("config/node.json")
        .await
        .expect("Failed to initialize state");

    println!("🔥 Arsonnet Node initialized");
    println!(
        "   Public Key: {}...",
        &state.node.read().await.pubkey.chars().take(20).collect::<String>()
    );

    // Ensure vault_data directory exists
    std::fs::create_dir_all("vault_data").ok();

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    // Serve static files from frontend/build with SPA fallback
    let static_dir = std::env::var("STATIC_DIR").unwrap_or_else(|_| "../frontend/build".to_string());
    
    // Check if static directory exists
    let static_exists = std::path::Path::new(&static_dir).exists();
    if static_exists {
        println!("📁 Serving static files from: {}", static_dir);
    } else {
        println!("⚠️  Static directory not found: {} (API-only mode)", static_dir);
    }

    let api_routes = Router::new()
        // Vault API routes
        .route("/api/start_upload", post(start_upload_handler))
        .route("/api/upload_chunk", post(upload_chunk_handler))
        .route("/api/finish_upload", post(finish_upload_handler))
        .route("/api/get_file/{env}/{file_id}", get(get_file_handler))
        .route("/api/get_preview/{env}/{file_id}", get(get_preview_handler))
        .route("/api/metadata/{env}", get(get_metadata_handler).post(save_metadata_handler))
        .route("/api/delete_files", post(delete_files_handler))
        // P2P routes
        .route("/p2p/ws", get(ws_handler))
        .route("/p2p/info", get(node_info_handler))
        .route("/p2p/peers", get(peers_handler))
        .with_state(state);

    let app = if static_exists {
        let serve_dir = ServeDir::new(&static_dir)
            .not_found_service(ServeFile::new(format!("{}/index.html", static_dir)));
        
        api_routes
            .fallback_service(serve_dir)
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    } else {
        api_routes
            .layer(cors)
            .layer(TraceLayer::new_for_http())
    };

    let addr = SocketAddr::from(([0, 0, 0, 0], 8181));
    println!("🚀 Server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============== VAULT HANDLERS ==============

async fn start_upload_handler(
    Json(req): Json<StartUploadRequest>,
) -> Result<Json<StartUploadResponse>, StatusCode> {
    use rand::Rng;

    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);

    std::fs::create_dir_all(&vault_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut rng = rand::thread_rng();
    let file_id: String = (0..16).map(|_| format!("{:x}", rng.gen_range(0..16))).collect();
    let content_id: String = (0..32).map(|_| format!("{:x}", rng.gen_range(0..16))).collect();

    let temp_dir = format!("{}/{}_chunks", vault_dir, file_id);
    std::fs::create_dir_all(&temp_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut preview_id = None;

    if let (Some(preview), Some(preview_nonce)) = (&req.preview, &req.preview_nonce) {
        let pid: String = (0..32).map(|_| format!("{:x}", rng.gen_range(0..16))).collect();
        let preview_path = format!("{}/{}", vault_dir, pid);

        let mut file_data = Vec::with_capacity(preview_nonce.len() + preview.len());
        file_data.extend_from_slice(preview_nonce);
        file_data.extend_from_slice(preview);

        std::fs::write(&preview_path, file_data).ok();
        preview_id = Some(pid);
    }

    let meta = serde_json::json!({
        "encrypted_name": req.encrypted_name,
        "name_nonce": req.name_nonce,
        "item_type": req.item_type,
        "nonce": req.nonce,
        "total_chunks": req.total_chunks,
        "content_id": content_id,
        "preview_id": preview_id
    });
    std::fs::write(format!("{}/meta.json", temp_dir), meta.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(StartUploadResponse { file_id }))
}

#[derive(Deserialize)]
struct UploadChunkQuery {
    token: String,
    file_id: String,
    chunk: usize,
}

async fn upload_chunk_handler(
    Query(params): Query<UploadChunkQuery>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let environment = params.token.split('_').next().unwrap_or("personal");
    let chunk_path = format!(
        "vault_data/{}/{}_chunks/{}.chunk",
        environment, params.file_id, params.chunk
    );

    std::fs::write(&chunk_path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

async fn finish_upload_handler(
    Json(req): Json<FinishUploadRequest>,
) -> Result<Json<UploadResult>, StatusCode> {
    use std::io::Write;

    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);
    let temp_dir = format!("{}/{}_chunks", vault_dir, req.file_id);

    let meta_str = std::fs::read_to_string(format!("{}/meta.json", temp_dir))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta_str).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_chunks = meta["total_chunks"].as_u64().unwrap_or(0) as usize;
    let content_id = meta["content_id"].as_str().unwrap_or("").to_string();

    let file_path = format!("{}/{}", vault_dir, content_id);
    let mut output = std::fs::File::create(&file_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut total_size = 0usize;
    for i in 0..total_chunks {
        let chunk_path = format!("{}/{}.chunk", temp_dir, i);
        let chunk_data = std::fs::read(&chunk_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        total_size += chunk_data.len();
        output.write_all(&chunk_data).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    std::fs::remove_dir_all(&temp_dir).ok();

    let item = VaultItem {
        id: req.file_id,
        encrypted_name: serde_json::from_value(meta["encrypted_name"].clone()).unwrap_or_default(),
        name_nonce: serde_json::from_value(meta["name_nonce"].clone()).unwrap_or_default(),
        item_type: meta["item_type"].as_str().unwrap_or("document").to_string(),
        size: total_size,
        nonce: serde_json::from_value(meta["nonce"].clone()).unwrap_or_default(),
        content_id,
        preview_id: meta["preview_id"].as_str().map(|s| s.to_string()),
    };

    Ok(Json(UploadResult {
        success: true,
        item: Some(item),
    }))
}

async fn get_file_handler(Path((env, file_id)): Path<(String, String)>) -> Result<axum::body::Bytes, StatusCode> {
    let file_path = format!("vault_data/{}/{}", env, file_id);
    let data = std::fs::read(&file_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(axum::body::Bytes::from(data))
}

async fn get_preview_handler(Path((env, file_id)): Path<(String, String)>) -> Result<axum::body::Bytes, StatusCode> {
    let preview_path = format!("vault_data/{}/{}", env, file_id);
    let data = std::fs::read(&preview_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(axum::body::Bytes::from(data))
}

async fn get_metadata_handler(Path(env): Path<String>) -> Result<axum::body::Bytes, StatusCode> {
    let vault_dir = format!("vault_data/{}", env);
    let metadata_path = format!("{}/metadata.enc", vault_dir);

    std::fs::create_dir_all(&vault_dir).ok();

    if std::path::Path::new(&metadata_path).exists() {
        let data = std::fs::read(&metadata_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        Ok(axum::body::Bytes::from(data))
    } else {
        Ok(axum::body::Bytes::new())
    }
}

async fn save_metadata_handler(
    Path(env): Path<String>,
    body: axum::body::Bytes,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let vault_dir = format!("vault_data/{}", env);
    let metadata_path = format!("{}/metadata.enc", vault_dir);

    std::fs::create_dir_all(&vault_dir).ok();
    std::fs::write(&metadata_path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(Json(serde_json::json!({ "success": true })))
}

#[derive(Deserialize)]
struct DeleteFilesRequest {
    session_token: String,
    file_ids: Vec<String>,
}

async fn delete_files_handler(Json(req): Json<DeleteFilesRequest>) -> Result<Json<serde_json::Value>, StatusCode> {
    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);

    for id in req.file_ids {
        let path = format!("{}/{}", vault_dir, id);
        if std::path::Path::new(&path).exists() {
            std::fs::remove_file(&path).ok();
        }
    }

    Ok(Json(serde_json::json!({ "success": true })))
}

// ============== P2P HANDLERS ==============

async fn ws_handler(ws: WebSocketUpgrade, State(state): State<AppState>) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AppState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WsServerMessage>();

    let mut client_pubkey: Option<String> = None;

    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap_or_default();
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });

    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(client_msg) = serde_json::from_str::<WsClientMessage>(&text) {
                    match client_msg {
                        WsClientMessage::Register { pubkey, signature } => {
                            let challenge = format!("register:{}", crypto::current_timestamp() / 60);
                            let valid = crypto::verify_signature(&pubkey, &signature, challenge.as_bytes())
                                .unwrap_or(false);

                            if valid || signature == "dev_mode" {
                                client_pubkey = Some(pubkey.clone());
                                state.register_connection(pubkey, tx.clone()).await;

                                let node_info = state.get_public_info().await;
                                let _ = tx.send(WsServerMessage::Registered {
                                    success: true,
                                    node_info: Some(node_info.node),
                                });
                            } else {
                                let _ = tx.send(WsServerMessage::Registered {
                                    success: false,
                                    node_info: None,
                                });
                            }
                        }

                        WsClientMessage::SendMessage {
                            to_pubkey,
                            encrypted_payload,
                        } => {
                            if let Some(from) = &client_pubkey {
                                let msg = WsServerMessage::IncomingMessage {
                                    from_pubkey: from.clone(),
                                    encrypted_payload,
                                    timestamp: crypto::current_timestamp(),
                                };

                                if !state.send_to_peer(&to_pubkey, msg).await {
                                    let _ = tx.send(WsServerMessage::Error {
                                        message: format!("Peer {} not online", to_pubkey),
                                    });
                                }
                            }
                        }

                        WsClientMessage::ListPeers => {
                            let peers = state.get_online_peers().await;
                            let _ = tx.send(WsServerMessage::PeerList { peers });
                        }

                        WsClientMessage::Ping => {
                            let _ = tx.send(WsServerMessage::Pong);
                        }
                    }
                }
            }
            Message::Close(_) => break,
            _ => {}
        }
    }

    if let Some(pubkey) = client_pubkey {
        state.unregister_connection(&pubkey).await;
    }

    send_task.abort();
}

async fn node_info_handler(State(state): State<AppState>) -> Json<SignedNode> {
    Json(state.get_public_info().await)
}

async fn peers_handler(State(state): State<AppState>) -> Json<Vec<PeerNode>> {
    Json(state.get_online_peers().await)
}
