#![cfg(feature = "server")]

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use once_cell::sync::OnceCell;
use dioxus::server::axum::{
    extract::ws::{Message, WebSocket, WebSocketUpgrade},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};

use crate::p2p_types::{
    ConnectionState, Node, PeerNode, SignedNode,
    WsClientMessage, WsServerMessage,
};
use crate::p2p_crypto::{self, current_timestamp, verify_signature};

// Stato globale P2P
static GLOBAL_P2P_STATE: OnceCell<P2PState> = OnceCell::new();

/// Inizializza lo stato globale P2P
pub fn init_global_state(state: P2PState) {
    let _ = GLOBAL_P2P_STATE.set(state);
}

/// Ottiene lo stato globale P2P
fn get_global_state() -> &'static P2PState {
    GLOBAL_P2P_STATE.get().expect("P2P state not initialized")
}

/// Stato condiviso del server P2P
#[derive(Clone)]
pub struct P2PState {
    /// Configurazione del nodo locale
    pub node: Arc<RwLock<Node>>,
    /// Connessioni attive: pubkey -> ConnectionState
    pub connections: Arc<RwLock<HashMap<String, ConnectionState>>>,
}

impl P2PState {
    pub fn new(node: Node) -> Self {
        Self {
            node: Arc::new(RwLock::new(node)),
            connections: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Carica o crea la configurazione del nodo
    pub async fn load_or_create(config_path: &str) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        use std::path::Path;
        
        let node = if Path::new(config_path).exists() {
            let data = std::fs::read_to_string(config_path)?;
            let mut node: Node = serde_json::from_str(&data)?;
            
            // Carica la chiave privata se esiste
            let privkey_path = "config/node_privkey.pem";
            if Path::new(privkey_path).exists() {
                node.privkey = std::fs::read_to_string(privkey_path)?;
                // Verifica che la pubkey corrisponda
                let derived = p2p_crypto::derive_pubkey(&node.privkey)?;
                if derived != node.pubkey {
                    return Err("Public key mismatch!".into());
                }
            }
            
            node.version = Some(env!("CARGO_PKG_VERSION").to_string());
            node
        } else {
            // Crea un nuovo nodo
            let (pubkey, privkey) = p2p_crypto::generate_keypair()?;
            
            let node = Node {
                name: "VaultNode".to_string(),
                pubkey,
                privkey: privkey.clone(),
                address: "0.0.0.0".to_string(),
                listen_port: 8080,
                public_port: 8080,
                secure: false,
                version: Some(env!("CARGO_PKG_VERSION").to_string()),
                peers: Vec::new(),
                ping_interval: 30,
            };
            
            // Salva la configurazione
            if let Some(parent) = Path::new(config_path).parent() {
                std::fs::create_dir_all(parent)?;
            }
            
            // Salva config senza privkey
            let mut save_node = node.clone();
            save_node.privkey = String::new();
            std::fs::write(config_path, serde_json::to_string_pretty(&save_node)?)?;
            
            // Salva privkey separatamente
            std::fs::write("config/node_privkey.pem", &privkey)?;
            
            node
        };
        
        Ok(Self::new(node))
    }

    /// Ottiene info pubbliche del nodo
    pub async fn get_public_info(&self) -> SignedNode {
        let node = self.node.read().await;
        let peer_node = PeerNode {
            name: Some(node.name.clone()),
            pubkey: node.pubkey.clone(),
            address: node.address.clone(),
            public_port: node.public_port,
            secure: node.secure,
            version: node.version.clone(),
            last_seen: Some(current_timestamp()),
            is_connected: true,
            peers: Some(node.peers.clone()),
        };
        
        let node_json = serde_json::to_string(&peer_node).unwrap_or_default();
        let signature = p2p_crypto::sign_data(&node.privkey, node_json.as_bytes())
            .unwrap_or_default();
        
        SignedNode {
            node: peer_node,
            signature,
        }
    }

    /// Registra una nuova connessione
    pub async fn register_connection(&self, pubkey: String, tx: mpsc::UnboundedSender<WsServerMessage>) {
        let state = ConnectionState {
            pubkey: pubkey.clone(),
            tx: tx.clone(),
            connected_at: current_timestamp(),
        };
        
        self.connections.write().await.insert(pubkey.clone(), state);
        
        // Notifica tutti gli altri peer
        self.broadcast_peer_status(&pubkey, true).await;
    }

    /// Rimuove una connessione
    pub async fn unregister_connection(&self, pubkey: &str) {
        self.connections.write().await.remove(pubkey);
        self.broadcast_peer_status(pubkey, false).await;
    }

    /// Invia messaggio a un peer specifico
    pub async fn send_to_peer(&self, to_pubkey: &str, message: WsServerMessage) -> bool {
        if let Some(conn) = self.connections.read().await.get(to_pubkey) {
            conn.tx.send(message).is_ok()
        } else {
            false
        }
    }

    /// Notifica tutti i peer dello stato di un utente
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

    /// Ottiene la lista dei peer online
    pub async fn get_online_peers(&self) -> Vec<PeerNode> {
        let connections = self.connections.read().await;
        connections.values().map(|conn| {
            PeerNode {
                name: None,
                pubkey: conn.pubkey.clone(),
                address: String::new(),
                public_port: 0,
                secure: false,
                version: None,
                last_seen: Some(conn.connected_at),
                is_connected: true,
                peers: None,
            }
        }).collect()
    }
}

/// Handler WebSocket globale (senza State)
pub async fn ws_handler_global(ws: WebSocketUpgrade) -> impl IntoResponse {
    let state = get_global_state().clone();
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

/// Gestisce una connessione WebSocket
async fn handle_socket(socket: WebSocket, state: P2PState) {
    let (mut sender, mut receiver) = socket.split();
    let (tx, mut rx) = mpsc::unbounded_channel::<WsServerMessage>();
    
    let mut client_pubkey: Option<String> = None;
    
    // Task per inviare messaggi al client
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            let json = serde_json::to_string(&msg).unwrap_or_default();
            if sender.send(Message::Text(json.into())).await.is_err() {
                break;
            }
        }
    });
    
    // Gestisci messaggi in arrivo
    while let Some(Ok(msg)) = receiver.next().await {
        match msg {
            Message::Text(text) => {
                if let Ok(client_msg) = serde_json::from_str::<WsClientMessage>(&text) {
                    match client_msg {
                        WsClientMessage::Register { pubkey, signature } => {
                            // Verifica la firma (dev_mode per test)
                            let challenge = format!("register:{}", current_timestamp() / 60);
                            let valid = verify_signature(&pubkey, &signature, challenge.as_bytes())
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
                        
                        WsClientMessage::SendMessage { to_pubkey, encrypted_payload } => {
                            if let Some(from) = &client_pubkey {
                                let msg = WsServerMessage::IncomingMessage {
                                    from_pubkey: from.clone(),
                                    encrypted_payload,
                                    timestamp: current_timestamp(),
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
    
    // Cleanup
    if let Some(pubkey) = client_pubkey {
        state.unregister_connection(&pubkey).await;
    }
    
    send_task.abort();
}

/// Handler HTTP per info del nodo (globale)
pub async fn node_info_handler_global() -> dioxus::server::axum::Json<SignedNode> {
    let state = get_global_state();
    dioxus::server::axum::Json(state.get_public_info().await)
}

/// Handler HTTP per lista peer online (globale)
pub async fn peers_handler_global() -> dioxus::server::axum::Json<Vec<PeerNode>> {
    let state = get_global_state();
    dioxus::server::axum::Json(state.get_online_peers().await)
}
