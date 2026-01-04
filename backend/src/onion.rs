// filepath: /home/smartcoder/Documenti/code/vault/backend/src/onion.rs
use crate::crypto::{
    aes_decrypt, aes_encrypt, derive_aes_key, generate_nonce, generate_packet_id,
    x25519_derive_shared, x25519_ephemeral, x25519_ephemeral_derive,
};
use crate::types::{KnownPeer, NextHop, OnionLayer, OnionPacket, OnionResponse, RoutedMessage};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tracing::{debug, error, info, warn};

/// Gestisce la creazione e il processamento di pacchetti onion
pub struct OnionRouter {
    /// Chiave privata X25519 del nodo
    x25519_privkey: [u8; 32],
    /// Chiave pubblica X25519 del nodo
    x25519_pubkey: [u8; 32],
    /// Cache di packet ID già visti (per prevenire replay attacks)
    seen_packets: Arc<RwLock<HashMap<[u8; 16], u64>>>,
    /// Circuiti attivi (packet_id -> return path info)
    active_circuits: Arc<RwLock<HashMap<[u8; 16], CircuitInfo>>>,
}

/// Informazioni su un circuito attivo per le risposte
#[derive(Debug, Clone)]
pub struct CircuitInfo {
    pub created_at: u64,
    pub return_key: [u8; 32],
    pub return_nonce: [u8; 12],
    pub prev_hop: Option<SocketAddr>,
}

impl OnionRouter {
    /// Crea un nuovo OnionRouter
    pub fn new(x25519_privkey: [u8; 32], x25519_pubkey: [u8; 32]) -> Self {
        Self {
            x25519_privkey,
            x25519_pubkey,
            seen_packets: Arc::new(RwLock::new(HashMap::new())),
            active_circuits: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Ottiene la chiave pubblica X25519
    pub fn pubkey(&self) -> [u8; 32] {
        self.x25519_pubkey
    }

    /// Crea un pacchetto onion multi-layer per una lista di hop
    /// L'ultimo hop è la destinazione finale
    pub fn create_onion_packet(
        &self,
        hops: &[KnownPeer],
        final_payload: &[u8],
    ) -> Result<OnionPacket, Box<dyn std::error::Error + Send + Sync>> {
        if hops.is_empty() {
            return Err("At least one hop is required".into());
        }

        // Costruiamo l'onion dall'interno verso l'esterno
        // Iniziamo con il payload finale
        let mut current_payload = final_payload.to_vec();

        // Procediamo al contrario attraverso gli hop
        for (i, hop) in hops.iter().enumerate().rev() {
            let x25519_pubkey = hop.x25519_pubkey
                .ok_or_else(|| format!("Hop {} missing X25519 public key", i))?;

            // Determina il next_hop (None per l'ultimo hop)
            let next_hop = if i < hops.len() - 1 {
                let next = &hops[i + 1];
                // Use arson_port for onion routing (TCP)
                let port = if next.node.arson_port > 0 {
                    next.node.arson_port
                } else {
                    next.node.http_port
                };
                Some(NextHop {
                    address: next.node.address.clone(),
                    port,
                    pubkey: next.node.pubkey.clone(),
                })
            } else {
                None
            };

            // Crea il layer
            let layer = OnionLayer {
                next_hop,
                inner_packet: current_payload,
            };

            let layer_bytes = bincode::serialize(&layer)?;

            // Genera chiave effimera per questo layer
            let (ephemeral_secret, ephemeral_pubkey) = x25519_ephemeral();

            // Deriva shared secret
            let shared_secret = x25519_ephemeral_derive(ephemeral_secret, &x25519_pubkey);
            let aes_key = derive_aes_key(&shared_secret);
            let nonce = generate_nonce();

            // Cripta il layer
            let encrypted = aes_encrypt(&aes_key, &nonce, &layer_bytes)?;

            // Crea il pacchetto per questo layer
            let packet = OnionPacket {
                packet_id: generate_packet_id(),
                ephemeral_pubkey,
                encrypted_payload: encrypted,
                nonce,
            };

            // Serializza per il prossimo layer
            current_payload = bincode::serialize(&packet)?;
        }

        // Deserializza il pacchetto finale (il più esterno)
        let final_packet: OnionPacket = bincode::deserialize(&current_payload)?;
        Ok(final_packet)
    }

    /// Processa un pacchetto onion in arrivo
    /// Ritorna: (payload decriptato, opzionale next hop)
    pub async fn unwrap_layer(
        &self,
        packet: &OnionPacket,
        from_addr: Option<SocketAddr>,
    ) -> Result<(Vec<u8>, Option<NextHop>), Box<dyn std::error::Error + Send + Sync>> {
        // Controlla replay attack
        {
            let seen = self.seen_packets.read().await;
            if seen.contains_key(&packet.packet_id) {
                return Err("Replay attack detected: packet already seen".into());
            }
        }

        // Registra packet ID
        {
            let mut seen = self.seen_packets.write().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            seen.insert(packet.packet_id, now);

            // Pulisci vecchi packet ID (più vecchi di 1 ora)
            seen.retain(|_, &mut ts| now - ts < 3600);
        }

        // Deriva shared secret usando la nostra chiave privata
        let shared_secret = x25519_derive_shared(&self.x25519_privkey, &packet.ephemeral_pubkey);
        let aes_key = derive_aes_key(&shared_secret);

        // Decripta il payload
        let decrypted = aes_decrypt(&aes_key, &packet.nonce, &packet.encrypted_payload)?;

        // Deserializza il layer
        let layer: OnionLayer = bincode::deserialize(&decrypted)?;

        // Salva info circuito per risposta
        {
            let mut circuits = self.active_circuits.write().await;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs();
            
            circuits.insert(packet.packet_id, CircuitInfo {
                created_at: now,
                return_key: aes_key,
                return_nonce: packet.nonce,
                prev_hop: from_addr,
            });

            // Pulisci vecchi circuiti (più vecchi di 10 minuti)
            circuits.retain(|_, info| now - info.created_at < 600);
        }

        Ok((layer.inner_packet, layer.next_hop))
    }

    /// Crea una risposta onion criptata
    pub async fn create_response(
        &self,
        packet_id: &[u8; 16],
        response_data: &[u8],
    ) -> Result<OnionResponse, Box<dyn std::error::Error + Send + Sync>> {
        let circuits = self.active_circuits.read().await;
        let circuit = circuits.get(packet_id)
            .ok_or("Circuit not found for response")?;

        let nonce = generate_nonce();
        let encrypted = aes_encrypt(&circuit.return_key, &nonce, response_data)?;

        Ok(OnionResponse {
            packet_id: *packet_id,
            encrypted_response: encrypted,
            nonce,
        })
    }

    /// Decripta una risposta onion ricevuta
    pub fn decrypt_response(
        &self,
        response: &OnionResponse,
        shared_secrets: &[[u8; 32]], // Chiavi condivise usate per creare il pacchetto originale
    ) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
        let mut data = response.encrypted_response.clone();

        // Decripta attraverso tutti i layer in ordine inverso
        for shared_secret in shared_secrets.iter().rev() {
            let aes_key = derive_aes_key(shared_secret);
            data = aes_decrypt(&aes_key, &response.nonce, &data)?;
        }

        Ok(data)
    }

    /// Inoltra un pacchetto onion al prossimo hop
    pub async fn forward_packet(
        &self,
        next_hop: &NextHop,
        inner_packet: &[u8],
    ) -> Result<Option<OnionResponse>, Box<dyn std::error::Error + Send + Sync>> {
        let addr = format!("{}:{}", next_hop.address, next_hop.port);
        let mut stream = TcpStream::connect(&addr).await?;

        // Invia il pacchetto
        let len = inner_packet.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(inner_packet).await?;
        stream.flush().await?;

        // Aspetta risposta (con timeout)
        let mut len_buf = [0u8; 4];
        match tokio::time::timeout(
            std::time::Duration::from_secs(30),
            stream.read_exact(&mut len_buf),
        )
        .await
        {
            Ok(Ok(_)) => {
                let len = u32::from_be_bytes(len_buf) as usize;
                if len > 10 * 1024 * 1024 {
                    return Err("Response too large".into());
                }
                let mut data = vec![0u8; len];
                stream.read_exact(&mut data).await?;
                let response: OnionResponse = bincode::deserialize(&data)?;
                Ok(Some(response))
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => Ok(None), // Timeout, nessuna risposta
        }
    }

    /// Invia un messaggio attraverso un circuito onion
    pub async fn send_through_circuit(
        &self,
        hops: &[KnownPeer],
        message: RoutedMessage,
    ) -> Result<Option<Vec<u8>>, Box<dyn std::error::Error + Send + Sync>> {
        if hops.is_empty() {
            return Err("No hops provided".into());
        }

        let payload = bincode::serialize(&message)?;
        let packet = self.create_onion_packet(hops, &payload)?;

        // Invia al primo hop - use arson_port for TCP
        let first_hop = &hops[0];
        let port = if first_hop.node.arson_port > 0 {
            first_hop.node.arson_port
        } else {
            first_hop.node.http_port
        };
        let addr = format!("{}:{}", first_hop.node.address, port);

        debug!("Sending onion packet to first hop: {}", addr);

        let mut stream = TcpStream::connect(&addr).await?;
        let packet_bytes = bincode::serialize(&packet)?;

        let len = packet_bytes.len() as u32;
        stream.write_all(&len.to_be_bytes()).await?;
        stream.write_all(&packet_bytes).await?;
        stream.flush().await?;

        // Aspetta risposta
        let mut len_buf = [0u8; 4];
        match tokio::time::timeout(
            std::time::Duration::from_secs(60),
            stream.read_exact(&mut len_buf),
        )
        .await
        {
            Ok(Ok(_)) => {
                let len = u32::from_be_bytes(len_buf) as usize;
                if len > 10 * 1024 * 1024 {
                    return Err("Response too large".into());
                }
                let mut data = vec![0u8; len];
                stream.read_exact(&mut data).await?;
                Ok(Some(data))
            }
            Ok(Err(e)) => Err(e.into()),
            Err(_) => {
                debug!("Timeout waiting for onion response");
                Ok(None)
            }
        }
    }
}

/// Gestisce connessioni TCP in arrivo per onion routing
pub async fn handle_onion_connection(
    mut stream: TcpStream,
    router: Arc<OnionRouter>,
    peer_addr: SocketAddr,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
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

    let packet: OnionPacket = bincode::deserialize(&data)?;
    let packet_id = packet.packet_id;

    // Processa il layer
    match router.unwrap_layer(&packet, Some(peer_addr)).await {
        Ok((inner_data, next_hop)) => {
            if let Some(next) = next_hop {
                // Siamo un relay, inoltra al prossimo hop
                debug!("Forwarding onion packet to {}:{}", next.address, next.port);

                match router.forward_packet(&next, &inner_data).await {
                    Ok(Some(response)) => {
                        // Inoltra la risposta indietro
                        let response_bytes = bincode::serialize(&response)?;
                        let len = response_bytes.len() as u32;
                        stream.write_all(&len.to_be_bytes()).await?;
                        stream.write_all(&response_bytes).await?;
                    }
                    Ok(None) => {
                        debug!("No response from next hop");
                    }
                    Err(e) => {
                        warn!("Failed to forward packet: {}", e);
                    }
                }
            } else {
                // Siamo la destinazione finale
                debug!("Received onion packet as final destination");

                // Prova a deserializzare come RoutedMessage
                match bincode::deserialize::<RoutedMessage>(&inner_data) {
                    Ok(message) => {
                        info!(
                            "Received routed message type: {:?}",
                            message.message_type
                        );

                        // TODO: Processa il messaggio e genera risposta
                        // Per ora inviamo un ACK
                        let ack = b"ACK";
                        if let Ok(response) = router.create_response(&packet_id, ack).await {
                            let response_bytes = bincode::serialize(&response)?;
                            let len = response_bytes.len() as u32;
                            stream.write_all(&len.to_be_bytes()).await?;
                            stream.write_all(&response_bytes).await?;
                        }
                    }
                    Err(e) => {
                        warn!("Failed to deserialize routed message: {}", e);
                    }
                }
            }
        }
        Err(e) => {
            error!("Failed to unwrap onion layer: {}", e);
        }
    }

    stream.flush().await?;
    Ok(())
}
