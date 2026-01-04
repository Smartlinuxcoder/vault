use base64::{engine::general_purpose, Engine as _};
use rsa::{
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
    rand_core::OsRng,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer, Verifier},
    RsaPrivateKey, RsaPublicKey,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Genera una nuova coppia di chiavi RSA
pub fn generate_keypair() -> Result<(String, String), Box<dyn std::error::Error + Send + Sync>> {
    let private_key = RsaPrivateKey::new(&mut OsRng, 2048)?;
    let public_key = RsaPublicKey::from(&private_key);

    let privkey_pem = private_key.to_pkcs8_pem(rsa::pkcs8::LineEnding::LF)?;
    let pubkey_der = public_key.to_public_key_der()?;
    let pubkey_b64 = general_purpose::STANDARD.encode(pubkey_der.as_bytes());

    Ok((pubkey_b64, privkey_pem.to_string()))
}

/// Firma dati con la chiave privata
pub fn sign_data(privkey_pem: &str, data: &[u8]) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(privkey_pem)?;
    let signing_key = SigningKey::<Sha256>::new(private_key);
    let signature = signing_key.sign(data);
    Ok(general_purpose::STANDARD.encode(signature.to_vec()))
}

/// Verifica una firma con la chiave pubblica
pub fn verify_signature(
    pubkey_b64: &str,
    signature_b64: &str,
    data: &[u8],
) -> Result<bool, Box<dyn std::error::Error + Send + Sync>> {
    let pubkey_der = general_purpose::STANDARD.decode(pubkey_b64)?;
    let public_key = RsaPublicKey::from_public_key_der(&pubkey_der)?;

    let signature_bytes = general_purpose::STANDARD.decode(signature_b64)?;
    let signature = Signature::try_from(signature_bytes.as_slice())?;

    let verifying_key = VerifyingKey::<Sha256>::new(public_key);

    match verifying_key.verify(data, &signature) {
        Ok(_) => Ok(true),
        Err(_) => Ok(false),
    }
}

/// Deriva la chiave pubblica dalla chiave privata
pub fn derive_pubkey(privkey_pem: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(privkey_pem)?;
    let public_key = RsaPublicKey::from(&private_key);
    let pubkey_der = public_key.to_public_key_der()?;
    Ok(general_purpose::STANDARD.encode(pubkey_der.as_bytes()))
}

/// Ottiene il timestamp corrente in secondi
pub fn current_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}

// ============== X25519 & AES-GCM per Onion Routing ==============

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Nonce,
};
use x25519_dalek::{EphemeralSecret, PublicKey, StaticSecret};

/// Genera una coppia di chiavi X25519 per onion routing
pub fn generate_x25519_keypair() -> ([u8; 32], [u8; 32]) {
    let secret = StaticSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret.to_bytes(), public.to_bytes())
}

/// Deriva una chiave condivisa usando X25519 ECDH
pub fn x25519_derive_shared(our_secret: &[u8; 32], their_public: &[u8; 32]) -> [u8; 32] {
    let secret = StaticSecret::from(*our_secret);
    let public = PublicKey::from(*their_public);
    let shared = secret.diffie_hellman(&public);
    *shared.as_bytes()
}

/// Genera un segreto effimero X25519 e la chiave pubblica corrispondente
pub fn x25519_ephemeral() -> (x25519_dalek::EphemeralSecret, [u8; 32]) {
    let secret = EphemeralSecret::random_from_rng(OsRng);
    let public = PublicKey::from(&secret);
    (secret, public.to_bytes())
}

/// Esegue ECDH con un segreto effimero
pub fn x25519_ephemeral_derive(ephemeral: x25519_dalek::EphemeralSecret, their_public: &[u8; 32]) -> [u8; 32] {
    let public = PublicKey::from(*their_public);
    let shared = ephemeral.diffie_hellman(&public);
    *shared.as_bytes()
}

/// Deriva una chiave AES-256 dalla shared secret usando SHA256
pub fn derive_aes_key(shared_secret: &[u8; 32]) -> [u8; 32] {
    use sha2::{Sha256, Digest};
    let mut hasher = Sha256::new();
    hasher.update(b"onion-aes-key-v1");
    hasher.update(shared_secret);
    let result = hasher.finalize();
    let mut key = [0u8; 32];
    key.copy_from_slice(&result);
    key
}

/// Cripta dati con AES-256-GCM
pub fn aes_encrypt(key: &[u8; 32], nonce: &[u8; 12], plaintext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let nonce = Nonce::from_slice(nonce);
    cipher.encrypt(nonce, plaintext)
        .map_err(|e| format!("AES encryption failed: {}", e).into())
}

/// Decripta dati con AES-256-GCM
pub fn aes_decrypt(key: &[u8; 32], nonce: &[u8; 12], ciphertext: &[u8]) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let cipher = Aes256Gcm::new_from_slice(key)?;
    let nonce = Nonce::from_slice(nonce);
    cipher.decrypt(nonce, ciphertext)
        .map_err(|e| format!("AES decryption failed: {}", e).into())
}

/// Genera un nonce casuale per AES-GCM
pub fn generate_nonce() -> [u8; 12] {
    use rand::RngCore;
    let mut nonce = [0u8; 12];
    OsRng.fill_bytes(&mut nonce);
    nonce
}

/// Genera un ID pacchetto casuale
pub fn generate_packet_id() -> [u8; 16] {
    use rand::RngCore;
    let mut id = [0u8; 16];
    OsRng.fill_bytes(&mut id);
    id
}

/// Genera byte casuali
pub fn random_bytes<const N: usize>() -> [u8; N] {
    use rand::RngCore;
    let mut bytes = [0u8; N];
    OsRng.fill_bytes(&mut bytes);
    bytes
}

// ============== Ed25519 per Discovery ==============

use ed25519_dalek::{
    SigningKey as Ed25519SigningKey, 
    VerifyingKey as Ed25519VerifyingKey, 
    Signer as Ed25519Signer, 
    Verifier as Ed25519Verifier
};

/// Genera una coppia di chiavi Ed25519
pub fn generate_ed25519_keypair() -> ([u8; 32], [u8; 32]) {
    let signing_key = Ed25519SigningKey::generate(&mut OsRng);
    let verifying_key = signing_key.verifying_key();
    (signing_key.to_bytes(), verifying_key.to_bytes())
}

/// Firma un messaggio con la chiave privata Ed25519
pub fn sign_message(privkey: &[u8], message: &[u8]) -> Vec<u8> {
    let privkey_bytes: [u8; 32] = privkey.try_into().expect("Invalid Ed25519 private key length");
    let signing_key = Ed25519SigningKey::from_bytes(&privkey_bytes);
    let signature = signing_key.sign(message);
    signature.to_bytes().to_vec()
}

/// Verifica una firma Ed25519 con bytes raw
pub fn verify_signature_bytes(
    pubkey: &[u8],
    message: &[u8],
    signature: &[u8],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let pubkey_bytes: [u8; 32] = pubkey.try_into()
        .map_err(|_| "Invalid Ed25519 public key length")?;
    let sig_bytes: [u8; 64] = signature.try_into()
        .map_err(|_| "Invalid Ed25519 signature length")?;
    
    let verifying_key = Ed25519VerifyingKey::from_bytes(&pubkey_bytes)
        .map_err(|e| format!("Invalid Ed25519 public key: {}", e))?;
    let sig = ed25519_dalek::Signature::from_bytes(&sig_bytes);
    
    verifying_key.verify(message, &sig)
        .map_err(|e| format!("Signature verification failed: {}", e).into())
}
