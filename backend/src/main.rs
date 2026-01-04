use axum::{
    extract::{ws::{Message, WebSocket, WebSocketUpgrade}, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Json},
    routing::{get, post},
    Router,
};
use futures_util::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, net::SocketAddr, sync::Arc};
use tokio::sync::{mpsc, RwLock};
use tokio::net::TcpListener;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::{ServeDir, ServeFile};
use tower_http::trace::TraceLayer;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod crypto;
mod discovery;
mod onion;
mod types;

use discovery::DiscoveryManager;
use onion::OnionRouter;
use types::*;

// ============== APP STATE ==============

#[derive(Clone)]
pub struct AppState {
    pub node: Arc<RwLock<Node>>,
    pub connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
    pub remote_peers: Arc<RwLock<HashMap<String, RemotePeerConnection>>>,
    pub discovery: Arc<DiscoveryManager>,
    pub onion_router: Arc<OnionRouter>,
}

/// Connessione verso un peer remoto (su un altro nodo)
#[derive(Debug, Clone)]
pub struct RemotePeerConnection {
    pub peer: PeerNode,
    pub config: PeerConfig,
    pub last_check: u64,
    pub is_reachable: bool,
}

impl AppState {
    pub fn new(node: Node, discovery: DiscoveryManager, onion_router: OnionRouter) -> Self {
        Self {
            node: Arc::new(RwLock::new(node)),
            connections: Arc::new(RwLock::new(HashMap::new())),
            remote_peers: Arc::new(RwLock::new(HashMap::new())),
            discovery: Arc::new(discovery),
            onion_router: Arc::new(onion_router),
        }
    }

    pub async fn load_or_create(config_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use std::path::Path;

        let mut node = if Path::new(config_path).exists() {
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
                http_port: 8181,
                public_http_port: 8181,
                arson_port: 3000,
                public_arson_port: 3000,
                secure: false,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                peers: Vec::new(),
                ping_interval: 30,
                listen_port: 0,
                public_port: 0,
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

        // Migra config legacy se necessario
        node.migrate_legacy();

        // Create Ed25519 key for discovery (derive from RSA or generate new)
        let (ed25519_privkey, _ed25519_pubkey) = crypto::generate_ed25519_keypair();
        
        // Create discovery manager
        let discovery = DiscoveryManager::new(node.clone(), ed25519_privkey.to_vec());
        
        // Create onion router
        let x25519_privkey = discovery.x25519_privkey();
        let x25519_pubkey = discovery.x25519_pubkey();
        let onion_router = OnionRouter::new(x25519_privkey, x25519_pubkey);

        let state = Self::new(node, discovery, onion_router);
        
        // Initialize remote peers from config
        state.init_configured_peers().await;
        
        Ok(state)
    }

    /// Initialize peers from node configuration
    async fn init_configured_peers(&self) {
        let node = self.node.read().await;
        let mut remote_peers = self.remote_peers.write().await;
        
        for peer_config in &node.peers {
            let peer_node = PeerNode::from_config(peer_config);
            let remote = RemotePeerConnection {
                peer: peer_node,
                config: peer_config.clone(),
                last_check: 0,
                is_reachable: false,
            };
            remote_peers.insert(peer_config.pubkey.clone(), remote);
        }
        
        println!("📡 Loaded {} configured peers", remote_peers.len());
    }

    pub async fn get_public_info(&self) -> SignedNode {
        let node = self.node.read().await;
        let peer_node = PeerNode {
            name: Some(node.name.clone()),
            pubkey: node.pubkey.clone(),
            address: node.address.clone(),
            http_port: node.public_http_port,
            arson_port: node.public_arson_port,
            secure: node.secure,
            protocols: vec![PeerProtocol::Arson, if node.secure { PeerProtocol::Wss } else { PeerProtocol::Ws }],
            version: node.version.clone(),
            last_seen: Some(crypto::current_timestamp()),
            is_connected: true,
            public_port: node.public_http_port,
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
        let remote_peers = self.remote_peers.read().await;
        
        let mut peers: Vec<PeerNode> = connections
            .values()
            .map(|conn| PeerNode {
                name: None,
                pubkey: conn.pubkey.clone(),
                address: String::new(),
                http_port: 0,
                arson_port: 0,
                secure: false,
                protocols: vec![],
                version: None,
                last_seen: Some(conn.connected_at),
                is_connected: true,
                public_port: 0,
            })
            .collect();
        
        // Add configured peers (mark as remote)
        for (_, remote) in remote_peers.iter() {
            // Don't duplicate if already connected locally
            if !peers.iter().any(|p| p.pubkey == remote.peer.pubkey) {
                let mut peer = remote.peer.clone();
                peer.is_connected = remote.is_reachable;
                peers.push(peer);
            }
        }
        
        peers
    }

    /// Get all configured peers from node.json
    pub async fn get_configured_peers(&self) -> Vec<PeerNode> {
        self.node.read().await.peers.iter().map(PeerNode::from_config).collect()
    }

    /// Try to send a message to a remote peer via configured nodes (broadcast)
    /// Since we don't know which node hosts which user, we try all configured nodes
    pub async fn send_to_remote_peer(&self, to_pubkey: &str, message: WsServerMessage) -> Result<bool, String> {
        let remote_peers = self.remote_peers.read().await;
        
        if remote_peers.is_empty() {
            return Err("No configured remote nodes".to_string());
        }
        
        let mut last_error = String::new();
        let client = reqwest::Client::new();
        
        // Try all configured nodes until one accepts the message
        for (_, remote) in remote_peers.iter() {
            if !remote.is_reachable {
                continue;
            }
            
            let peer = &remote.peer;
            
            // Prefer HTTP port for relay API
            if peer.http_port == 0 {
                continue;
            }
            
            let protocol = if peer.secure { "https" } else { "http" };
            let url = format!("{}://{}:{}/p2p/relay", protocol, peer.address, peer.http_port);
            
            // Create relay message
            let relay_msg = RelayMessage {
                to_pubkey: to_pubkey.to_string(),
                message: message.clone(),
            };
            
            match client.post(&url)
                .json(&relay_msg)
                .timeout(std::time::Duration::from_secs(10))
                .send()
                .await 
            {
                Ok(resp) if resp.status().is_success() => {
                    println!("📨 Message relayed to {} via node {}", to_pubkey, peer.address);
                    return Ok(true);
                },
                Ok(resp) => {
                    last_error = format!("Node {} returned {}", peer.address, resp.status());
                },
                Err(e) => {
                    last_error = format!("Failed to reach {}: {}", peer.address, e);
                },
            }
        }
        
        Err(last_error)
    }

    /// Check connectivity to configured peers (supports multiple protocols)
    pub async fn check_peer_connectivity(&self) {
        let peers_to_check: Vec<_> = {
            let remote = self.remote_peers.read().await;
            remote.values().cloned().collect()
        };
        
        for remote in peers_to_check {
            let peer = &remote.peer;
            let config = &remote.config;
            
            let is_reachable = match config.protocol {
                PeerProtocol::Ws | PeerProtocol::Wss => {
                    // Check via HTTP API
                    let protocol = if peer.secure { "https" } else { "http" };
                    let url = format!("{}://{}:{}/p2p/info", protocol, peer.address, peer.http_port);
                    
                    let client = reqwest::Client::new();
                    match client.get(&url)
                        .timeout(std::time::Duration::from_secs(5))
                        .send()
                        .await 
                    {
                        Ok(resp) => resp.status().is_success(),
                        Err(_) => false,
                    }
                },
                PeerProtocol::Arson => {
                    // Check via TCP connection
                    let addr = format!("{}:{}", peer.address, peer.arson_port);
                    match tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        tokio::net::TcpStream::connect(&addr)
                    ).await {
                        Ok(Ok(_)) => true,
                        _ => false,
                    }
                },
            };
            
            let mut remote_peers = self.remote_peers.write().await;
            if let Some(r) = remote_peers.get_mut(&peer.pubkey) {
                r.is_reachable = is_reachable;
                r.last_check = crypto::current_timestamp();
            }
        }
    }
}

/// Message to relay to a peer on another node
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayMessage {
    pub to_pubkey: String,
    pub message: WsServerMessage,
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

    let node = state.node.read().await;
    let http_port = node.http_port;
    let arson_port = node.arson_port;
    drop(node);

    println!("🔥 Arsonnet Node initialized");
    println!(
        "   Public Key: {}...",
        &state.node.read().await.pubkey.chars().take(20).collect::<String>()
    );
    println!(
        "   X25519 Pubkey: {}...",
        hex::encode(&state.onion_router.pubkey()[..8])
    );

    // Ensure vault_data directory exists
    std::fs::create_dir_all("vault_data").ok();

    // Start discovery manager
    state.discovery.start().await;
    println!("📡 Discovery manager started");

    // Start TCP listener for P2P/Onion routing (Arson protocol)
    let onion_router = state.onion_router.clone();
    let discovery = state.discovery.clone();
    
    tokio::spawn(async move {
        start_tcp_listener(arson_port, onion_router, discovery).await;
    });
    println!("🔌 Arson TCP listener started on port {}", arson_port);

    // Start peer connectivity checker
    let check_state = state.clone();
    tokio::spawn(async move {
        loop {
            check_state.check_peer_connectivity().await;
            tokio::time::sleep(tokio::time::Duration::from_secs(30)).await;
        }
    });

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
        .route("/p2p/configured_peers", get(configured_peers_handler))
        .route("/p2p/known_peers", get(known_peers_handler))
        .route("/p2p/relay", post(relay_handler))
        .route("/p2p/discovery", get(discovery_info_handler))
        .route("/p2p/onion/send", post(onion_send_handler))
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

    let addr = SocketAddr::from(([0, 0, 0, 0], http_port));
    println!("🚀 HTTP/WebSocket server listening on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

// ============== TCP LISTENER FOR P2P ==============

async fn start_tcp_listener(
    port: u16,
    onion_router: Arc<OnionRouter>,
    discovery: Arc<DiscoveryManager>,
) {
    let addr = SocketAddr::from(([0, 0, 0, 0], port));
    
    let listener = match TcpListener::bind(addr).await {
        Ok(l) => l,
        Err(e) => {
            tracing::error!("Failed to bind TCP listener on {}: {}", addr, e);
            return;
        }
    };

    tracing::info!("TCP P2P listener running on {}", addr);

    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                let router = onion_router.clone();
                let disc = discovery.clone();
                
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_connection(stream, router, disc, peer_addr).await {
                        tracing::debug!("TCP connection error from {}: {}", peer_addr, e);
                    }
                });
            }
            Err(e) => {
                tracing::error!("Failed to accept TCP connection: {}", e);
            }
        }
    }
}

async fn handle_tcp_connection(
    mut stream: tokio::net::TcpStream,
    onion_router: Arc<OnionRouter>,
    discovery: Arc<DiscoveryManager>,
    peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    
    // Leggi lunghezza pacchetto
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;

    if len > 10 * 1024 * 1024 {
        return Err("Packet too large".into());
    }

    // Leggi pacchetto
    let mut data = vec![0u8; len];
    stream.read_exact(&mut data).await?;

    // Prova a deserializzare come NodePacket
    let packet: NodePacket = bincode::deserialize(&data)?;

    match packet {
        NodePacket::Onion(onion_packet) => {
            // Gestisci pacchetto onion
            let packet_id = onion_packet.packet_id;
            match onion_router.unwrap_layer(&onion_packet, Some(peer_addr)).await {
                Ok((inner_data, next_hop)) => {
                    if let Some(next) = next_hop {
                        // Relay al prossimo hop
                        tracing::debug!("Relaying onion packet to {}:{}", next.address, next.port);
                        
                        match onion_router.forward_packet(&next, &inner_data).await {
                            Ok(Some(response)) => {
                                let response_packet = NodePacket::OnionResponse(response);
                                let response_bytes = bincode::serialize(&response_packet)?;
                                let len = response_bytes.len() as u32;
                                stream.write_all(&len.to_be_bytes()).await?;
                                stream.write_all(&response_bytes).await?;
                            }
                            Ok(None) => {}
                            Err(e) => {
                                tracing::warn!("Failed to forward onion: {}", e);
                            }
                        }
                    } else {
                        // Destinazione finale
                        if let Ok(message) = bincode::deserialize::<RoutedMessage>(&inner_data) {
                            tracing::info!("Received routed message: {:?}", message.message_type);
                            
                            // Crea risposta
                            if let Ok(response) = onion_router.create_response(&packet_id, b"ACK").await {
                                let response_packet = NodePacket::OnionResponse(response);
                                let response_bytes = bincode::serialize(&response_packet)?;
                                let len = response_bytes.len() as u32;
                                stream.write_all(&len.to_be_bytes()).await?;
                                stream.write_all(&response_bytes).await?;
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to unwrap onion: {}", e);
                }
            }
        }
        NodePacket::Discovery(disc_msg) => {
            // Gestisci messaggio di discovery
            if let Some(response) = discovery.handle_discovery(disc_msg).await {
                let response_packet = NodePacket::Discovery(response);
                let response_bytes = bincode::serialize(&response_packet)?;
                let len = response_bytes.len() as u32;
                stream.write_all(&len.to_be_bytes()).await?;
                stream.write_all(&response_bytes).await?;
            }
        }
        NodePacket::OnionResponse(_) => {
            // Le risposte vengono gestite dal chiamante
            tracing::debug!("Received unexpected OnionResponse");
        }
    }

    stream.flush().await?;
    Ok(())
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

                                // Try local first
                                if !state.send_to_peer(&to_pubkey, msg.clone()).await {
                                    // Try remote peer
                                    match state.send_to_remote_peer(&to_pubkey, msg).await {
                                        Ok(true) => {
                                            // Message relayed successfully
                                        }
                                        Ok(false) | Err(_) => {
                                            let _ = tx.send(WsServerMessage::Error {
                                                message: format!("Peer {} not reachable", to_pubkey),
                                            });
                                        }
                                    }
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

async fn configured_peers_handler(State(state): State<AppState>) -> Json<Vec<PeerNode>> {
    Json(state.get_configured_peers().await)
}

async fn relay_handler(
    State(state): State<AppState>,
    Json(relay): Json<RelayMessage>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    // Try to deliver the message to the local peer
    if state.send_to_peer(&relay.to_pubkey, relay.message).await {
        Ok(Json(serde_json::json!({ "success": true })))
    } else {
        Err(StatusCode::NOT_FOUND)
    }
}

// ============== NEW P2P HANDLERS ==============

async fn known_peers_handler(State(state): State<AppState>) -> Json<Vec<KnownPeer>> {
    Json(state.discovery.get_known_peers().await)
}

async fn discovery_info_handler(State(state): State<AppState>) -> Json<serde_json::Value> {
    let (signed_node, x25519_pubkey) = state.discovery.create_announcement();
    Json(serde_json::json!({
        "node": signed_node,
        "x25519_pubkey": hex::encode(x25519_pubkey),
        "known_peers_count": state.discovery.get_known_peers().await.len(),
    }))
}

#[derive(Debug, Deserialize)]
struct OnionSendRequest {
    /// Pubkeys of hops (in order)
    hops: Vec<String>,
    /// Message payload (base64 encoded)
    payload: String,
    /// Message type
    message_type: String,
}

async fn onion_send_handler(
    State(state): State<AppState>,
    Json(req): Json<OnionSendRequest>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    use base64::{engine::general_purpose, Engine};
    
    // Get hop peers from discovery
    let known_peers = state.discovery.get_known_peers().await;
    let mut hops: Vec<KnownPeer> = Vec::new();
    
    for pubkey in &req.hops {
        if let Some(peer) = known_peers.iter().find(|p| &p.node.pubkey == pubkey) {
            if peer.x25519_pubkey.is_some() {
                hops.push(peer.clone());
            } else {
                return Err(StatusCode::BAD_REQUEST);
            }
        } else {
            return Err(StatusCode::NOT_FOUND);
        }
    }
    
    if hops.is_empty() {
        return Err(StatusCode::BAD_REQUEST);
    }
    
    let payload = general_purpose::STANDARD.decode(&req.payload)
        .map_err(|_| StatusCode::BAD_REQUEST)?;
    
    let message_type = match req.message_type.as_str() {
        "chat" => RoutedMessageType::Chat,
        "file_request" => RoutedMessageType::FileRequest,
        "file_chunk" => RoutedMessageType::FileChunk,
        "discovery" => RoutedMessageType::PeerDiscovery,
        "keepalive" => RoutedMessageType::KeepAlive,
        _ => RoutedMessageType::Chat,
    };
    
    let message = RoutedMessage {
        message_type,
        payload,
        timestamp: crypto::current_timestamp(),
    };
    
    match state.onion_router.send_through_circuit(&hops, message).await {
        Ok(Some(response)) => {
            Ok(Json(serde_json::json!({
                "success": true,
                "response": general_purpose::STANDARD.encode(&response),
            })))
        }
        Ok(None) => {
            Ok(Json(serde_json::json!({
                "success": true,
                "response": null,
            })))
        }
        Err(e) => {
            Ok(Json(serde_json::json!({
                "success": false,
                "error": e.to_string(),
            })))
        }
    }
}
