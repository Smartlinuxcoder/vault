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
