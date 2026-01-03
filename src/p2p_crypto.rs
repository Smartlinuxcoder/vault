use base64::{engine::general_purpose, Engine as _};
use rsa::{
    RsaPrivateKey, RsaPublicKey,
    pkcs1v15::{Signature, SigningKey, VerifyingKey},
    pkcs8::{DecodePrivateKey, DecodePublicKey, EncodePrivateKey, EncodePublicKey},
    rand_core::OsRng,
    sha2::Sha256,
    signature::{SignatureEncoding, Signer, Verifier},
    traits::PublicKeyParts,
};

#[cfg(feature = "server")]
use aes_gcm::{
    aead::{Aead, AeadCore, KeyInit},
    Aes256Gcm, Nonce,
};

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

/// Cripta dati con la chiave pubblica del destinatario (ibrido RSA + AES-GCM)
#[cfg(feature = "server")]
pub fn encrypt_for_pubkey(
    pubkey_b64: &str,
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let pubkey_der = general_purpose::STANDARD.decode(pubkey_b64)?;
    let public_key = RsaPublicKey::from_public_key_der(&pubkey_der)?;
    
    // Genera chiave AES casuale
    let aes_key = Aes256Gcm::generate_key(&mut OsRng);
    let cipher = Aes256Gcm::new(&aes_key);
    let nonce = Aes256Gcm::generate_nonce(&mut OsRng);
    
    // Cripta i dati con AES
    let ciphertext = cipher.encrypt(&nonce, data)
        .map_err(|e| format!("AES encryption failed: {}", e))?;
    
    // Cripta la chiave AES con RSA
    let padding = rsa::Pkcs1v15Encrypt;
    let encrypted_key = public_key.encrypt(&mut OsRng, padding, aes_key.as_slice())?;
    
    // Combina: [encrypted_key (256 bytes)] [nonce (12 bytes)] [ciphertext]
    let mut combined = Vec::with_capacity(encrypted_key.len() + nonce.len() + ciphertext.len());
    combined.extend_from_slice(&encrypted_key);
    combined.extend_from_slice(nonce.as_slice());
    combined.extend_from_slice(&ciphertext);
    
    Ok(combined)
}

/// Decripta dati con la propria chiave privata
#[cfg(feature = "server")]
pub fn decrypt_with_privkey(
    privkey_pem: &str,
    data: &[u8],
) -> Result<Vec<u8>, Box<dyn std::error::Error + Send + Sync>> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(privkey_pem)?;
    
    let rsa_key_size = private_key.size();
    if data.len() < rsa_key_size + 12 {
        return Err("Invalid encrypted data: too short".into());
    }
    
    let (encrypted_key, rest) = data.split_at(rsa_key_size);
    let (nonce_bytes, ciphertext) = rest.split_at(12);
    
    // Decripta la chiave AES con RSA
    let padding = rsa::Pkcs1v15Encrypt;
    let symmetric_key = private_key.decrypt(padding, encrypted_key)?;
    
    // Decripta i dati con AES
    let nonce = Nonce::from_slice(nonce_bytes);
    let cipher = Aes256Gcm::new_from_slice(&symmetric_key)
        .map_err(|e| format!("Failed to create AES cipher: {}", e))?;
    let decrypted = cipher.decrypt(nonce, ciphertext)
        .map_err(|e| format!("AES decryption failed: {}", e))?;
    
    Ok(decrypted)
}

/// Deriva la chiave pubblica dalla chiave privata
pub fn derive_pubkey(privkey_pem: &str) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let private_key = RsaPrivateKey::from_pkcs8_pem(privkey_pem)?;
    let public_key = RsaPublicKey::from(&private_key);
    let pubkey_der = public_key.to_public_key_der()?;
    Ok(general_purpose::STANDARD.encode(pubkey_der.as_bytes()))
}

/// Genera un ID casuale
pub fn generate_id() -> String {
    let mut bytes = [0u8; 16];
    getrandom::getrandom(&mut bytes).expect("Failed to generate random bytes");
    bytes.iter().map(|b| format!("{:02x}", b)).collect()
}

/// Ottiene il timestamp corrente in secondi
pub fn current_timestamp() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Time went backwards")
        .as_secs()
}
