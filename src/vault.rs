use dioxus::prelude::*;
use crate::Route;
use crate::{get_vault_metadata, save_vault_metadata, delete_files};

use super::types::{
    VaultItem, VaultMetadata, DecryptedItem, SaveMetadataRequest,
};

// CSS loaded from external file
static VAULT_CSS: Asset = asset!("/assets/css/vault.css");

#[component]
pub fn Vault() -> Element {
    let navigator = use_navigator();
    let mut vault_items = use_signal(|| Vec::<DecryptedItem>::new());
    let mut vault_name = use_signal(|| String::new());
    let mut loading = use_signal(|| true);
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut uploading = use_signal(|| false);
    let mut upload_progress = use_signal(|| String::new());
    let mut viewing_item = use_signal(|| Option::<DecryptedItem>::None);
    let mut view_content = use_signal(|| Option::<String>::None);
    let mut view_loading = use_signal(|| false);
    let mut view_mode = use_signal(|| "grid".to_string()); // grid or list
    
    // State for lazy loading
    let mut session_token_sig = use_signal(|| String::new());
    let mut user_pin_sig = use_signal(|| String::new());
    let mut loading_previews = use_signal(|| std::collections::HashSet::<String>::new());

    // Load vault and decrypt names on mount
    use_effect(move || {
        spawn(async move {
            // Get identity from sessionStorage (set by Identity page)
            let mut eval = document::eval(r#"
                (async function() {
                    const pubkey = sessionStorage.getItem('p2p_pubkey');
                    const pin = sessionStorage.getItem('user_pin');
                    const name = sessionStorage.getItem('p2p_name');
                    
                    if (!pubkey || !pin) {
                        dioxus.send(JSON.stringify({ error: 'not_authenticated' }));
                        return;
                    }
                    
                    // Create a vault ID from pubkey hash (first 16 chars of SHA-256)
                    const encoder = new TextEncoder();
                    const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkey));
                    const hashArray = Array.from(new Uint8Array(hashBuffer));
                    const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
                    
                    dioxus.send(JSON.stringify({
                        session_token: vaultId + '_' + Date.now(),
                        pin: pin,
                        name: name || 'My Vault',
                        vault_id: vaultId
                    }));
                })()
            "#);
            
            let (session_token, user_pin, display_name): (String, String, String) = match eval.recv::<String>().await {
                Ok(val) => {
                    match serde_json::from_str::<serde_json::Value>(&val) {
                        Ok(v) => {
                            if v.get("error").is_some() {
                                navigator.push(Route::Identity {});
                                return;
                            }
                            let token = v["session_token"].as_str().unwrap_or("").to_string();
                            let pin = v["pin"].as_str().unwrap_or("").to_string();
                            let name = v["name"].as_str().unwrap_or("My Vault").to_string();
                            if token.is_empty() || pin.is_empty() {
                                navigator.push(Route::Identity {});
                                return;
                            }
                            (token, pin, name)
                        }
                        Err(_) => { navigator.push(Route::Identity {}); return; }
                    }
                }
                Err(_) => { navigator.push(Route::Identity {}); return; }
            };
            
            session_token_sig.set(session_token.clone());
            user_pin_sig.set(user_pin.clone());
            vault_name.set(format!("{}'s Vault", display_name));
            
            match get_vault_metadata(session_token.clone()).await {
                Ok(encrypted_blob) => {
                    if encrypted_blob.is_empty() {
                        vault_name.set(get_vault_name(&session_token.split('_').next().unwrap_or("")));
                        loading.set(false);
                        return;
                    }
                    
                    let blob_json = serde_json::to_string(&encrypted_blob).unwrap_or("[]".to_string());
                    
                    let decrypt_js = format!(r#"
                        (async function() {{
                            try {{
                                const encryptedData = new Uint8Array({blob_json});
                                const pin = '{user_pin}';
                                const encoder = new TextEncoder();
                                const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                                const cryptoKey = await crypto.subtle.importKey('raw', keyHash, {{ name: 'AES-GCM' }}, false, ['decrypt']);
                                
                                // 1. Decrypt Metadata Blob
                                // Format: [nonce (12 bytes)] [encrypted data]
                                const metaNonce = encryptedData.slice(0, 12);
                                const metaEnc = encryptedData.slice(12);
                                
                                const decryptedMetaBytes = await crypto.subtle.decrypt({{ name: 'AES-GCM', iv: metaNonce }}, cryptoKey, metaEnc);
                                const metadata = JSON.parse(new TextDecoder().decode(decryptedMetaBytes));
                                
                                // 2. Decrypt Item Names
                                const decryptedItems = [];
                                for (const item of metadata.items) {{
                                    try {{
                                        const encName = new Uint8Array(item.encrypted_name);
                                        const nameNonce = new Uint8Array(item.name_nonce);
                                        const decryptedNameBytes = await crypto.subtle.decrypt({{ name: 'AES-GCM', iv: nameNonce }}, cryptoKey, encName);
                                        const name = new TextDecoder().decode(decryptedNameBytes);
                                        
                                        decryptedItems.push({{
                                            id: item.id,
                                            name: name,
                                            encrypted_name: Array.from(encName),
                                            item_type: item.item_type,
                                            size: item.size,
                                            nonce: Array.from(new Uint8Array(item.nonce)),
                                            name_nonce: Array.from(nameNonce),
                                            content_id: item.content_id,
                                            preview_id: item.preview_id,
                                            preview_url: null
                                        }});
                                    }} catch (e) {{
                                        console.error('Failed to decrypt item name', e);
                                        decryptedItems.push({{
                                            id: item.id,
                                            name: '[Encrypted]',
                                            encrypted_name: item.encrypted_name,
                                            item_type: item.item_type,
                                            size: item.size,
                                            nonce: item.nonce,
                                            name_nonce: item.name_nonce,
                                            content_id: item.content_id,
                                            preview_id: item.preview_id,
                                            preview_url: null
                                        }});
                                    }}
                                }}
                                
                                dioxus.send(JSON.stringify({{ success: true, name: metadata.name, items: decryptedItems }}));
                            }} catch (e) {{
                                dioxus.send(JSON.stringify({{ error: e.toString() }}));
                            }}
                        }})()
                    "#);
                    
                    let mut eval = document::eval(&decrypt_js);
                    match eval.recv::<String>().await {
                        Ok(result) => {
                            if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                                if json.get("success").is_some() {
                                    if let Some(name) = json["name"].as_str() {
                                        vault_name.set(name.to_string());
                                    }
                                    if let Some(items) = json["items"].as_array() {
                                        let decrypted: Vec<DecryptedItem> = items.iter().filter_map(|i| {
                                            Some(DecryptedItem {
                                                id: i["id"].as_str()?.to_string(),
                                                name: i["name"].as_str()?.to_string(),
                                                encrypted_name: serde_json::from_value(i["encrypted_name"].clone()).ok()?,
                                                item_type: i["item_type"].as_str()?.to_string(),
                                                size: i["size"].as_u64()? as usize,
                                                nonce: serde_json::from_value(i["nonce"].clone()).ok()?,
                                                name_nonce: serde_json::from_value(i["name_nonce"].clone()).ok()?,
                                                content_id: i["content_id"].as_str()?.to_string(),
                                                preview_id: i["preview_id"].as_str().map(|s| s.to_string()),
                                                preview_url: None,
                                            })
                                        }).collect();
                                        vault_items.set(decrypted);
                                    }
                                } else if let Some(err) = json.get("error") {
                                    error_msg.set(Some(format!("Decrypt error: {}", err)));
                                }
                            } 
                        }
                        Err(e) => error_msg.set(Some(format!("Decrypt error: {:?}", e))),
                    }
                    loading.set(false);
                }
                Err(e) => { error_msg.set(Some(e.to_string())); loading.set(false); }
            }
        });
    });

    // Helper to save metadata
    let save_current_state = move || {
        let items = vault_items();
        let name = vault_name();
        let token = session_token_sig();
        let pin = user_pin_sig();
        
        spawn(async move {
            let export_items: Vec<VaultItem> = items.iter().map(|i| VaultItem {
                id: i.id.clone(),
                encrypted_name: i.encrypted_name.clone(),
                name_nonce: i.name_nonce.clone(),
                item_type: i.item_type.clone(),
                size: i.size,
                nonce: i.nonce.clone(),
                content_id: i.content_id.clone(),
                preview_id: i.preview_id.clone(),
            }).collect();
            
            let metadata = VaultMetadata { name, items: export_items };
            let json_str = serde_json::to_string(&metadata).unwrap_or("{}".to_string());
            
            let encrypt_js = format!(r#"
                (async function() {{
                    try {{
                        const jsonStr = {};
                        const pin = '{}';
                        const encoder = new TextEncoder();
                        const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                        const cryptoKey = await crypto.subtle.importKey('raw', keyHash, {{ name: 'AES-GCM' }}, false, ['encrypt']);
                        
                        const nonce = crypto.getRandomValues(new Uint8Array(12));
                        const encrypted = await crypto.subtle.encrypt({{ name: 'AES-GCM', iv: nonce }}, cryptoKey, encoder.encode(jsonStr));
                        
                        // Combine nonce + encrypted data
                        const combined = new Uint8Array(nonce.byteLength + encrypted.byteLength);
                        combined.set(nonce, 0);
                        combined.set(new Uint8Array(encrypted), nonce.byteLength);
                        
                        // Send back as array of numbers for Rust Vec<u8>
                        dioxus.send(JSON.stringify(Array.from(combined)));
                    }} catch(e) {{ dioxus.send("error:" + e.toString()); }}
                }})()
            "#, serde_json::to_string(&json_str).unwrap(), pin);
            
            let mut eval = document::eval(&encrypt_js);
            if let Ok(result) = eval.recv::<String>().await {
                if !result.starts_with("error") {
                    if let Ok(bytes) = serde_json::from_str::<Vec<u8>>(&result) {
                        let _ = save_vault_metadata(SaveMetadataRequest {
                            session_token: token,
                            encrypted_metadata: bytes
                        }).await;
                    }
                }
            }
        });
    };

    // Lazy loading observer setup
    use_effect(move || {
        spawn(async move {
            let mut eval = document::eval(r#"
                const observer = new IntersectionObserver((entries) => {
                    entries.forEach(entry => {
                        if (entry.isIntersecting) {
                            const id = entry.target.getAttribute('data-id');
                            const type = entry.target.getAttribute('data-type');
                            if (id && type) {
                                dioxus.send({id, type});
                                observer.unobserve(entry.target);
                                entry.target.classList.remove('lazy-preview');
                            }
                        }
                    });
                }, { rootMargin: '200px' });

                window.observePreviews = () => {
                    document.querySelectorAll('.lazy-preview').forEach(el => {
                        observer.observe(el);
                    });
                };
                
                // Keep channel open
                await new Promise(() => {});
            "#);
            
            while let Ok(msg) = eval.recv::<serde_json::Value>().await {
                let id = msg["id"].as_str().unwrap_or("").to_string();
                let item_type = msg["type"].as_str().unwrap_or("").to_string();
                
                if !id.is_empty() && (item_type == "photo" || item_type == "video") {
                    // Check if already loading or loaded
                    let already_loading = loading_previews.read().contains(&id);
                    let already_loaded = vault_items.read().iter().find(|i| i.id == id).map(|i| i.preview_url.is_some()).unwrap_or(false);
                    
                    if !already_loading && !already_loaded {
                        loading_previews.write().insert(id.clone());
                        
                        let item_id = id.clone();
                        let env = session_token_sig().split('_').next().unwrap_or("personal").to_string();
                        let pin = user_pin_sig();
                        
                        // Find the item to get its preview_id
                        let preview_id = vault_items.read().iter()
                            .find(|i| i.id == item_id)
                            .and_then(|i| i.preview_id.clone());
                            
                        if let Some(pid) = preview_id {
                            spawn(async move {
                                let preview_js = format!(r#"
                                    (async function() {{
                                        try {{
                                            const res = await fetch('/api/get_preview/{env}/{preview_id}');
                                            if (!res.ok) {{ dioxus.send(JSON.stringify({{ id: '{file_id}', error: true }})); return; }}
                                            
                                            const previewData = new Uint8Array(await res.arrayBuffer());
                                            // Format: [nonce (12 bytes)] [encrypted data]
                                            const nonce = previewData.slice(0, 12);
                                            const encryptedBytes = previewData.slice(12);
                                            
                                            const pin = '{pin}';
                                            const encoder = new TextEncoder();
                                            const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                                            const cryptoKey = await crypto.subtle.importKey('raw', keyHash, {{ name: 'AES-GCM' }}, false, ['decrypt']);
                                            
                                            const decrypted = await crypto.subtle.decrypt({{ name: 'AES-GCM', iv: nonce }}, cryptoKey, encryptedBytes);
                                            const data = new Uint8Array(decrypted);
                                            
                                            const blob = new Blob([data], {{ type: 'image/jpeg' }});
                                            dioxus.send(JSON.stringify({{ id: '{file_id}', url: URL.createObjectURL(blob) }}));
                                        }} catch (e) {{
                                            dioxus.send(JSON.stringify({{ id: '{file_id}', error: true }}));
                                        }}
                                    }})()
                                "#, env = env, file_id = item_id, preview_id = pid, pin = pin);
                                
                                let mut eval = document::eval(&preview_js);
                                if let Ok(result) = eval.recv::<String>().await {
                                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                                        if let (Some(id), Some(url)) = (json["id"].as_str(), json["url"].as_str()) {
                                            vault_items.write().iter_mut().for_each(|item| {
                                                if item.id == id {
                                                    item.preview_url = Some(url.to_string());
                                                }
                                            });
                                        }
                                    }
                                }
                                loading_previews.write().remove(&item_id);
                            });
                        } else {
                            loading_previews.write().remove(&item_id);
                        }
                    }
                }
            }
        });
    });

    // Trigger observer when items change
    use_effect(move || {
        let _ = vault_items();
        spawn(async move {
            let mut eval = document::eval(r#"
                setTimeout(() => {
                    if (window.observePreviews) window.observePreviews();
                    dioxus.send(true);
                }, 200);
            "#);
            let _ = eval.recv::<bool>().await;
        });
    });

    let logout = move |_| {
        let _ = document::eval("sessionStorage.clear();");
        navigator.push(Route::Identity {});
    };

    // Chunked upload using fetch API directly
    let handle_file_upload = {
        let save_current_state = save_current_state.clone();
        move |_| async move {
            uploading.set(true);
            upload_progress.set("Selecting files...".to_string());
            
            let upload_js = r#"
            (async function() {
            try {
                function arrayBufferToBase64(buffer) {
                const bytes = new Uint8Array(buffer);
                const chunkSize = 8192;
                let binary = '';
                for (let i = 0; i < bytes.length; i += chunkSize) {
                    const chunk = bytes.subarray(i, Math.min(i + chunkSize, bytes.length));
                    binary += String.fromCharCode.apply(null, chunk);
                }
                return btoa(binary);
                }
                
                async function generatePreview(file, itemType) {
                return new Promise(async (resolve) => {
                    const timeout = setTimeout(() => resolve(null), 15000);
                    
                    if (itemType === 'photo') {
                    const img = new Image();
                    img.onload = () => {
                        clearTimeout(timeout);
                        const canvas = document.createElement('canvas');
                        // Ensure preview is exactly 320x320
                        canvas.width = 320;
                        canvas.height = 320;
                        const ctx = canvas.getContext('2d');
                        
                        const size = Math.min(img.width, img.height);
                        const x = (img.width - size) / 2;
                        const y = (img.height - size) / 2;
                        
                        // Draw cropped and scaled to 320x320
                        ctx.drawImage(img, x, y, size, size, 0, 0, 320, 320);
                        canvas.toBlob((blob) => {
                        URL.revokeObjectURL(img.src);
                        if (blob) {
                            blob.arrayBuffer().then(ab => resolve(new Uint8Array(ab)));
                        } else {
                            resolve(null);
                        }
                        }, 'image/jpeg', 0.7);
                    };
                    img.onerror = () => { clearTimeout(timeout); resolve(null); };
                    img.src = URL.createObjectURL(file);
                    } else if (itemType === 'video') {
                    let video = null;
                    try {
                        video = document.createElement("video");
                        video.src = URL.createObjectURL(file);
                        video.muted = true;
                        video.playsInline = true;
                        video.style.display = 'none';
                        document.body.appendChild(video);

                        await new Promise((r, e) => {
                        video.onloadedmetadata = r;
                        video.onerror = e;
                        });
                        
                        let time = 1.0;
                        if (video.duration < 2.0) time = video.duration / 2;
                        video.currentTime = Math.min(time, video.duration);

                        await new Promise((r, e) => {
                        video.onseeked = r;
                        video.onerror = e;
                        });

                        const canvas = document.createElement("canvas");
                        // Ensure preview is exactly 320x320
                        canvas.width = 320;
                        canvas.height = 320;
                        const ctx = canvas.getContext("2d");

                        const size = Math.min(video.videoWidth, video.videoHeight);
                        const x = (video.videoWidth - size) / 2;
                        const y = (video.videoHeight - size) / 2;

                        // Draw cropped and scaled to 320x320
                        ctx.drawImage(video, x, y, size, size, 0, 0, 320, 320);

                        canvas.toBlob(blob => {
                        clearTimeout(timeout);
                        URL.revokeObjectURL(video.src);
                        if (video.parentNode) document.body.removeChild(video);
                        
                        if (blob) {
                            blob.arrayBuffer().then(ab => resolve(new Uint8Array(ab)));
                        } else {
                            resolve(null);
                        }
                        }, "image/jpeg", 0.7);
                    } catch (e) {
                        clearTimeout(timeout);
                        if (video) {
                        URL.revokeObjectURL(video.src);
                        if (video.parentNode) document.body.removeChild(video);
                        }
                        resolve(null);
                    }
                    } else {
                    resolve(null);
                    }
                });
                }
                
                const input = document.createElement('input');
                input.type = 'file';
                input.multiple = true;
                input.style.display = 'none';
                document.body.appendChild(input);
                
                const filesSelected = new Promise((resolve) => {
                input.onchange = () => resolve(Array.from(input.files));
                input.click();
                });
                
                const files = await filesSelected;
                document.body.removeChild(input);
                
                if (!files || files.length === 0) {
                dioxus.send(JSON.stringify({ done: true, items: [] }));
                return;
                }
                
                const pin = sessionStorage.getItem('user_pin');
                const token = sessionStorage.getItem('session_token');
                const encoder = new TextEncoder();
                const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['encrypt']);
                
                const CHUNK_SIZE = 1024 * 1024;
                const uploadedItems = [];
                
                for (let fileIdx = 0; fileIdx < files.length; fileIdx++) {
                const file = files[fileIdx];
                const arrayBuffer = await file.arrayBuffer();
                const data = new Uint8Array(arrayBuffer);
                
                const fileNonce = crypto.getRandomValues(new Uint8Array(12));
                const nameNonce = crypto.getRandomValues(new Uint8Array(12));
                
                const encryptedFile = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: fileNonce }, cryptoKey, data);
                const encryptedBytes = new Uint8Array(encryptedFile);
                
                const nameBytes = encoder.encode(file.name);
                const encryptedName = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nameNonce }, cryptoKey, nameBytes);
                
                // Convert to arrays for JSON
                const nameArr = Array.from(new Uint8Array(encryptedName));
                const fileNonceArr = Array.from(fileNonce);
                const nameNonceArr = Array.from(nameNonce);
                
                let itemType = 'document';
                const ext = file.name.split('.').pop().toLowerCase();
                if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) itemType = 'photo';
                else if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(ext)) itemType = 'video';
                else if (['mp3', 'wav', 'ogg', 'm4a', 'flac'].includes(ext)) itemType = 'audio';
                else if (['txt', 'md', 'log', 'rs', 'js', 'ts', 'html', 'css', 'json', 'toml', 'yaml', 'xml', 'c', 'cpp', 'h', 'py', 'sh', 'bat'].includes(ext)) itemType = 'text';
                else if (['key', 'pem', 'env'].includes(ext)) itemType = 'password';
                
                // Generate and encrypt preview for media files (always 320x320)
                let previewArr = null;
                let previewNonceArr = null;
                
                if (itemType === 'photo' || itemType === 'video') {
                    const previewData = await generatePreview(file, itemType);
                    if (previewData) {
                    const previewNonce = crypto.getRandomValues(new Uint8Array(12));
                    const encryptedPreview = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: previewNonce }, cryptoKey, previewData);
                    previewArr = Array.from(new Uint8Array(encryptedPreview));
                    previewNonceArr = Array.from(previewNonce);
                    }
                }
                
                const totalChunks = Math.ceil(encryptedBytes.length / CHUNK_SIZE);
                
                const startRes = await fetch('/api/start_upload', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({
                    session_token: token,
                    encrypted_name: nameArr,
                    name_nonce: nameNonceArr,
                    item_type: itemType,
                    nonce: fileNonceArr,
                    total_chunks: totalChunks,
                    preview: previewArr,
                    preview_nonce: previewNonceArr
                    })
                });
                
                if (!startRes.ok) throw new Error('Failed to start upload');
                const { file_id, encrypted_name, content_id, preview_id } = await startRes.json();
                
                for (let i = 0; i < totalChunks; i++) {
                    const start = i * CHUNK_SIZE;
                    const end = Math.min(start + CHUNK_SIZE, encryptedBytes.length);
                    const chunk = encryptedBytes.slice(start, end);
                    
                    const chunkRes = await fetch(`/api/upload_chunk?token=${token}&file_id=${file_id}&chunk=${i}`, {
                    method: 'POST',
                    body: chunk
                    });
                    
                    if (!chunkRes.ok) throw new Error(`Failed to upload chunk ${i}`);
                }
                
                const finishRes = await fetch('/api/finish_upload', {
                    method: 'POST',
                    headers: { 'Content-Type': 'application/json' },
                    body: JSON.stringify({ session_token: token, file_id })
                });
                
                if (!finishRes.ok) throw new Error('Failed to finish upload');
                const result = await finishRes.json();
                
                if (result.success && result.item) {
                    uploadedItems.push({
                    id: result.item.id,
                    name: file.name,
                    encrypted_name: result.item.encrypted_name,
                    item_type: itemType,
                    size: result.item.size,
                    nonce: fileNonceArr,
                    name_nonce: nameNonceArr,
                    content_id: result.item.content_id,
                    preview_id: result.item.preview_id,
                    preview_url: null,
                    });
                }
                }
                
                dioxus.send(JSON.stringify({ done: true, items: uploadedItems }));
            } catch (e) {
                dioxus.send(JSON.stringify({ error: e.toString() }));
            }
            })()
        "#;
        
            let mut eval = document::eval(upload_js);
            match eval.recv::<String>().await {
                Ok(result) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                        if let Some(err) = json.get("error") {
                            error_msg.set(Some(err.as_str().unwrap_or("Upload error").to_string()));
                        } else if let Some(items) = json["items"].as_array() {
                            for item in items {
                                vault_items.write().push(DecryptedItem {
                                    id: item["id"].as_str().unwrap_or("").to_string(),
                                    name: item["name"].as_str().unwrap_or("").to_string(),
                                    encrypted_name: serde_json::from_value(item["encrypted_name"].clone()).unwrap_or_default(),
                                    item_type: item["item_type"].as_str().unwrap_or("document").to_string(),
                                    size: item["size"].as_u64().unwrap_or(0) as usize,
                                    nonce: serde_json::from_value(item["nonce"].clone()).unwrap_or_default(),
                                    name_nonce: serde_json::from_value(item["name_nonce"].clone()).unwrap_or_default(),
                                    content_id: item["content_id"].as_str().unwrap_or("").to_string(),
                                    preview_id: item["preview_id"].as_str().map(|s| s.to_string()),
                                    preview_url: None,
                                });
                            }
                            save_current_state();
                        }
                    }
                }
                Err(e) => error_msg.set(Some(format!("Upload error: {:?}", e))),
            }
            
            uploading.set(false);
            upload_progress.set(String::new());
        }
    };

    let open_file = move |item: DecryptedItem| async move {
        view_loading.set(true);
        viewing_item.set(Some(item.clone()));
        view_content.set(None);
        
        let mut eval = document::eval("dioxus.send(sessionStorage.getItem('environment'));");
        let env = match eval.recv::<String>().await {
            Ok(e) => e,
            Err(_) => { view_loading.set(false); return; }
        };
        
        let nonce_json = serde_json::to_string(&item.nonce).unwrap_or("[]".to_string());
        
        let decrypt_js = format!(r#"
            (async function() {{
                try {{
                    const res = await fetch('/api/get_file/{env}/{content_id}');
                    if (!res.ok) throw new Error('Failed to fetch file');
                    
                    const encryptedBytes = new Uint8Array(await res.arrayBuffer());
                    const pin = sessionStorage.getItem('user_pin');
                    const nonce = new Uint8Array({nonce_json});
                    const fileName = '{file_name}';
                    
                    const encoder = new TextEncoder();
                    const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                    const cryptoKey = await crypto.subtle.importKey('raw', keyHash, {{ name: 'AES-GCM' }}, false, ['decrypt']);
                    
                    const decrypted = await crypto.subtle.decrypt({{ name: 'AES-GCM', iv: nonce }}, cryptoKey, encryptedBytes);
                    const data = new Uint8Array(decrypted);
                    const ext = fileName.split('.').pop().toLowerCase();
                    
                    if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) {{
                        const mimeTypes = {{ 'jpg': 'image/jpeg', 'jpeg': 'image/jpeg', 'png': 'image/png', 'gif': 'image/gif', 'webp': 'image/webp', 'bmp': 'image/bmp', 'svg': 'image/svg+xml' }};
                        const blob = new Blob([data], {{ type: mimeTypes[ext] || 'image/png' }});
                        dioxus.send(JSON.stringify({{ type: 'image', data: URL.createObjectURL(blob) }}));
                    }} else if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(ext)) {{
                        const mimeTypes = {{ 'mp4': 'video/mp4', 'webm': 'video/webm', 'mov': 'video/quicktime', 'avi': 'video/x-msvideo', 'mkv': 'video/x-matroska' }};
                        const blob = new Blob([data], {{ type: mimeTypes[ext] || 'video/mp4' }});
                        dioxus.send(JSON.stringify({{ type: 'video', data: URL.createObjectURL(blob) }}));
                    }} else if (['mp3', 'wav', 'ogg', 'm4a', 'flac'].includes(ext)) {{
                        const mimeTypes = {{ 'mp3': 'audio/mpeg', 'wav': 'audio/wav', 'ogg': 'audio/ogg', 'm4a': 'audio/mp4', 'flac': 'audio/flac' }};
                        const blob = new Blob([data], {{ type: mimeTypes[ext] || 'audio/mpeg' }});
                        dioxus.send(JSON.stringify({{ type: 'audio', data: URL.createObjectURL(blob) }}));
                    }} else if (['txt', 'md', 'log', 'rs', 'js', 'ts', 'html', 'css', 'json', 'toml', 'yaml', 'xml', 'c', 'cpp', 'h', 'py', 'sh', 'bat', 'key', 'pem', 'env'].includes(ext)) {{
                        dioxus.send(JSON.stringify({{ type: 'text', data: new TextDecoder().decode(data) }}));
                    }} else {{
                        const blob = new Blob([data]);
                        const url = URL.createObjectURL(blob);
                        const a = document.createElement('a');
                        a.href = url;
                        a.download = fileName;
                        a.click();
                        URL.revokeObjectURL(url);
                        dioxus.send(JSON.stringify({{ type: 'download', data: 'Downloaded' }}));
                    }}
                }} catch (e) {{
                    dioxus.send(JSON.stringify({{ type: 'error', data: e.toString() }}));
                }}
            }})()
        "#, env = env, content_id = item.content_id, nonce_json = nonce_json, file_name = item.name.replace('\'', "\\'"));
        
        let mut eval = document::eval(&decrypt_js);
        match eval.recv::<String>().await {
            Ok(result) => {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                    let content_type = json["type"].as_str().unwrap_or("error");
                    let data = json["data"].as_str().unwrap_or("").to_string();
                    
                    if content_type == "error" {
                        error_msg.set(Some(data));
                        viewing_item.set(None);
                    } else if content_type == "download" {
                        viewing_item.set(None);
                    } else {
                        view_content.set(Some(format!("{}:{}", content_type, data)));
                    }
                }
            }
            Err(e) => { error_msg.set(Some(format!("Error: {:?}", e))); viewing_item.set(None); }
        }
        
        view_loading.set(false);
    };

    let delete_item = {
        let save_current_state = save_current_state.clone();
        move |item_id: String| async move {
            let mut eval = document::eval("dioxus.send(sessionStorage.getItem('session_token'));");
            let token = match eval.recv::<String>().await { Ok(t) => t, Err(_) => return };
            
            let item = vault_items.read().iter().find(|i| i.id == item_id).cloned();
            if let Some(item) = item {
                let mut files_to_delete = vec![item.content_id];
                if let Some(pid) = item.preview_id {
                    files_to_delete.push(pid);
                }
                
                if let Ok(true) = delete_files(token, files_to_delete).await {
                    vault_items.write().retain(|i| i.id != item_id);
                    viewing_item.set(None);
                    view_content.set(None);
                    save_current_state();
                }
            }
        }
    };

    let get_icon = |item_type: &str| -> &str {
        match item_type { 
            "photo" => "🖼️", 
            "video" => "🎬", 
            "audio" => "🎵",
            "text" => "📝",
            "document" => "📄", 
            "password" => "🔑", 
            "note" => "📝", 
            _ => "📦" 
        }
    };

    let format_size = |size: usize| -> String {
        if size < 1024 { format!("{} B", size) }
        else if size < 1024 * 1024 { format!("{:.1} KB", size as f64 / 1024.0) }
        else { format!("{:.1} MB", size as f64 / (1024.0 * 1024.0)) }
    };

    rsx! {
        document::Stylesheet { href: VAULT_CSS }
        
        // Modal viewer
        if let Some(item) = viewing_item() {
            div { class: "modal-overlay",
                onclick: move |_| { viewing_item.set(None); view_content.set(None); },
                div { class: "modal-glass",
                    onclick: move |e| e.stop_propagation(),
                    div { class: "modal-header",
                        div { class: "modal-title-row",
                            span { class: "modal-icon", "{get_icon(&item.item_type)}" }
                            h3 { "{item.name}" }
                        }
                        button { class: "modal-close", onclick: move |_| { viewing_item.set(None); view_content.set(None); }, "✕" }
                    }
                    div { class: "modal-body",
                        if view_loading() {
                            div { class: "modal-loading",
                                div { class: "spinner" }
                                p { "Decrypting..." }
                            }
                        } else if let Some(content) = view_content() {
                            {
                                if content.starts_with("image:") {
                                    let src = content.strip_prefix("image:").unwrap_or("");
                                    rsx! { img { class: "modal-media", src: "{src}" } }
                                } else if content.starts_with("video:") {
                                    let src = content.strip_prefix("video:").unwrap_or("");
                                    rsx! { video { class: "modal-media", controls: true, autoplay: true, src: "{src}" } }
                                } else if content.starts_with("audio:") {
                                    let src = content.strip_prefix("audio:").unwrap_or("");
                                    rsx! { 
                                        div { class: "modal-audio-container",
                                            div { class: "audio-visualizer", "🎵" }
                                            audio { controls: true, autoplay: true, src: "{src}" }
                                        }
                                    }
                                } else if content.starts_with("text:") {
                                    let txt = content.strip_prefix("text:").unwrap_or("");
                                    rsx! { pre { class: "modal-text", "{txt}" } }
                                } else {
                                    rsx! { p { "{content}" } }
                                }
                            }
                        }
                    }
                    div { class: "modal-footer",
                        span { class: "modal-size", "{format_size(item.size)}" }
                        button { class: "btn-danger",
                            onclick: move |_| { let id = item.id.clone(); spawn(async move { delete_item(id).await; }); },
                            "🗑️ Delete"
                        }
                    }
                }
            }
        }
        
        // Error toast
        if let Some(err) = error_msg() {
            div { class: "error-toast",
                span { "{err}" }
                button { onclick: move |_| error_msg.set(None), "✕" }
            }
        }
        
        div { class: "vault-page",
            // Header
            header { class: "vault-header glass-bar",
                div { class: "header-left",
                    span { class: "vault-logo", "🔐" }
                    div { class: "vault-info",
                        h1 { "{vault_name}" }
                        p { "{vault_items().len()} encrypted items" }
                    }
                }
                div { class: "header-actions",
                    // View toggle
                    div { class: "view-toggle",
                        button {
                            class: if view_mode() == "grid" { "active" } else { "" },
                            onclick: move |_| view_mode.set("grid".to_string()),
                            "⊞"
                        }
                        button {
                            class: if view_mode() == "list" { "active" } else { "" },
                            onclick: move |_| view_mode.set("list".to_string()),
                            "☰"
                        }
                    }
                    button {
                        class: "btn-primary",
                        disabled: uploading(),
                        onclick: handle_file_upload,
                        if uploading() { "⏳ Uploading..." } else { "➕ Upload" }
                    }
                    button {
                        class: "btn-chat",
                        onclick: move |_| { navigator.push(Route::Chat {}); },
                        "💬 Chat"
                    }
                    button { class: "btn-logout", onclick: logout, "🔒" }
                }
            }
            
            // Main content
            main { class: "vault-main",
                if loading() {
                    div { class: "loading-state",
                        div { class: "spinner large" }
                        p { "Decrypting vault..." }
                    }
                } else if vault_items().is_empty() {
                    div { class: "empty-state",
                        div { class: "empty-icon", "📂" }
                        h2 { "Your vault is empty" }
                        p { "Upload files to encrypt and store them securely" }
                        button {
                            class: "btn-primary large",
                            onclick: handle_file_upload,
                            "📁 Upload Files"
                        }
                    }
                } else {
                    div { class: if view_mode() == "grid" { "gallery-grid" } else { "gallery-list" },
                        for item in vault_items() {
                            div {
                                class: if (item.item_type == "photo" || item.item_type == "video") && item.preview_url.is_none() { "gallery-item glass-card lazy-preview" } else { "gallery-item glass-card" },
                                "data-id": "{item.id}",
                                "data-type": "{item.item_type}",
                                onclick: { let ic = item.clone(); move |_| { let i = ic.clone(); spawn(async move { open_file(i).await }); } },
                                if let Some(preview_url) = &item.preview_url {
                                    // Previews are JPEG thumbnails for both photos and videos
                                    img { class: "item-preview", src: "{preview_url}" }
                                } else {
                                    div { class: "item-icon", "{get_icon(&item.item_type)}" }
                                }
                                if view_mode() == "list" {
                                    div { class: "item-details",
                                        span { class: "item-name", "{item.name}" }
                                        span { class: "item-meta", "{item.item_type} • {format_size(item.size)}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
    }
}

pub fn get_vault_name(environment: &str) -> String {
    match environment {
        "personal" => "Personal Vault".to_string(),
        "emergency" => "Family Photos".to_string(),
        "work" => "Work Documents".to_string(),
        _ => "Vault".to_string(),
    }
}
