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

/// Protocollo di connessione per peer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Default)]
#[serde(rename_all = "lowercase")]
pub enum PeerProtocol {
    /// WebSocket non sicuro (ws://)
    Ws,
    /// WebSocket sicuro (wss://)
    Wss,
    /// Protocollo Arson nativo su TCP (bincode)
    #[default]
    Arson,
}

impl std::fmt::Display for PeerProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PeerProtocol::Ws => write!(f, "ws"),
            PeerProtocol::Wss => write!(f, "wss"),
            PeerProtocol::Arson => write!(f, "arson"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub pubkey: String,
    #[serde(default, skip_serializing)]
    pub privkey: String,
    pub address: String,
    /// Porta HTTP/WebSocket (API e ws://)
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    /// Porta pubblica HTTP/WebSocket (dietro reverse proxy)
    #[serde(default = "default_http_port")]
    pub public_http_port: u16,
    /// Porta TCP per protocollo Arson (P2P nativo)
    #[serde(default = "default_arson_port")]
    pub arson_port: u16,
    /// Porta pubblica Arson (dietro NAT/firewall)
    #[serde(default = "default_arson_port")]
    pub public_arson_port: u16,
    /// Se il nodo usa HTTPS/WSS
    pub secure: bool,
    pub version: Option<String>,
    pub peers: Vec<PeerConfig>,
    pub ping_interval: u64,
    /// Modalità relay: il nodo non ha IP pubblico e si connette a un relay
    #[serde(default)]
    pub relay_mode: bool,
    /// Nodo relay a cui connettersi (pubkey del peer in peers[])
    #[serde(default)]
    pub relay_node: Option<String>,
    // Campi legacy per retrocompatibilità
    #[serde(default, skip_serializing)]
    pub listen_port: u16,
    #[serde(default, skip_serializing)]
    pub public_port: u16,
}

fn default_http_port() -> u16 { 8181 }
fn default_arson_port() -> u16 { 3000 }

impl Node {
    /// Migra configurazione legacy se necessario
    pub fn migrate_legacy(&mut self) {
        if self.http_port == 0 && self.listen_port != 0 {
            // Vecchia config: listen_port era per HTTP
            self.http_port = 8181;
            self.public_http_port = if self.secure { 443 } else { self.public_port };
            self.arson_port = self.listen_port;
            self.public_arson_port = self.listen_port;
        }
    }
}

/// Configurazione di un peer nel file config
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerConfig {
    pub name: Option<String>,
    pub pubkey: String,
    pub address: String,
    /// Porta da usare (interpretata in base al protocollo)
    pub port: u16,
    /// Protocollo di connessione
    #[serde(default)]
    pub protocol: PeerProtocol,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PeerNode {
    pub name: Option<String>,
    pub pubkey: String,
    pub address: String,
    /// Porta HTTP/WebSocket pubblica
    #[serde(default)]
    pub http_port: u16,
    /// Porta Arson TCP pubblica
    #[serde(default)]
    pub arson_port: u16,
    /// Se supporta HTTPS/WSS
    pub secure: bool,
    /// Protocolli supportati
    #[serde(default)]
    pub protocols: Vec<PeerProtocol>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_seen: Option<u64>,
    #[serde(default)]
    pub is_connected: bool,
    // Legacy field for compatibility
    #[serde(default, skip_serializing)]
    pub public_port: u16,
}

impl PeerNode {
    /// Crea PeerNode da PeerConfig
    pub fn from_config(config: &PeerConfig) -> Self {
        let (http_port, arson_port) = match config.protocol {
            PeerProtocol::Ws | PeerProtocol::Wss => (config.port, 0),
            PeerProtocol::Arson => (0, config.port),
        };
        
        Self {
            name: config.name.clone(),
            pubkey: config.pubkey.clone(),
            address: config.address.clone(),
            http_port,
            arson_port,
            secure: matches!(config.protocol, PeerProtocol::Wss),
            protocols: vec![config.protocol.clone()],
            version: None,
            last_seen: None,
            is_connected: false,
            public_port: config.port,
        }
    }

    /// Ottiene l'URL WebSocket per questo peer
    pub fn ws_url(&self) -> Option<String> {
        if self.http_port > 0 {
            let proto = if self.secure { "wss" } else { "ws" };
            Some(format!("{}://{}:{}/p2p/ws", proto, self.address, self.http_port))
        } else {
            None
        }
    }

    /// Ottiene l'indirizzo TCP Arson per questo peer
    pub fn arson_addr(&self) -> Option<String> {
        if self.arson_port > 0 {
            Some(format!("{}:{}", self.address, self.arson_port))
        } else {
            None
        }
    }
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
    SendMessage { to_pubkey: String, encrypted_payload: Vec<u8>, #[serde(default)] message_id: Option<String> },
    ListPeers,
    Ping,
    /// Registra questo nodo come relay client (per nodi senza IP pubblico)
    RegisterAsNode { node: SignedNode, x25519_pubkey: [u8; 32] },
    /// Inoltra un messaggio attraverso il relay
    RelayMessage { to_pubkey: String, message_type: String, payload: Vec<u8> },
    /// Conferma ricezione messaggio
    MessageAck { to_pubkey: String, message_id: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsServerMessage {
    Registered { success: bool, node_info: Option<PeerNode> },
    IncomingMessage { from_pubkey: String, encrypted_payload: Vec<u8>, timestamp: u64, #[serde(default)] message_id: Option<String> },
    PeerList { peers: Vec<PeerNode> },
    PeerStatus { pubkey: String, online: bool },
    Pong,
    Error { message: String },
    /// Messaggio relayato da un altro nodo
    RelayedMessage { from_node: String, from_pubkey: String, message_type: String, payload: Vec<u8>, timestamp: u64, #[serde(default)] message_id: Option<String> },
    /// Conferma registrazione come nodo relay client
    NodeRegistered { success: bool },
    /// Conferma ricezione messaggio (ACK)
    MessageAck { from_pubkey: String, message_id: String },
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

// ============== ONION ROUTING TYPES ==============

/// Pacchetto onion - ogni strato contiene dati criptati per un hop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnionPacket {
    /// ID univoco del pacchetto per evitare replay
    pub packet_id: [u8; 16],
    /// Chiave pubblica effimera per ECDH (X25519)
    pub ephemeral_pubkey: [u8; 32],
    /// Payload criptato (contiene OnionLayer o payload finale)
    pub encrypted_payload: Vec<u8>,
    /// Nonce per AES-GCM
    pub nonce: [u8; 12],
}

/// Layer interno dell'onion - decriptato ad ogni hop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnionLayer {
    /// Prossimo hop (None se siamo la destinazione finale)
    pub next_hop: Option<NextHop>,
    /// Payload per questo hop o per il prossimo layer
    pub inner_packet: Vec<u8>,
}

/// Informazioni sul prossimo hop
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NextHop {
    pub address: String,
    pub port: u16,
    pub pubkey: String,
}

/// Risposta da un nodo relay
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OnionResponse {
    pub packet_id: [u8; 16],
    pub encrypted_response: Vec<u8>,
    pub nonce: [u8; 12],
}

/// Messaggio P2P routato attraverso la rete onion
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoutedMessage {
    pub message_type: RoutedMessageType,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RoutedMessageType {
    /// Messaggio di chat criptato
    Chat,
    /// Richiesta di file
    FileRequest,
    /// Chunk di file
    FileChunk,
    /// Discovery di peer
    PeerDiscovery,
    /// Ping per mantenere il circuito attivo
    KeepAlive,
}

// ============== DISCOVERY TYPES ==============

/// Stato di un peer conosciuto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnownPeer {
    pub node: PeerNode,
    pub x25519_pubkey: Option<[u8; 32]>,
    pub last_ping: u64,
    pub latency_ms: Option<u32>,
    pub trust_score: u8,
    pub failed_attempts: u32,
}

/// Messaggio di discovery tra nodi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DiscoveryMessage {
    /// Annuncio della propria presenza
    Announce {
        node: SignedNode,
        x25519_pubkey: [u8; 32],
    },
    /// Richiesta lista peer
    GetPeers {
        max_count: u16,
    },
    /// Risposta con lista peer
    PeerList {
        peers: Vec<KnownPeer>,
    },
    /// Ping per verificare che il nodo sia attivo
    Ping {
        timestamp: u64,
        nonce: [u8; 8],
    },
    /// Risposta al ping
    Pong {
        timestamp: u64,
        nonce: [u8; 8],
        original_timestamp: u64,
    },
}

/// Pacchetto bincode per comunicazione inter-nodo
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum NodePacket {
    /// Pacchetto onion da routare
    Onion(OnionPacket),
    /// Risposta onion
    OnionResponse(OnionResponse),
    /// Messaggio di discovery
    Discovery(DiscoveryMessage),
}
