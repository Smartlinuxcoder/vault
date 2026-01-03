use dioxus::prelude::*;
use crate::Route;
use crate::p2p_types::PeerNode;

// CSS loaded from external file
static NETWORK_CSS: Asset = asset!("/assets/css/network.css");

/// Componente per la vista del network P2P
#[component]
pub fn NetworkView() -> Element {
    let navigator = use_navigator();
    let mut my_pubkey = use_signal(|| String::new());
    let mut my_name = use_signal(|| String::new());
    let mut online_peers = use_signal(|| Vec::<PeerNode>::new());
    let mut node_info = use_signal(|| Option::<serde_json::Value>::None);
    let mut connected = use_signal(|| false);
    let mut show_my_key = use_signal(|| false);
    let mut show_export = use_signal(|| false);
    let mut export_privkey = use_signal(|| String::new());

    // Carica identità e connetti al WebSocket
    use_effect(move || {
        spawn(async move {
            // Verifica sessione
            let check_js = r#"
                (async function() {
                    const pubkey = sessionStorage.getItem('p2p_pubkey');
                    const name = sessionStorage.getItem('p2p_name');
                    const privkey = sessionStorage.getItem('p2p_privkey');
                    if (!pubkey || !privkey) {
                        dioxus.send(JSON.stringify({ redirect: true }));
                    } else {
                        dioxus.send(JSON.stringify({ pubkey, name, privkey }));
                    }
                })()
            "#;
            
            let mut eval = document::eval(check_js);
            if let Ok(result) = eval.recv::<String>().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                    if json["redirect"].as_bool() == Some(true) {
                        navigator.push(Route::Identity {});
                        return;
                    }
                    my_pubkey.set(json["pubkey"].as_str().unwrap_or("").to_string());
                    my_name.set(json["name"].as_str().unwrap_or("Anonymous").to_string());
                    export_privkey.set(json["privkey"].as_str().unwrap_or("").to_string());
                }
            }

            // Ottieni info del nodo server
            let node_js = r#"
                (async function() {
                    try {
                        const res = await fetch('/p2p/info');
                        const data = await res.json();
                        dioxus.send(JSON.stringify(data));
                    } catch (e) {
                        dioxus.send(JSON.stringify({ error: e.toString() }));
                    }
                })()
            "#;
            
            let mut eval = document::eval(node_js);
            if let Ok(result) = eval.recv::<String>().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                    if json.get("error").is_none() {
                        node_info.set(Some(json));
                    }
                }
            }

            // Connetti WebSocket
            let ws_js = r#"
                (async function() {
                    const pubkey = sessionStorage.getItem('p2p_pubkey');
                    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
                    
                    // Chiudi socket esistente se presente
                    if (window.p2pSocket) {
                        try { window.p2pSocket.close(); } catch(e) {}
                    }
                    
                    // Inizializza stato globale
                    window.p2pConnected = false;
                    window.p2pPeers = [];
                    
                    const ws = new WebSocket(`${protocol}//${location.host}/p2p/ws`);
                    window.p2pSocket = ws;
                    
                    ws.onopen = () => {
                        console.log('[P2P Network] WebSocket opened, registering...');
                        ws.send(JSON.stringify({
                            Register: { pubkey: pubkey, signature: 'dev_mode' }
                        }));
                    };
                    
                    ws.onmessage = (event) => {
                        try {
                            const data = JSON.parse(event.data);
                            console.log('[P2P Network] Received:', data);
                            
                            if (data.Registered && data.Registered.success) {
                                window.p2pConnected = true;
                                ws.send(JSON.stringify({ ListPeers: null }));
                            }
                        } catch(e) {}
                        dioxus.send(event.data);
                    };
                    
                    ws.onclose = () => {
                        console.log('[P2P Network] WebSocket closed');
                        window.p2pConnected = false;
                        dioxus.send(JSON.stringify({ Disconnected: true }));
                    };
                    
                    ws.onerror = (e) => {
                        console.error('[P2P Network] WebSocket error:', e);
                        window.p2pConnected = false;
                    };
                    
                    // Keep alive
                    setInterval(() => {
                        if (ws.readyState === WebSocket.OPEN) {
                            ws.send(JSON.stringify({ Ping: null }));
                        }
                    }, 30000);
                    
                    await new Promise(() => {});
                })()
            "#;
            
            let mut ws_eval = document::eval(ws_js);
            while let Ok(msg) = ws_eval.recv::<String>().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&msg) {
                    if json.get("Registered").is_some() {
                        if json["Registered"]["success"].as_bool() == Some(true) {
                            connected.set(true);
                            // Request peer list già inviato nel JS
                        }
                    } else if let Some(peers_arr) = json.get("PeerList").and_then(|p| p["peers"].as_array()) {
                        let peers: Vec<PeerNode> = peers_arr.iter().filter_map(|p| {
                            Some(PeerNode::new(
                                p["pubkey"].as_str()?.to_string(),
                                String::new(),
                                0,
                                false,
                            ))
                        }).collect();
                        online_peers.set(peers);
                    } else if json.get("PeerStatus").is_some() {
                        let pubkey = json["PeerStatus"]["pubkey"].as_str().unwrap_or("");
                        let online = json["PeerStatus"]["online"].as_bool().unwrap_or(false);
                        
                        if online {
                            let mut peers = online_peers();
                            if !peers.iter().any(|p| p.pubkey == pubkey) {
                                peers.push(PeerNode::new(pubkey.to_string(), String::new(), 0, false));
                                online_peers.set(peers);
                            }
                        } else {
                            online_peers.write().retain(|p| p.pubkey != pubkey);
                        }
                    } else if json.get("Disconnected").is_some() {
                        connected.set(false);
                    }
                }
            }
        });
    });

    // Aggiorna gli identicon dopo ogni render
    use_effect(move || {
        let _ = document::eval("if (typeof jdenticon !== 'undefined') { jdenticon(); }");
    });

    let logout = move |_| {
        let _ = document::eval("sessionStorage.clear();");
        navigator.push(Route::Identity {});
    };

    let copy_pubkey = move |_| {
        let pk = my_pubkey();
        let _ = document::eval(&format!("navigator.clipboard.writeText('{}');", pk));
    };

    let copy_privkey = move |_| {
        let pk = export_privkey();
        let _ = document::eval(&format!("navigator.clipboard.writeText('{}');", pk));
    };

    let short_key = |key: &str| -> String {
        if key.len() > 16 {
            format!("{}...{}", &key[..8], &key[key.len()-8..])
        } else {
            key.to_string()
        }
    };

    rsx! {
        document::Stylesheet { href: NETWORK_CSS }
        
        div { class: "network-page",
            // Header
            header { class: "network-header glass-bar",
                div { class: "header-left",
                    span { class: "header-logo", "🌐" }
                    div { class: "header-info",
                        h1 { "PeerWave Network" }
                        div { class: "connection-badge",
                            span { class: if connected() { "status-dot online" } else { "status-dot offline" } }
                            span { if connected() { "Connected" } else { "Disconnected" } }
                        }
                    }
                }
                div { class: "header-actions",
                    button {
                        class: "nav-btn",
                        onclick: move |_| { navigator.push(Route::Chat {}); },
                        "💬 Chat"
                    }
                    button {
                        class: "nav-btn",
                        onclick: move |_| { navigator.push(Route::Vault {}); },
                        "📁 Vault"
                    }
                    button {
                        class: "nav-btn logout",
                        onclick: logout,
                        "🔒 Lock"
                    }
                }
            }
            
            main { class: "network-main",
                // My Identity Card
                section { class: "identity-card glass-card",
                    div { class: "card-header",
                        h2 { "👤 My Identity" }
                        button {
                            class: "export-btn",
                            onclick: move |_| { show_export.set(!show_export()); },
                            if show_export() { "Hide Key" } else { "Export Key" }
                        }
                    }
                    
                    div { class: "identity-info",
                        div { class: "avatar-large",
                            svg {
                                width: "80",
                                height: "80",
                                "data-jdenticon-value": "{my_pubkey}"
                            }
                        }
                        div { class: "identity-details",
                            h3 { "{my_name}" }
                            div { 
                                class: "pubkey-row",
                                onclick: move |_| { show_my_key.set(!show_my_key()); },
                                code { 
                                    if show_my_key() {
                                        "{my_pubkey().chars().take(60).collect::<String>()}..."
                                    } else {
                                        "{short_key(&my_pubkey())}"
                                    }
                                }
                                button {
                                    class: "copy-btn",
                                    onclick: copy_pubkey,
                                    "📋"
                                }
                            }
                        }
                    }
                    
                    if show_export() {
                        div { class: "export-section",
                            p { class: "warning", "⚠️ Keep your private key secret! Anyone with this key can impersonate you." }
                            div { class: "privkey-box",
                                code { "{export_privkey().chars().take(80).collect::<String>()}..." }
                                button {
                                    class: "copy-btn-large",
                                    onclick: copy_privkey,
                                    "📋 Copy Full Key"
                                }
                            }
                        }
                    }
                }
                
                // Node Info Card
                if let Some(info) = node_info() {
                    section { class: "node-card glass-card",
                        h2 { "🖥️ Server Node" }
                        div { class: "node-info-grid",
                            div { class: "info-item",
                                span { class: "info-label", "Name" }
                                span { class: "info-value", 
                                    "{info[\"node\"][\"name\"].as_str().unwrap_or(\"Unknown\")}" 
                                }
                            }
                            div { class: "info-item",
                                span { class: "info-label", "Address" }
                                span { class: "info-value", 
                                    "{info[\"node\"][\"address\"].as_str().unwrap_or(\"\")}:{info[\"node\"][\"public_port\"].as_u64().unwrap_or(0)}" 
                                }
                            }
                            div { class: "info-item",
                                span { class: "info-label", "Version" }
                                span { class: "info-value", 
                                    "{info[\"node\"][\"version\"].as_str().unwrap_or(\"?\")}" 
                                }
                            }
                            div { class: "info-item",
                                span { class: "info-label", "Secure" }
                                span { class: "info-value", 
                                    if info["node"]["secure"].as_bool() == Some(true) { "🔒 Yes" } else { "🔓 No" }
                                }
                            }
                        }
                        div { class: "node-pubkey",
                            span { class: "info-label", "Node Public Key" }
                            code { "{short_key(info[\"node\"][\"pubkey\"].as_str().unwrap_or(\"\"))}" }
                        }
                    }
                }
                
                // Online Peers
                section { class: "peers-card glass-card",
                    div { class: "card-header",
                        h2 { "👥 Online Peers" }
                        span { class: "peer-count", "{online_peers().len()} online" }
                    }
                    
                    if online_peers().is_empty() {
                        div { class: "empty-peers",
                            div { class: "empty-icon", "🔍" }
                            p { "No other peers online" }
                            p { class: "hint", "Share your public key to connect with others!" }
                        }
                    } else {
                        div { class: "peers-list",
                            for peer in online_peers() {
                                div { class: "peer-item",
                                    div { class: "peer-avatar",
                                        span { class: "status-dot online" }
                                        svg {
                                            width: "40",
                                            height: "40",
                                            "data-jdenticon-value": "{peer.pubkey}"
                                        }
                                    }
                                    div { class: "peer-info",
                                        code { class: "peer-pubkey", "{short_key(&peer.pubkey)}" }
                                        span { class: "peer-status", "Online" }
                                    }
                                    div { class: "peer-actions",
                                        button {
                                            class: "action-btn",
                                            onclick: {
                                                let pk = peer.pubkey.clone();
                                                move |_| {
                                                    let _ = document::eval(&format!(
                                                        "navigator.clipboard.writeText('{}');",
                                                        pk
                                                    ));
                                                }
                                            },
                                            "📋"
                                        }
                                        button {
                                            class: "action-btn chat",
                                            onclick: move |_| { navigator.push(Route::Chat {}); },
                                            "💬"
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
}
