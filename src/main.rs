use dioxus::prelude::*;

mod vault;
use vault::Vault;
mod styles;
mod types;
use types::{
    FinishUploadRequest, SaveMetadataRequest, StartUploadRequest, StartUploadResponse,
    UploadResult, VaultItem,
};

// P2P Modules
mod p2p_chat;
mod p2p_crypto;
#[cfg(feature = "server")]
mod p2p_server;
mod p2p_types;
use p2p_chat::P2PChat;

// Identity & Network
mod identity;
use identity::IdentitySetup;
mod network;
use network::NetworkView;

fn main() {
    #[cfg(feature = "server")]
    dioxus::serve(|| async move {
        use dioxus::server::axum::{
            extract::DefaultBodyLimit,
            routing::{any, get, post},
        };

        // Inizializza lo stato P2P
        let p2p_state = p2p_server::P2PState::load_or_create("config/node.json")
            .await
            .expect("Failed to initialize P2P state");

        println!("🌐 P2P Node initialized");
        println!(
            "   Public Key: {}...",
            &p2p_state
                .node
                .read()
                .await
                .pubkey
                .chars()
                .take(20)
                .collect::<String>()
        );

        // Salva lo stato globalmente per le server functions
        p2p_server::init_global_state(p2p_state.clone());

        let router = dioxus::server::router(App)
            // Vault API routes
            .route("/api/start_upload", post(start_upload_handler))
            .route("/api/upload_chunk", post(upload_chunk_handler))
            .route("/api/finish_upload", post(finish_upload_handler))
            .route("/api/get_file/{env}/{file_id}", get(get_file_handler))
            .route("/api/get_preview/{env}/{file_id}", get(get_preview_handler))
            // P2P routes
            .route("/p2p/ws", any(p2p_server::ws_handler_global))
            .route("/p2p/info", get(p2p_server::node_info_handler_global))
            .route("/p2p/peers", get(p2p_server::peers_handler_global))
            .layer(DefaultBodyLimit::max(2 * 1024 * 1024));

        Ok(router)
    });

    #[cfg(not(feature = "server"))]
    dioxus::launch(App);
}

#[derive(Routable, Clone, PartialEq)]
pub enum Route {
    #[route("/")]
    Identity {},
    #[route("/network")]
    Network {},
    #[route("/vault")]
    Vault {},
    #[route("/chat")]
    Chat {},
}

#[component]
fn App() -> Element {
    static CSS: Asset = asset!("/assets/tailwind.css");
    rsx! {
    document::Stylesheet { href: CSS }

    Router::<Route> {} }
}

#[component]
fn Identity() -> Element {
    rsx! { IdentitySetup {} }
}

#[component]
fn Network() -> Element {
    rsx! { NetworkView {} }
}

#[component]
fn Chat() -> Element {
    let navigator = use_navigator();

    rsx! {

        // Header
        header { class: "vault-header glass-bar",
            div { class: "header-left",
                span { class: "vault-logo", "💬" }
                div { class: "vault-info",
                    h1 { "PeerWave Chat" }
                    p { "End-to-end encrypted messaging" }
                }
            }
            div { class: "header-actions",
                button {
                    class: "btn-chat",
                    onclick: move |_| { navigator.push(Route::Network {}); },
                    "🌐 Network"
                }
                button {
                    class: "btn-primary",
                    onclick: move |_| { navigator.push(Route::Vault {}); },
                    "📁 Vault"
                }
                button {
                    class: "btn-logout",
                    onclick: move |_| { navigator.push(Route::Identity {}); },
                    "🔒"
                }
            }
        }

        P2PChat {}
    }
}

// ============== AXUM HANDLERS ==============

#[cfg(feature = "server")]
async fn start_upload_handler(
    dioxus::server::axum::extract::Json(req): dioxus::server::axum::extract::Json<
        StartUploadRequest,
    >,
) -> Result<
    dioxus::server::axum::response::Json<StartUploadResponse>,
    dioxus::server::axum::http::StatusCode,
> {
    use dioxus::server::axum::http::StatusCode;
    use rand::Rng;
    use std::path::Path;

    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);

    if !Path::new(&vault_dir).exists() {
        std::fs::create_dir_all(&vault_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    // Generate random IDs for content and preview
    let mut rng = rand::thread_rng();
    let file_id: String = (0..16)
        .map(|_| rng.gen_range(0..16))
        .map(|x| format!("{:x}", x))
        .collect();
    let content_id: String = (0..32)
        .map(|_| rng.gen_range(0..16))
        .map(|x| format!("{:x}", x))
        .collect();

    let temp_dir = format!("{}/{}_chunks", vault_dir, file_id);
    std::fs::create_dir_all(&temp_dir).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut preview_id = None;

    // Save preview if provided
    if let (Some(preview), Some(preview_nonce)) = (&req.preview, &req.preview_nonce) {
        let pid: String = (0..32)
            .map(|_| rng.gen_range(0..16))
            .map(|x| format!("{:x}", x))
            .collect();
        let preview_path = format!("{}/{}", vault_dir, pid);

        // Store as binary: [nonce (12 bytes)] [data]
        let mut file_data = Vec::with_capacity(preview_nonce.len() + preview.len());
        file_data.extend_from_slice(preview_nonce);
        file_data.extend_from_slice(preview);

        std::fs::write(&preview_path, file_data).ok();
        preview_id = Some(pid);
    }

    let meta = serde_json::json!({
        "encrypted_name": req.encrypted_name,
        "name_nonce": req.name_nonce,
        "item_type": req.item_type,
        "nonce": req.nonce,
        "total_chunks": req.total_chunks,
        "content_id": content_id,
        "preview_id": preview_id
    });
    std::fs::write(format!("{}/meta.json", temp_dir), meta.to_string())
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(dioxus::server::axum::response::Json(StartUploadResponse {
        file_id,
    }))
}

#[cfg(feature = "server")]
async fn upload_chunk_handler(
    dioxus::server::axum::extract::Query(params): dioxus::server::axum::extract::Query<
        std::collections::HashMap<String, String>,
    >,
    body: dioxus::server::axum::body::Bytes,
) -> Result<
    dioxus::server::axum::response::Json<serde_json::Value>,
    dioxus::server::axum::http::StatusCode,
> {
    use dioxus::server::axum::http::StatusCode;

    let session_token = params.get("token").ok_or(StatusCode::BAD_REQUEST)?;
    let file_id = params.get("file_id").ok_or(StatusCode::BAD_REQUEST)?;
    let chunk_index: usize = params
        .get("chunk")
        .ok_or(StatusCode::BAD_REQUEST)?
        .parse()
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let environment = session_token.split('_').next().unwrap_or("personal");
    let chunk_path = format!(
        "vault_data/{}/{}_chunks/{}.chunk",
        environment, file_id, chunk_index
    );

    std::fs::write(&chunk_path, &body).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    Ok(dioxus::server::axum::response::Json(
        serde_json::json!({ "success": true }),
    ))
}

#[cfg(feature = "server")]
async fn finish_upload_handler(
    dioxus::server::axum::extract::Json(req): dioxus::server::axum::extract::Json<
        FinishUploadRequest,
    >,
) -> Result<
    dioxus::server::axum::response::Json<UploadResult>,
    dioxus::server::axum::http::StatusCode,
> {
    use dioxus::server::axum::http::StatusCode;
    use std::io::Write;
    use std::path::Path;

    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);
    let temp_dir = format!("{}/{}_chunks", vault_dir, req.file_id);

    let meta_str = std::fs::read_to_string(format!("{}/meta.json", temp_dir))
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let meta: serde_json::Value =
        serde_json::from_str(&meta_str).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let total_chunks = meta["total_chunks"].as_u64().unwrap_or(0) as usize;
    let content_id = meta["content_id"].as_str().unwrap_or("").to_string();

    let file_path = format!("{}/{}", vault_dir, content_id);
    let mut output =
        std::fs::File::create(&file_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut total_size = 0usize;
    for i in 0..total_chunks {
        let chunk_path = format!("{}/{}.chunk", temp_dir, i);
        let chunk_data =
            std::fs::read(&chunk_path).map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
        total_size += chunk_data.len();
        output
            .write_all(&chunk_data)
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    }

    std::fs::remove_dir_all(&temp_dir).ok();

    // We do NOT update metadata here anymore. The client must do it.

    let item = VaultItem {
        id: req.file_id, // Keep the upload ID as the logical ID
        encrypted_name: serde_json::from_value(meta["encrypted_name"].clone()).unwrap_or_default(),
        name_nonce: serde_json::from_value(meta["name_nonce"].clone()).unwrap_or_default(),
        item_type: meta["item_type"].as_str().unwrap_or("document").to_string(),
        size: total_size,
        nonce: serde_json::from_value(meta["nonce"].clone()).unwrap_or_default(),
        content_id: content_id,
        preview_id: meta["preview_id"].as_str().map(|s| s.to_string()),
    };

    Ok(dioxus::server::axum::response::Json(UploadResult {
        success: true,
        item: Some(item),
    }))
}

#[cfg(feature = "server")]
async fn get_file_handler(
    dioxus::server::axum::extract::Path((env, file_id)): dioxus::server::axum::extract::Path<(
        String,
        String,
    )>,
) -> Result<dioxus::server::axum::body::Bytes, dioxus::server::axum::http::StatusCode> {
    use dioxus::server::axum::http::StatusCode;

    // Files are now just flat files in the directory with no extension
    let file_path = format!("vault_data/{}/{}", env, file_id);
    let data = std::fs::read(&file_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(dioxus::server::axum::body::Bytes::from(data))
}

#[cfg(feature = "server")]
async fn get_preview_handler(
    dioxus::server::axum::extract::Path((env, file_id)): dioxus::server::axum::extract::Path<(
        String,
        String,
    )>,
) -> Result<dioxus::server::axum::body::Bytes, dioxus::server::axum::http::StatusCode> {
    use dioxus::server::axum::http::StatusCode;

    // Previews are also flat files
    let preview_path = format!("vault_data/{}/{}", env, file_id);
    let data = std::fs::read(&preview_path).map_err(|_| StatusCode::NOT_FOUND)?;
    Ok(dioxus::server::axum::body::Bytes::from(data))
}

// ============== SERVER FUNCTIONS ==============

#[server]
pub async fn get_vault_metadata(session_token: String) -> Result<Vec<u8>, ServerFnError> {
    use std::path::Path;

    let environment = session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);
    let metadata_path = format!("{}/metadata.enc", vault_dir);

    if !Path::new(&vault_dir).exists() {
        std::fs::create_dir_all(&vault_dir).ok();
    }

    if Path::new(&metadata_path).exists() {
        let data = std::fs::read(&metadata_path).map_err(|e| ServerFnError::new(e.to_string()))?;
        Ok(data)
    } else {
        Ok(Vec::new()) // Empty vector means no metadata yet
    }
}

#[server]
pub async fn save_vault_metadata(req: SaveMetadataRequest) -> Result<bool, ServerFnError> {
    use std::path::Path;

    let environment = req.session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);
    let metadata_path = format!("{}/metadata.enc", vault_dir);

    if !Path::new(&vault_dir).exists() {
        std::fs::create_dir_all(&vault_dir).ok();
    }

    std::fs::write(&metadata_path, req.encrypted_metadata)
        .map_err(|e| ServerFnError::new(e.to_string()))?;

    Ok(true)
}

#[server]
pub async fn delete_files(
    session_token: String,
    file_ids: Vec<String>,
) -> Result<bool, ServerFnError> {
    use std::path::Path;

    let environment = session_token.split('_').next().unwrap_or("personal");
    let vault_dir = format!("vault_data/{}", environment);

    for id in file_ids {
        let path = format!("{}/{}", vault_dir, id);
        if Path::new(&path).exists() {
            std::fs::remove_file(&path).ok();
        }
    }

    Ok(true)
}
