use serde::{Deserialize, Serialize};
use std::collections::{HashSet, VecDeque};

#[cfg(feature = "server")]
use tokio::sync::mpsc;

// ============== NODE TYPES ==============

/// Rappresenta un nodo completo con chiave privata (solo server)
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

/// Nodo peer pubblico (senza chiave privata)
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
    #[serde(skip)]
    pub peers: Option<Vec<PeerNode>>,
}

/// Nodo firmato per verifica autenticità
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedNode {
    pub node: PeerNode,
    pub signature: String,
}

// ============== MESSAGE TYPES ==============

/// Messaggio crittografato per il routing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBlob {
    pub to_pubkey: String,
    pub payload: Vec<u8>,
    pub timestamp: u64,
}

/// Messaggio in chiaro (dopo decrittazione)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct P2PMessage {
    pub from_pubkey: String,
    pub from_name: Option<String>,
    pub content: MessageContent,
    pub timestamp: u64,
    pub signature: String,
}

/// Tipi di contenuto messaggi
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    FileOffer {
        file_id: String,
        file_name: String,
        file_size: usize,
        file_type: String,
    },
    FileAccept {
        file_id: String,
    },
    FileChunk {
        file_id: String,
        chunk_index: usize,
        total_chunks: usize,
        data: Vec<u8>,
    },
    Ping,
    Pong,
}

// ============== WEBSOCKET TYPES ==============

/// Messaggi WebSocket client -> server
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsClientMessage {
    /// Registra l'utente con la sua chiave pubblica
    Register {
        pubkey: String,
        signature: String,
    },
    /// Invia messaggio a un peer
    SendMessage {
        to_pubkey: String,
        encrypted_payload: Vec<u8>,
    },
    /// Richiedi lista peers connessi
    ListPeers,
    /// Ping per mantenere connessione
    Ping,
}

/// Messaggi WebSocket server -> client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum WsServerMessage {
    /// Conferma registrazione
    Registered {
        success: bool,
        node_info: Option<PeerNode>,
    },
    /// Messaggio in arrivo da un peer
    IncomingMessage {
        from_pubkey: String,
        encrypted_payload: Vec<u8>,
        timestamp: u64,
    },
    /// Lista peers online
    PeerList {
        peers: Vec<PeerNode>,
    },
    /// Notifica peer online/offline
    PeerStatus {
        pubkey: String,
        online: bool,
    },
    /// Pong response
    Pong,
    /// Errore
    Error {
        message: String,
    },
}

// ============== CONNECTION STATE ==============

#[cfg(feature = "server")]
#[derive(Debug)]
pub struct ConnectionState {
    pub pubkey: String,
    pub tx: mpsc::UnboundedSender<WsServerMessage>,
    pub connected_at: u64,
}

#[cfg(feature = "server")]
impl Clone for ConnectionState {
    fn clone(&self) -> Self {
        Self {
            pubkey: self.pubkey.clone(),
            tx: self.tx.clone(),
            connected_at: self.connected_at,
        }
    }
}

// ============== NODE IMPLEMENTATION ==============

impl Node {
    /// Trova un percorso verso la destinazione
    pub fn find_route(&self, destination: &str) -> Result<Vec<String>, String> {
        if self.pubkey == destination {
            return Ok(vec![]);
        }

        let mut queue: VecDeque<(&PeerNode, Vec<String>)> = VecDeque::new();
        let mut visited: HashSet<String> = HashSet::new();
        visited.insert(self.pubkey.clone());

        // Aggiungi i peer diretti
        for peer in &self.peers {
            if visited.contains(&peer.pubkey) {
                continue;
            }

            let path = vec![peer.pubkey.clone()];
            if peer.pubkey == destination {
                return Ok(path);
            }

            visited.insert(peer.pubkey.clone());
            queue.push_back((peer, path));
        }

        // BFS per trovare il percorso
        while let Some((current, path)) = queue.pop_front() {
            if let Some(peers) = &current.peers {
                for peer in peers {
                    if visited.contains(&peer.pubkey) {
                        continue;
                    }

                    let mut new_path = path.clone();
                    new_path.push(peer.pubkey.clone());

                    if peer.pubkey == destination {
                        return Ok(new_path);
                    }

                    visited.insert(peer.pubkey.clone());
                    queue.push_back((peer, new_path));
                }
            }
        }

        Err(format!("No route found to {}", destination))
    }
}

impl PeerNode {
    pub fn new(pubkey: String, address: String, port: u16, secure: bool) -> Self {
        Self {
            name: None,
            pubkey,
            address,
            public_port: port,
            secure,
            version: None,
            last_seen: None,
            is_connected: false,
            peers: None,
        }
    }
}

// ============== USER/CONTACT TYPES ==============

/// Contatto salvato dall'utente
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    pub pubkey: String,
    pub name: String,
    pub added_at: u64,
    pub last_message: Option<u64>,
}

/// Conversazione con un contatto
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Conversation {
    pub peer_pubkey: String,
    pub messages: Vec<ChatMessage>,
}

/// Singolo messaggio nella chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: String,
    pub from_me: bool,
    pub content: String,
    pub timestamp: u64,
    pub delivered: bool,
    pub read: bool,
}
