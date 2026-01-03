use serde::{Deserialize, Serialize};
// ============== SHARED TYPES ==============

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct VaultItem {
    pub id: String,
    pub encrypted_name: Vec<u8>,  // Encrypted filename (binary)
    pub name_nonce: Vec<u8>,      // Nonce for name encryption (binary)
    pub item_type: String,       // Encrypted item type
    pub size: usize,
    pub nonce: Vec<u8>,           // Nonce for file encryption (binary)
    pub content_id: String,      // Physical ID of the encrypted content file
    pub preview_id: Option<String>, // Physical ID of the encrypted preview file
}

/// Client-side decrypted item for display
#[derive(Clone, PartialEq, Debug)]
pub struct DecryptedItem {
    pub id: String,
    pub name: String,
    pub encrypted_name: Vec<u8>, // Added to persist original encrypted name
    pub item_type: String,
    pub size: usize,
    pub nonce: Vec<u8>,
    pub name_nonce: Vec<u8>,
    pub preview_url: Option<String>,
    pub content_id: String,
    pub preview_id: Option<String>,
}

#[derive(Clone, Serialize, Deserialize, PartialEq, Debug)]
pub struct VaultMetadata {
    pub name: String,
    pub items: Vec<VaultItem>,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct StartUploadRequest {
    pub session_token: String,
    pub encrypted_name: Vec<u8>,
    pub name_nonce: Vec<u8>,
    pub item_type: String,
    pub nonce: Vec<u8>,
    pub total_chunks: usize,
    pub preview: Option<Vec<u8>>,      // Binary encrypted preview
    pub preview_nonce: Option<Vec<u8>>, // Nonce for preview encryption
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

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct SaveMetadataRequest {
    pub session_token: String,
    pub encrypted_metadata: Vec<u8>, // Binary encoded encrypted JSON
}
