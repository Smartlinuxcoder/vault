use dioxus::prelude::*;
use crate::p2p_types::{ChatMessage, Contact, PeerNode};
use crate::Route;

// CSS loaded from external file
static CHAT_CSS: Asset = asset!("/assets/css/chat.css");

/// Componente principale della chat P2P
#[component]
pub fn P2PChat() -> Element {
    let navigator = use_navigator();
    let mut contacts = use_signal(|| Vec::<Contact>::new());
    let mut online_peers = use_signal(|| Vec::<PeerNode>::new());
    let mut selected_contact = use_signal(|| Option::<String>::None);
    let mut messages = use_signal(|| std::collections::HashMap::<String, Vec<ChatMessage>>::new());
    let mut new_message = use_signal(|| String::new());
    let mut my_pubkey = use_signal(|| String::new());
    let mut my_name = use_signal(|| String::new());
    let mut connected = use_signal(|| false);
    let mut show_add_contact = use_signal(|| false);
    let mut new_contact_pubkey = use_signal(|| String::new());
    let mut new_contact_name = use_signal(|| String::new());
    let mut error_msg = use_signal(|| Option::<String>::None);
    let mut ws_initialized = use_signal(|| false);

    // Inizializza WebSocket con comunicazione continua tramite dioxus.send()
    use_effect(move || {
        if ws_initialized() { return; }
        ws_initialized.set(true);
        
        spawn(async move {
            // Setup e WebSocket unificati - usa dioxus.send() per tutto
            let ws_js = r#"
                (async function() {
                    const pubkey = sessionStorage.getItem('p2p_pubkey');
                    const name = sessionStorage.getItem('p2p_name');
                    
                    if (!pubkey) {
                        dioxus.send(JSON.stringify({ type: 'redirect' }));
                        return;
                    }
                    
                    // Carica dati salvati
                    const contacts = JSON.parse(localStorage.getItem('p2p_contacts') || '[]');
                    const savedMessages = JSON.parse(localStorage.getItem('p2p_messages') || '{}');
                    
                    // Invia dati iniziali
                    dioxus.send(JSON.stringify({ 
                        type: 'init',
                        pubkey: pubkey, 
                        name: name || 'Anonymous',
                        contacts: contacts,
                        messages: savedMessages
                    }));
                    
                    // Chiudi socket esistente se presente
                    if (window.p2pSocket) {
                        try { window.p2pSocket.close(); } catch(e) {}
                    }
                    
                    // Connetti WebSocket
                    const protocol = location.protocol === 'https:' ? 'wss:' : 'ws:';
                    const wsUrl = protocol + '//' + location.host + '/p2p/ws';
                    
                    console.log('[P2P] Connecting to:', wsUrl);
                    
                    const ws = new WebSocket(wsUrl);
                    window.p2pSocket = ws;
                    
                    ws.onopen = function() {
                        console.log('[P2P] WebSocket opened');
                        ws.send(JSON.stringify({
                            Register: { pubkey: pubkey, signature: 'dev_mode' }
                        }));
                    };
                    
                    ws.onmessage = function(event) {
                        try {
                            const data = JSON.parse(event.data);
                            console.log('[P2P] Received:', data);
                            
                            if (data.Registered) {
                                dioxus.send(JSON.stringify({ 
                                    type: 'connected', 
                                    success: data.Registered.success === true 
                                }));
                                if (data.Registered.success) {
                                    ws.send(JSON.stringify({ ListPeers: null }));
                                }
                            } else if (data.PeerList) {
                                const peers = (data.PeerList.peers || []).map(p => p.pubkey);
                                dioxus.send(JSON.stringify({ type: 'peers', peers: peers }));
                            } else if (data.PeerStatus) {
                                dioxus.send(JSON.stringify({ 
                                    type: 'peer_status', 
                                    pubkey: data.PeerStatus.pubkey, 
                                    online: data.PeerStatus.online 
                                }));
                            } else if (data.IncomingMessage) {
                                const payload = new Uint8Array(data.IncomingMessage.encrypted_payload);
                                const text = new TextDecoder().decode(payload);
                                dioxus.send(JSON.stringify({
                                    type: 'message',
                                    from: data.IncomingMessage.from_pubkey,
                                    content: text,
                                    timestamp: data.IncomingMessage.timestamp
                                }));
                            } else if (data.Error) {
                                dioxus.send(JSON.stringify({ type: 'error', message: data.Error.message }));
                            }
                        } catch (e) {
                            console.error('[P2P] Parse error:', e);
                        }
                    };
                    
                    ws.onclose = function() {
                        console.log('[P2P] WebSocket closed');
                        dioxus.send(JSON.stringify({ type: 'disconnected' }));
                    };
                    
                    ws.onerror = function(e) {
                        console.error('[P2P] WebSocket error:', e);
                        dioxus.send(JSON.stringify({ type: 'error', message: 'Connection failed' }));
                    };
                    
                    // Keep alive ping ogni 25 secondi
                    setInterval(function() {
                        if (ws.readyState === WebSocket.OPEN) {
                            ws.send(JSON.stringify({ Ping: null }));
                        }
                    }, 25000);
                    
                    // Mantieni la promise aperta per sempre
                    await new Promise(function() {});
                })()
            "#;
            
            let mut eval = document::eval(ws_js);
            
            // Loop infinito per ricevere messaggi da JavaScript
            while let Ok(msg) = eval.recv::<String>().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&msg) {
                    let msg_type = json["type"].as_str().unwrap_or("");
                    
                    match msg_type {
                        "redirect" => {
                            navigator.push(Route::Identity {});
                            return;
                        }
                        "init" => {
                            if let Some(pk) = json["pubkey"].as_str() {
                                my_pubkey.set(pk.to_string());
                            }
                            if let Some(n) = json["name"].as_str() {
                                my_name.set(n.to_string());
                            }
                            if let Some(c) = json["contacts"].as_array() {
                                let loaded: Vec<Contact> = c.iter().filter_map(|v| {
                                    Some(Contact {
                                        pubkey: v["pubkey"].as_str()?.to_string(),
                                        name: v["name"].as_str()?.to_string(),
                                        added_at: v["added_at"].as_u64().unwrap_or(0),
                                        last_message: v["last_message"].as_u64(),
                                    })
                                }).collect();
                                contacts.set(loaded);
                            }
                            if let Some(msgs) = json["messages"].as_object() {
                                let mut loaded_msgs = std::collections::HashMap::new();
                                for (key, val) in msgs {
                                    if let Some(arr) = val.as_array() {
                                        let chat_msgs: Vec<ChatMessage> = arr.iter().filter_map(|m| {
                                            Some(ChatMessage {
                                                id: m["id"].as_str()?.to_string(),
                                                from_me: m["from_me"].as_bool()?,
                                                content: m["content"].as_str()?.to_string(),
                                                timestamp: m["timestamp"].as_u64()?,
                                                delivered: m["delivered"].as_bool().unwrap_or(true),
                                                read: m["read"].as_bool().unwrap_or(false),
                                            })
                                        }).collect();
                                        loaded_msgs.insert(key.clone(), chat_msgs);
                                    }
                                }
                                messages.set(loaded_msgs);
                            }
                        }
                        "connected" => {
                            let success = json["success"].as_bool().unwrap_or(false);
                            connected.set(success);
                        }
                        "disconnected" => {
                            connected.set(false);
                        }
                        "peers" => {
                            if let Some(peers_arr) = json["peers"].as_array() {
                                let peers: Vec<PeerNode> = peers_arr.iter().filter_map(|p| {
                                    Some(PeerNode::new(
                                        p.as_str()?.to_string(),
                                        String::new(),
                                        0,
                                        false,
                                    ))
                                }).collect();
                                online_peers.set(peers);
                            }
                        }
                        "peer_status" => {
                            let pubkey = json["pubkey"].as_str().unwrap_or("").to_string();
                            let online = json["online"].as_bool().unwrap_or(false);
                            
                            if online {
                                let mut peers = online_peers();
                                if !peers.iter().any(|p| p.pubkey == pubkey) {
                                    peers.push(PeerNode::new(pubkey, String::new(), 0, false));
                                    online_peers.set(peers);
                                }
                            } else {
                                online_peers.write().retain(|p| p.pubkey != pubkey);
                            }
                        }
                        "message" => {
                            let from = json["from"].as_str().unwrap_or("").to_string();
                            let content = json["content"].as_str().unwrap_or("").to_string();
                            let timestamp = json["timestamp"].as_u64().unwrap_or_else(|| {
                                (js_sys::Date::now() / 1000.0) as u64
                            });
                            
                            if !from.is_empty() && !content.is_empty() {
                                let new_msg = ChatMessage {
                                    id: format!("{}-{}", from, timestamp),
                                    from_me: false,
                                    content,
                                    timestamp,
                                    delivered: true,
                                    read: selected_contact() == Some(from.clone()),
                                };
                                
                                messages.write().entry(from.clone()).or_insert_with(Vec::new).push(new_msg);
                                save_messages_to_storage(&messages());
                            }
                        }
                        "error" => {
                            if let Some(err) = json["message"].as_str() {
                                error_msg.set(Some(err.to_string()));
                            }
                        }
                        _ => {}
                    }
                }
            }
        });
    });

    // Aggiorna gli identicon dopo ogni render
    use_effect(move || {
        let _ = document::eval("if (typeof jdenticon !== 'undefined') { jdenticon(); }");
    });

    // Ottieni messaggi per il contatto selezionato
    let get_current_messages = move || -> Vec<ChatMessage> {
        if let Some(contact_pk) = selected_contact() {
            messages().get(&contact_pk).cloned().unwrap_or_default()
        } else {
            Vec::new()
        }
    };

    let mut send_message = move |_: ()| {
        let msg = new_message();
        if msg.is_empty() { return; }
        
        if let Some(to) = selected_contact() {
            let msg_clone = msg.clone();
            let to_clone = to.clone();
            
            spawn(async move {
                let timestamp = (js_sys::Date::now() / 1000.0) as u64;
                
                let escaped_msg = msg_clone
                    .replace('\\', "\\\\")
                    .replace('\'', "\\'")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r");
                
                let send_js = format!(r#"
                    (function() {{
                        const text = '{}';
                        const toPubkey = '{}';
                        
                        if (window.p2pSocket && window.p2pSocket.readyState === WebSocket.OPEN) {{
                            const payload = new TextEncoder().encode(text);
                            window.p2pSocket.send(JSON.stringify({{
                                SendMessage: {{
                                    to_pubkey: toPubkey,
                                    encrypted_payload: Array.from(payload)
                                }}
                            }}));
                            return 'sent';
                        }}
                        return 'not_connected';
                    }})()
                "#, escaped_msg, to_clone);
                
                let mut eval = document::eval(&send_js);
                if let Ok(result) = eval.recv::<String>().await {
                    if result == "sent" {
                        let new_msg = ChatMessage {
                            id: format!("me-{}", timestamp),
                            from_me: true,
                            content: msg_clone,
                            timestamp,
                            delivered: true,
                            read: false,
                        };
                        
                        messages.write().entry(to_clone).or_insert_with(Vec::new).push(new_msg);
                        save_messages_to_storage(&messages());
                    } else {
                        error_msg.set(Some("Not connected".to_string()));
                    }
                }
            });
            new_message.set(String::new());
        }
    };

    let add_contact = move |_| {
        let pubkey = new_contact_pubkey().trim().to_string();
        let name = new_contact_name().trim().to_string();
        
        if pubkey.is_empty() || name.is_empty() { return; }
        
        if pubkey == my_pubkey() {
            error_msg.set(Some("Cannot add yourself".to_string()));
            return;
        }
        
        if contacts().iter().any(|c| c.pubkey == pubkey) {
            error_msg.set(Some("Contact already exists".to_string()));
            return;
        }
        
        let new_contact = Contact {
            pubkey: pubkey.clone(),
            name: name.clone(),
            added_at: (js_sys::Date::now() / 1000.0) as u64,
            last_message: None,
        };
        
        contacts.write().push(new_contact);
        save_contacts_to_storage(&contacts());
        
        new_contact_pubkey.set(String::new());
        new_contact_name.set(String::new());
        show_add_contact.set(false);
    };

    let is_online = |pubkey: &str| -> bool {
        online_peers().iter().any(|p| p.pubkey == pubkey)
    };

    let go_to_vault = move |_| {
        navigator.push(Route::Vault {});
    };

    rsx! {
        document::Stylesheet { href: CHAT_CSS }
        
        div { class: "p2p-chat-container",
            // Sidebar contatti
            aside { class: "contacts-sidebar",
                div { class: "sidebar-header",
                    h3 { "💬 PeerWave" }
                    div { class: "connection-status",
                        span { class: if connected() { "status-dot online" } else { "status-dot offline" } }
                        span { if connected() { "Online" } else { "Connecting..." } }
                    }
                }
                
                div { class: "my-id-section",
                    p { class: "my-name", "👤 {my_name}" }
                    p { class: "label", "Your ID:" }
                    code { class: "pubkey-display", "{my_pubkey().chars().take(24).collect::<String>()}..." }
                    button {
                        class: "btn-copy",
                        title: "Copy full public key",
                        onclick: move |_| {
                            let pk = my_pubkey();
                            let _ = document::eval(&format!("navigator.clipboard.writeText('{}');", pk));
                        },
                        "📋"
                    }
                }
                
                div { class: "contacts-list",
                    if contacts().is_empty() {
                        div { class: "no-contacts",
                            p { "No contacts yet" }
                            p { class: "hint", "Add a contact to start chatting" }
                        }
                    }
                    for contact in contacts() {
                        {
                            let contact_pk = contact.pubkey.clone();
                            let contact_online = is_online(&contact_pk);
                            let unread_count = messages().get(&contact_pk)
                                .map(|msgs| msgs.iter().filter(|m| !m.from_me && !m.read).count())
                                .unwrap_or(0);
                            
                            rsx! {
                                div {
                                    key: "{contact.pubkey}",
                                    class: if selected_contact() == Some(contact.pubkey.clone()) { "contact-item active" } else { "contact-item" },
                                    onclick: {
                                        let pk = contact.pubkey.clone();
                                        move |_| {
                                            selected_contact.set(Some(pk.clone()));
                                            if let Some(msgs) = messages.write().get_mut(&pk) {
                                                for msg in msgs.iter_mut() {
                                                    msg.read = true;
                                                }
                                            }
                                            save_messages_to_storage(&messages());
                                        }
                                    },
                                    div { class: "contact-avatar",
                                        span { class: if contact_online { "status-dot online" } else { "status-dot offline" } }
                                        svg {
                                            width: "40",
                                            height: "40",
                                            "data-jdenticon-value": "{contact.pubkey}"
                                        }
                                    }
                                    div { class: "contact-info",
                                        span { class: "contact-name", "{contact.name}" }
                                        span { class: "contact-status", 
                                            if contact_online { "Online" } else { "Offline" }
                                        }
                                    }
                                    if unread_count > 0 {
                                        span { class: "unread-badge", "{unread_count}" }
                                    }
                                }
                            }
                        }
                    }
                }
                
                div { class: "sidebar-actions",
                    button {
                        class: "btn-add-contact",
                        onclick: move |_| show_add_contact.set(true),
                        "➕ Add Contact"
                    }
                    button {
                        class: "btn-vault",
                        onclick: go_to_vault,
                        "🔐 Vault"
                    }
                }
            }
            
            // Area chat principale
            main { class: "chat-main",
                if let Some(contact_pk) = selected_contact() {
                    {
                        let contact = contacts().iter().find(|c| c.pubkey == contact_pk).cloned();
                        let contact_online = is_online(&contact_pk);
                        rsx! {
                            div { class: "chat-header",
                                div { class: "chat-contact-info",
                                    span { class: "chat-contact-name", 
                                        "{contact.as_ref().map(|c| c.name.as_str()).unwrap_or(\"Unknown\")}" 
                                    }
                                    span { class: "chat-contact-status",
                                        if contact_online { "🟢 Online" } else { "⚫ Offline" }
                                    }
                                }
                            }
                            
                            div { class: "messages-container",
                                if get_current_messages().is_empty() {
                                    div { class: "no-messages",
                                        p { "No messages yet" }
                                        p { class: "hint", "Send a message to start the conversation" }
                                    }
                                }
                                for msg in get_current_messages() {
                                    div { 
                                        key: "{msg.id}",
                                        class: if msg.from_me { "message sent" } else { "message received" },
                                        p { class: "message-text", "{msg.content}" }
                                        span { class: "message-time", "{format_timestamp(msg.timestamp)}" }
                                    }
                                }
                            }
                            
                            div { class: "message-input-container",
                                input {
                                    class: "message-input",
                                    r#type: "text",
                                    placeholder: if contact_online { "Type a message..." } else { "User is offline" },
                                    disabled: !contact_online,
                                    value: "{new_message}",
                                    oninput: move |e| new_message.set(e.value()),
                                    onkeypress: move |e| {
                                        if e.key() == Key::Enter {
                                            send_message(());
                                        }
                                    }
                                }
                                button {
                                    class: "btn-send",
                                    onclick: move |_| send_message(()),
                                    disabled: new_message().is_empty() || !contact_online,
                                    "📤"
                                }
                            }
                        }
                    }
                } else {
                    div { class: "no-chat-selected",
                        div { class: "empty-chat-icon", "💬" }
                        h2 { "Select a contact" }
                        p { "Choose a contact from the list to start chatting" }
                    }
                }
            }
        }
        
        // Modal aggiungi contatto
        if show_add_contact() {
            div { class: "modal-overlay",
                onclick: move |_| show_add_contact.set(false),
                div { class: "add-contact-modal",
                    onclick: move |e| e.stop_propagation(),
                    h3 { "Add Contact" }
                    
                    div { class: "form-group",
                        label { "Name" }
                        input {
                            r#type: "text",
                            placeholder: "Contact name",
                            value: "{new_contact_name}",
                            oninput: move |e| new_contact_name.set(e.value())
                        }
                    }
                    
                    div { class: "form-group",
                        label { "Public Key" }
                        textarea {
                            placeholder: "Paste the contact's public key",
                            value: "{new_contact_pubkey}",
                            oninput: move |e| new_contact_pubkey.set(e.value())
                        }
                    }
                    
                    div { class: "modal-actions",
                        button {
                            class: "btn-secondary",
                            onclick: move |_| show_add_contact.set(false),
                            "Cancel"
                        }
                        button {
                            class: "btn-primary",
                            onclick: add_contact,
                            disabled: new_contact_name().is_empty() || new_contact_pubkey().is_empty(),
                            "Add"
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
    }
}

fn save_messages_to_storage(messages: &std::collections::HashMap<String, Vec<ChatMessage>>) {
    if let Ok(json) = serde_json::to_string(messages) {
        let escaped = json.replace('\\', "\\\\").replace('\'', "\\'");
        let _ = document::eval(&format!("localStorage.setItem('p2p_messages', '{}');", escaped));
    }
}

fn save_contacts_to_storage(contacts: &Vec<Contact>) {
    if let Ok(json) = serde_json::to_string(contacts) {
        let escaped = json.replace('\\', "\\\\").replace('\'', "\\'");
        let _ = document::eval(&format!("localStorage.setItem('p2p_contacts', '{}');", escaped));
    }
}

fn format_timestamp(ts: u64) -> String {
    let now = (js_sys::Date::now() / 1000.0) as u64;
    let diff = now.saturating_sub(ts);
    
    if diff < 60 {
        "Now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
