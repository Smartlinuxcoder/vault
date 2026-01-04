use crate::crypto::{
    current_timestamp, generate_x25519_keypair, random_bytes, sign_message, verify_signature_bytes,
};
use crate::types::{
    DiscoveryMessage, KnownPeer, Node, NodePacket, PeerNode, PeerProtocol, SignedNode,
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Gestisce la discovery e il mantenimento della rete peer
pub struct DiscoveryManager {
    /// Configurazione del nodo locale
    node_config: Node,
    /// Chiave privata Ed25519 per firme
    ed25519_privkey: Vec<u8>,
    /// Coppia di chiavi X25519 per onion routing
    x25519_keypair: ([u8; 32], [u8; 32]),
    /// Peer conosciuti (pubkey -> KnownPeer)
    known_peers: Arc<RwLock<HashMap<String, KnownPeer>>>,
    /// Peer attualmente connessi
    connected_peers: Arc<RwLock<HashMap<String, PeerConnection>>>,
}

/// Connessione attiva a un peer
#[derive(Debug)]
pub struct PeerConnection {
    pub address: SocketAddr,
    pub x25519_pubkey: Option<[u8; 32]>,
    pub connected_at: u64,
    pub last_activity: u64,
}

impl DiscoveryManager {
    /// Crea un nuovo DiscoveryManager
    pub fn new(node_config: Node, ed25519_privkey: Vec<u8>) -> Self {
        let x25519_keypair = generate_x25519_keypair();
        
        Self {
            node_config,
            ed25519_privkey,
            x25519_keypair,
            known_peers: Arc::new(RwLock::new(HashMap::new())),
            connected_peers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Ottiene la chiave pubblica X25519 del nodo
    pub fn x25519_pubkey(&self) -> [u8; 32] {
        self.x25519_keypair.1
    }

    /// Ottiene la chiave privata X25519 del nodo
    pub fn x25519_privkey(&self) -> [u8; 32] {
        self.x25519_keypair.0
    }

    /// Avvia il processo di discovery in background
    pub async fn start(&self) {
        info!("Starting peer discovery...");

        // Aggiungi i peer iniziali dalla configurazione
        self.bootstrap_peers().await;

        // Avvia il loop di ping periodico
        let known_peers = self.known_peers.clone();
        let connected_peers = self.connected_peers.clone();
        let node_config = self.node_config.clone();
        let ed25519_privkey = self.ed25519_privkey.clone();
        let x25519_pubkey = self.x25519_keypair.1;

        tokio::spawn(async move {
            let mut ping_interval = interval(Duration::from_secs(node_config.ping_interval));

            loop {
                ping_interval.tick().await;
                
                // Ping tutti i peer conosciuti
                let peers: Vec<_> = {
                    let peers = known_peers.read().await;
                    peers.values().cloned().collect()
                };

                for peer in peers {
                    // Use arson_port for TCP discovery, fallback to http_port
                    let port = if peer.node.arson_port > 0 { 
                        peer.node.arson_port 
                    } else { 
                        peer.node.http_port 
                    };
                    let addr = format!("{}:{}", peer.node.address, port);
                    match Self::ping_peer(&addr, &node_config, &ed25519_privkey, x25519_pubkey).await {
                        Ok(latency) => {
                            let mut peers = known_peers.write().await;
                            if let Some(p) = peers.get_mut(&peer.node.pubkey) {
                                p.last_ping = current_timestamp();
                                p.latency_ms = Some(latency);
                                p.failed_attempts = 0;
                                if p.trust_score < 255 {
                                    p.trust_score = p.trust_score.saturating_add(1);
                                }
                            }
                            debug!("Ping to {} successful, latency: {}ms", peer.node.pubkey, latency);
                        }
                        Err(e) => {
                            let mut peers = known_peers.write().await;
                            if let Some(p) = peers.get_mut(&peer.node.pubkey) {
                                p.failed_attempts += 1;
                                p.trust_score = p.trust_score.saturating_sub(5);
                                
                                // Rimuovi peer non raggiungibili dopo troppi tentativi
                                if p.failed_attempts > 10 {
                                    warn!("Removing unreachable peer: {}", peer.node.pubkey);
                                    peers.remove(&peer.node.pubkey);
                                    connected_peers.write().await.remove(&peer.node.pubkey);
                                }
                            }
                            debug!("Ping to {} failed: {}", peer.node.pubkey, e);
                        }
                    }
                }

                // Richiedi nuovi peer ai nodi connessi
                let connected: Vec<_> = {
                    let conns = connected_peers.read().await;
                    conns.keys().cloned().collect()
                };

                for pubkey in connected {
                    if let Some(peer) = known_peers.read().await.get(&pubkey) {
                        let port = if peer.node.arson_port > 0 { 
                            peer.node.arson_port 
                        } else { 
                            peer.node.http_port 
                        };
                        let addr = format!("{}:{}", peer.node.address, port);
                        if let Ok(new_peers) = Self::request_peers(&addr, 10).await {
                            let mut peers = known_peers.write().await;
                            for new_peer in new_peers {
                                if !peers.contains_key(&new_peer.node.pubkey) 
                                   && new_peer.node.pubkey != node_config.pubkey {
                                    info!("Discovered new peer: {} at {}", 
                                        new_peer.node.name.as_deref().unwrap_or("unknown"),
                                        new_peer.node.address
                                    );
                                    peers.insert(new_peer.node.pubkey.clone(), new_peer);
                                }
                            }
                        }
                    }
                }
            }
        });
    }

    /// Aggiunge i peer iniziali dalla configurazione
    async fn bootstrap_peers(&self) {
        let mut peers = self.known_peers.write().await;
        
        for peer_config in &self.node_config.peers {
            let peer_node = PeerNode::from_config(peer_config);
            let known_peer = KnownPeer {
                node: peer_node.clone(),
                x25519_pubkey: None,
                last_ping: 0,
                latency_ms: None,
                trust_score: 50, // Trust iniziale medio
                failed_attempts: 0,
            };
            peers.insert(peer_config.pubkey.clone(), known_peer);
            info!("Added bootstrap peer: {} at {}:{} ({})", 
                peer_config.name.as_deref().unwrap_or("unknown"),
                peer_config.address,
                peer_config.port,
                peer_config.protocol
            );
        }
    }

    /// Invia un ping a un peer e misura la latenza
    async fn ping_peer(
        addr: &str,
        node_config: &Node,
        ed25519_privkey: &[u8],
        x25519_pubkey: [u8; 32],
    ) -> Result<u32, Box<dyn std::error::Error + Send + Sync>> {
        let start = std::time::Instant::now();
        
        let mut stream = TcpStream::connect(addr).await?;
        
        let nonce: [u8; 8] = random_bytes();
        let timestamp = current_timestamp();
        
        let ping = DiscoveryMessage::Ping { timestamp, nonce };
        let packet = NodePacket::Discovery(ping);
        
        Self::send_packet(&mut stream, &packet).await?;
        
        // Aspetta risposta
        let response = Self::receive_packet(&mut stream).await?;
        
        match response {
            NodePacket::Discovery(DiscoveryMessage::Pong { nonce: resp_nonce, .. }) => {
                if resp_nonce == nonce {
                    let latency = start.elapsed().as_millis() as u32;
                    Ok(latency)
                } else {
                    Err("Nonce mismatch in pong".into())
                }
            }
            _ => Err("Unexpected response to ping".into()),
        }
    }

    /// Richiede una lista di peer a un nodo
    async fn request_peers(
        addr: &str,
        max_count: u16,
    ) -> Result<Vec<KnownPeer>, Box<dyn std::error::Error + Send + Sync>> {
        let mut stream = TcpStream::connect(addr).await?;
        
        let request = DiscoveryMessage::GetPeers { max_count };
        let packet = NodePacket::Discovery(request);
        
        Self::send_packet(&mut stream, &packet).await?;
        
        let response = Self::receive_packet(&mut stream).await?;
        
        match response {
            NodePacket::Discovery(DiscoveryMessage::PeerList { peers }) => Ok(peers),
            _ => Err("Unexpected response to peer request".into()),
        }
    }

    /// Invia un pacchetto serializzato con bincode
    async fn send_packet(
        stream: &mut TcpStream,
        packet: &NodePacket,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let data = bincode::serialize(packet)?;
        let len = data.len() as u32;
        
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&data).await?;
        stream.flush().await?;
        
        Ok(())
    }

    /// Riceve un pacchetto serializzato con bincode
    async fn receive_packet(
        stream: &mut TcpStream,
    ) -> Result<NodePacket, Box<dyn std::error::Error + Send + Sync>> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        
        if len > 10 * 1024 * 1024 {
            return Err("Packet too large".into());
        }
        
        let mut data = vec![0u8; len];
        stream.read_exact(&mut data).await?;
        
        let packet: NodePacket = bincode::deserialize(&data)?;
        Ok(packet)
    }

    /// Gestisce una richiesta di discovery in arrivo
    pub async fn handle_discovery(
        &self,
        msg: DiscoveryMessage,
    ) -> Option<DiscoveryMessage> {
        match msg {
            DiscoveryMessage::Ping { timestamp, nonce } => {
                Some(DiscoveryMessage::Pong {
                    timestamp: current_timestamp(),
                    nonce,
                    original_timestamp: timestamp,
                })
            }
            DiscoveryMessage::GetPeers { max_count } => {
                let peers = self.known_peers.read().await;
                let mut result: Vec<_> = peers.values()
                    .filter(|p| p.trust_score > 20 && p.failed_attempts < 3)
                    .take(max_count as usize)
                    .cloned()
                    .collect();
                
                // Ordina per trust score
                result.sort_by(|a, b| b.trust_score.cmp(&a.trust_score));
                
                Some(DiscoveryMessage::PeerList { peers: result })
            }
            DiscoveryMessage::Announce { node, x25519_pubkey } => {
                // Verifica la firma
                let node_data = bincode::serialize(&node.node).unwrap_or_default();
                let pubkey_bytes = hex::decode(&node.node.pubkey).unwrap_or_default();
                let sig_bytes = hex::decode(&node.signature).unwrap_or_default();
                
                if verify_signature_bytes(&pubkey_bytes, &node_data, &sig_bytes).is_ok() {
                    let mut peers = self.known_peers.write().await;
                    
                    let known_peer = KnownPeer {
                        node: node.node.clone(),
                        x25519_pubkey: Some(x25519_pubkey),
                        last_ping: current_timestamp(),
                        latency_ms: None,
                        trust_score: 30,
                        failed_attempts: 0,
                    };
                    
                    peers.insert(node.node.pubkey.clone(), known_peer);
                    info!("Peer announced: {}", node.node.pubkey);
                }
                None
            }
            DiscoveryMessage::Pong { .. } | DiscoveryMessage::PeerList { .. } => None,
        }
    }

    /// Ottiene i peer migliori per costruire un circuito onion
    pub async fn get_circuit_peers(&self, count: usize, exclude: &[String]) -> Vec<KnownPeer> {
        let peers = self.known_peers.read().await;
        
        let mut candidates: Vec<_> = peers.values()
            .filter(|p| {
                p.trust_score > 30 
                && p.x25519_pubkey.is_some() 
                && !exclude.contains(&p.node.pubkey)
                && p.failed_attempts < 3
            })
            .cloned()
            .collect();
        
        // Ordina per latenza e trust
        candidates.sort_by(|a, b| {
            let score_a = a.trust_score as i32 - a.latency_ms.unwrap_or(1000) as i32 / 10;
            let score_b = b.trust_score as i32 - b.latency_ms.unwrap_or(1000) as i32 / 10;
            score_b.cmp(&score_a)
        });
        
        candidates.into_iter().take(count).collect()
    }

    /// Ottiene tutti i peer conosciuti
    pub async fn get_known_peers(&self) -> Vec<KnownPeer> {
        self.known_peers.read().await.values().cloned().collect()
    }

    /// Crea un annuncio firmato del nodo locale
    pub fn create_announcement(&self) -> (SignedNode, [u8; 32]) {
        let peer_node = PeerNode {
            name: Some(self.node_config.name.clone()),
            pubkey: self.node_config.pubkey.clone(),
            address: self.node_config.address.clone(),
            http_port: self.node_config.public_http_port,
            arson_port: self.node_config.public_arson_port,
            secure: self.node_config.secure,
            protocols: vec![
                PeerProtocol::Arson,
                if self.node_config.secure { PeerProtocol::Wss } else { PeerProtocol::Ws }
            ],
            version: self.node_config.version.clone(),
            last_seen: Some(current_timestamp()),
            is_connected: true,
            public_port: self.node_config.public_http_port,
        };
        
        let node_data = bincode::serialize(&peer_node).unwrap_or_default();
        let signature = sign_message(&self.ed25519_privkey, &node_data);
        
        let signed_node = SignedNode {
            node: peer_node,
            signature: hex::encode(signature),
        };
        
        (signed_node, self.x25519_keypair.1)
    }
}
