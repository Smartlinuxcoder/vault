use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

// ============== VAULT TYPES ==============

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct VaultItem {
    pub id: String,
    pub encrypted_name: Vec<u8>,
    pub name_nonce: Vec<u8>,
    pub item_type: String,
    pub size: usize,
    pub nonce: Vec<u8>,
    pub content_id: String,
    pub preview_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StartUploadRequest {
    pub session_token: String,
    pub encrypted_name: Vec<u8>,
    pub name_nonce: Vec<u8>,
    pub item_type: String,
    pub nonce: Vec<u8>,
    pub total_chunks: usize,
    pub preview: Option<Vec<u8>>,
    pub preview_nonce: Option<Vec<u8>>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StartUploadResponse {
    pub file_id: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct FinishUploadRequest {
    pub session_token: String,
    pub file_id: String,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct UploadResult {
    pub success: bool,
    pub item: Option<VaultItem>,
}

// ============== NODE TYPES ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub pubkey: String,
    #[serde(default, skip_serializing)]
    pub privkey: String,
    pub address: String,
    pub listen_port: u16,
    pub public_port: u16,
    pub secure: bool,
    pub version: Option<String>,
    pub peers: Vec<PeerNode>,
    pub ping_interval: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerNode {
    pub name: Option<String>,
    pub pubkey: String,
    pub address: String,
    pub public_port: u16,
    pub secure: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<u64>,
    #[serde(default)]
    pub is_connected: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedNode {
    pub node: PeerNode,
    pub signature: String,
}

// ============== WEBSOCKET TYPES ==============

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsClientMessage {
    Register { pubkey: String, signature: String },
    SendMessage { to_pubkey: String, encrypted_payload: Vec<u8> },
    ListPeers,
    Ping,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsServerMessage {
    Registered { success: bool, node_info: Option<PeerNode> },
    IncomingMessage { from_pubkey: String, encrypted_payload: Vec<u8>, timestamp: u64 },
    PeerList { peers: Vec<PeerNode> },
    PeerStatus { pubkey: String, online: bool },
    Pong,
    Error { message: String },
}

// ============== CONNECTION STATE ==============

#[derive(Debug)]
pub struct ConnectionState {
    pub pubkey: String,
    pub tx: mpsc::UnboundedSender<WsServerMessage>,
    pub connected_at: u64,
}

impl Clone for ConnectionState {
    fn clone(&self) -> Self {
        Self {
            pubkey: self.pubkey.clone(),
            tx: self.tx.clone(),
            connected_at: self.connected_at,
        }
    }
}
