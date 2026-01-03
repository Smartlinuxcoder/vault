use dioxus::prelude::*;
use crate::Route;

// CSS loaded from external file
static IDENTITY_CSS: Asset = asset!("/assets/css/identity.css");

/// Stato dell'identità utente
#[derive(Clone, PartialEq, Debug)]
pub struct UserIdentity {
    pub pubkey: String,
    pub name: String,
}

/// Componente per setup/login identità
#[component]
pub fn IdentitySetup() -> Element {
    let navigator = use_navigator();
    let mut mode = use_signal(|| "check".to_string()); // check, login, create, import
    let mut pin = use_signal(|| String::new());
    let mut confirm_pin = use_signal(|| String::new());
    let mut display_name = use_signal(|| String::new());
    let mut import_privkey = use_signal(|| String::new());
    let mut error = use_signal(|| Option::<String>::None);
    let mut loading = use_signal(|| false);
    let mut my_pubkey = use_signal(|| String::new());
    let mut show_pubkey = use_signal(|| false);

    // Controlla se esiste già un'identità salvata
    use_effect(move || {
        spawn(async move {
            let check_js = r#"
                (async function() {
                    const identity = localStorage.getItem('p2p_identity');
                    if (identity) {
                        const parsed = JSON.parse(identity);
                        dioxus.send(JSON.stringify({ exists: true, pubkey: parsed.pubkey, name: parsed.name }));
                    } else {
                        dioxus.send(JSON.stringify({ exists: false }));
                    }
                })()
            "#;
            
            let mut eval = document::eval(check_js);
            if let Ok(result) = eval.recv::<String>().await {
                if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                    if json["exists"].as_bool() == Some(true) {
                        my_pubkey.set(json["pubkey"].as_str().unwrap_or("").to_string());
                        display_name.set(json["name"].as_str().unwrap_or("").to_string());
                        mode.set("login".to_string());
                    } else {
                        mode.set("choose".to_string());
                    }
                }
            }
        });
    });

    // Aggiorna gli identicon dopo ogni render
    use_effect(move || {
        let _ = document::eval("if (typeof jdenticon !== 'undefined') { jdenticon(); }");
    });

    // Genera nuova identità
    let create_identity = move |_| {
        if pin().len() < 4 {
            error.set(Some("PIN must be at least 4 digits".to_string()));
            return;
        }
        if pin() != confirm_pin() {
            error.set(Some("PINs don't match".to_string()));
            return;
        }
        if display_name().is_empty() {
            error.set(Some("Please enter a display name".to_string()));
            return;
        }
        
        loading.set(true);
        error.set(None);
        let name = display_name();
        let user_pin = pin();
        
        spawn(async move {
            let create_js = format!(r#"
                (async function() {{
                    try {{
                        // Genera coppia di chiavi RSA
                        const keyPair = await crypto.subtle.generateKey(
                            {{
                                name: "RSA-OAEP",
                                modulusLength: 2048,
                                publicExponent: new Uint8Array([1, 0, 1]),
                                hash: "SHA-256"
                            }},
                            true,
                            ["encrypt", "decrypt"]
                        );
                        
                        // Esporta le chiavi
                        const pubKeySpki = await crypto.subtle.exportKey("spki", keyPair.publicKey);
                        const privKeyPkcs8 = await crypto.subtle.exportKey("pkcs8", keyPair.privateKey);
                        
                        const pubkeyB64 = btoa(String.fromCharCode(...new Uint8Array(pubKeySpki)));
                        const privkeyB64 = btoa(String.fromCharCode(...new Uint8Array(privKeyPkcs8)));
                        
                        // Cripta la chiave privata con il PIN usando AES-GCM
                        const pin = '{pin}';
                        const encoder = new TextEncoder();
                        const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                        const aesKey = await crypto.subtle.importKey('raw', pinHash, {{ name: 'AES-GCM' }}, false, ['encrypt']);
                        
                        const iv = crypto.getRandomValues(new Uint8Array(12));
                        const encryptedPrivkey = await crypto.subtle.encrypt(
                            {{ name: 'AES-GCM', iv: iv }},
                            aesKey,
                            encoder.encode(privkeyB64)
                        );
                        
                        // Combina IV + encrypted data
                        const combined = new Uint8Array(iv.length + encryptedPrivkey.byteLength);
                        combined.set(iv);
                        combined.set(new Uint8Array(encryptedPrivkey), iv.length);
                        const encryptedB64 = btoa(String.fromCharCode(...combined));
                        
                        // Salva l'identità
                        const identity = {{
                            pubkey: pubkeyB64,
                            name: '{name}',
                            encryptedPrivkey: encryptedB64,
                            createdAt: Date.now()
                        }};
                        localStorage.setItem('p2p_identity', JSON.stringify(identity));
                        
                        // Salva anche la sessione attiva
                        sessionStorage.setItem('p2p_pubkey', pubkeyB64);
                        sessionStorage.setItem('p2p_privkey', privkeyB64);
                        sessionStorage.setItem('p2p_name', '{name}');
                        sessionStorage.setItem('user_pin', pin);
                        
                        // Create vault session token from pubkey hash
                        const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkeyB64));
                        const hashArray = Array.from(new Uint8Array(hashBuffer));
                        const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
                        sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
                        sessionStorage.setItem('environment', vaultId);
                        
                        dioxus.send(JSON.stringify({{ success: true, pubkey: pubkeyB64 }}));
                    }} catch (e) {{
                        dioxus.send(JSON.stringify({{ error: e.toString() }}));
                    }}
                }})()
            "#, pin = user_pin, name = name.replace('\'', "\\'"));
            
            let mut eval = document::eval(&create_js);
            match eval.recv::<String>().await {
                Ok(result) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                        if json["success"].as_bool() == Some(true) {
                            navigator.push(Route::Network {});
                        } else if let Some(err) = json["error"].as_str() {
                            error.set(Some(err.to_string()));
                        }
                    }
                }
                Err(e) => error.set(Some(format!("Error: {:?}", e))),
            }
            loading.set(false);
        });
    };

    // Login con PIN esistente
    let login_with_pin = move |_| {
        if pin().len() < 4 {
            error.set(Some("Enter your PIN".to_string()));
            return;
        }
        
        loading.set(true);
        error.set(None);
        let user_pin = pin();
        
        spawn(async move {
            let login_js = format!(r#"
                (async function() {{
                    try {{
                        const identity = JSON.parse(localStorage.getItem('p2p_identity'));
                        if (!identity) {{
                            dioxus.send(JSON.stringify({{ error: 'No identity found' }}));
                            return;
                        }}
                        
                        // Decripta la chiave privata
                        const pin = '{pin}';
                        const encoder = new TextEncoder();
                        const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                        const aesKey = await crypto.subtle.importKey('raw', pinHash, {{ name: 'AES-GCM' }}, false, ['decrypt']);
                        
                        const encryptedData = Uint8Array.from(atob(identity.encryptedPrivkey), c => c.charCodeAt(0));
                        const iv = encryptedData.slice(0, 12);
                        const ciphertext = encryptedData.slice(12);
                        
                        try {{
                            const decrypted = await crypto.subtle.decrypt(
                                {{ name: 'AES-GCM', iv: iv }},
                                aesKey,
                                ciphertext
                            );
                            const privkeyB64 = new TextDecoder().decode(decrypted);
                            
                            // Verifica che la chiave sia valida importandola
                            const privKeyBytes = Uint8Array.from(atob(privkeyB64), c => c.charCodeAt(0));
                            await crypto.subtle.importKey(
                                "pkcs8",
                                privKeyBytes,
                                {{ name: "RSA-OAEP", hash: "SHA-256" }},
                                false,
                                ["decrypt"]
                            );
                            
                            // Salva la sessione
                            sessionStorage.setItem('p2p_pubkey', identity.pubkey);
                            sessionStorage.setItem('p2p_privkey', privkeyB64);
                            sessionStorage.setItem('p2p_name', identity.name);
                            sessionStorage.setItem('user_pin', pin);
                            
                            // Create vault session token from pubkey hash
                            const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(identity.pubkey));
                            const hashArray = Array.from(new Uint8Array(hashBuffer));
                            const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
                            sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
                            sessionStorage.setItem('environment', vaultId);
                            
                            dioxus.send(JSON.stringify({{ success: true }}));
                        }} catch (e) {{
                            dioxus.send(JSON.stringify({{ error: 'Invalid PIN' }}));
                        }}
                    }} catch (e) {{
                        dioxus.send(JSON.stringify({{ error: e.toString() }}));
                    }}
                }})()
            "#, pin = user_pin);
            
            let mut eval = document::eval(&login_js);
            match eval.recv::<String>().await {
                Ok(result) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                        if json["success"].as_bool() == Some(true) {
                            navigator.push(Route::Network {});
                        } else if let Some(err) = json["error"].as_str() {
                            error.set(Some(err.to_string()));
                            pin.set(String::new());
                        }
                    }
                }
                Err(e) => error.set(Some(format!("Error: {:?}", e))),
            }
            loading.set(false);
        });
    };

    // Importa chiave privata esistente
    let import_identity = move |_| {
        if pin().len() < 4 || pin() != confirm_pin() {
            error.set(Some("PINs must match and be at least 4 digits".to_string()));
            return;
        }
        if import_privkey().is_empty() {
            error.set(Some("Please paste your private key".to_string()));
            return;
        }
        if display_name().is_empty() {
            error.set(Some("Please enter a display name".to_string()));
            return;
        }
        
        loading.set(true);
        error.set(None);
        let privkey = import_privkey();
        let name = display_name();
        let user_pin = pin();
        
        spawn(async move {
            let import_js = format!(r#"
                (async function() {{
                    try {{
                        const privkeyB64 = '{}';
                        const privKeyBytes = Uint8Array.from(atob(privkeyB64), c => c.charCodeAt(0));
                        
                        // Importa e verifica la chiave
                        const privateKey = await crypto.subtle.importKey(
                            "pkcs8",
                            privKeyBytes,
                            {{ name: "RSA-OAEP", hash: "SHA-256" }},
                            true,
                            ["decrypt"]
                        );
                        
                        // Deriva la chiave pubblica (export e reimport come public)
                        // Per RSA-OAEP non possiamo derivare direttamente, quindi usiamo un workaround
                        // Generiamo una nuova coppia e usiamo solo la struttura
                        const keyPair = await crypto.subtle.generateKey(
                            {{ name: "RSA-OAEP", modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: "SHA-256" }},
                            true,
                            ["encrypt", "decrypt"]
                        );
                        
                        // Esporta la chiave privata importata per ottenere info
                        const reexported = await crypto.subtle.exportKey("pkcs8", privateKey);
                        
                        // Per ottenere la pubkey, dobbiamo usare un approccio diverso
                        // Usiamo la chiave privata per firmare e derivare la pubkey dal JWK
                        const jwk = await crypto.subtle.exportKey("jwk", privateKey);
                        delete jwk.d;
                        delete jwk.p;
                        delete jwk.q;
                        delete jwk.dp;
                        delete jwk.dq;
                        delete jwk.qi;
                        jwk.key_ops = ["encrypt"];
                        
                        const publicKey = await crypto.subtle.importKey(
                            "jwk",
                            jwk,
                            {{ name: "RSA-OAEP", hash: "SHA-256" }},
                            true,
                            ["encrypt"]
                        );
                        
                        const pubKeySpki = await crypto.subtle.exportKey("spki", publicKey);
                        const pubkeyB64 = btoa(String.fromCharCode(...new Uint8Array(pubKeySpki)));
                        
                        // Cripta la chiave privata con il PIN
                        const pin = '{pin}';
                        const encoder = new TextEncoder();
                        const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
                        const aesKey = await crypto.subtle.importKey('raw', pinHash, {{ name: 'AES-GCM' }}, false, ['encrypt']);
                        
                        const iv = crypto.getRandomValues(new Uint8Array(12));
                        const encryptedPrivkey = await crypto.subtle.encrypt(
                            {{ name: 'AES-GCM', iv: iv }},
                            aesKey,
                            encoder.encode(privkeyB64)
                        );
                        
                        const combined = new Uint8Array(iv.length + encryptedPrivkey.byteLength);
                        combined.set(iv);
                        combined.set(new Uint8Array(encryptedPrivkey), iv.length);
                        const encryptedB64 = btoa(String.fromCharCode(...combined));
                        
                        // Salva
                        const identity = {{
                            pubkey: pubkeyB64,
                            name: '{name}',
                            encryptedPrivkey: encryptedB64,
                            createdAt: Date.now()
                        }};
                        localStorage.setItem('p2p_identity', JSON.stringify(identity));
                        
                        sessionStorage.setItem('p2p_pubkey', pubkeyB64);
                        sessionStorage.setItem('p2p_privkey', privkeyB64);
                        sessionStorage.setItem('p2p_name', '{name}');
                        sessionStorage.setItem('user_pin', pin);
                        
                        // Create vault session token from pubkey hash
                        const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkeyB64));
                        const hashArray = Array.from(new Uint8Array(hashBuffer));
                        const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
                        sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
                        sessionStorage.setItem('environment', vaultId);
                        
                        dioxus.send(JSON.stringify({{ success: true, pubkey: pubkeyB64 }}));
                    }} catch (e) {{
                        dioxus.send(JSON.stringify({{ error: 'Invalid private key: ' + e.toString() }}));
                    }}
                }})()
            "#, privkey, pin = user_pin, name = name.replace('\'', "\\'"));
            
            let mut eval = document::eval(&import_js);
            match eval.recv::<String>().await {
                Ok(result) => {
                    if let Ok(json) = serde_json::from_str::<serde_json::Value>(&result) {
                        if json["success"].as_bool() == Some(true) {
                            navigator.push(Route::Network {});
                        } else if let Some(err) = json["error"].as_str() {
                            error.set(Some(err.to_string()));
                        }
                    }
                }
                Err(e) => error.set(Some(format!("Error: {:?}", e))),
            }
            loading.set(false);
        });
    };

    rsx! {
        document::Stylesheet { href: IDENTITY_CSS }
        
        div { class: "identity-page",
            div { class: "identity-container",
                // Header
                div { class: "identity-header",
                    div { class: "identity-logo", "🔑" }
                    h1 { "PeerWave Identity" }
                    p { "Secure, decentralized identity management" }
                }
                
                // Content based on mode
                match mode().as_str() {
                    "check" => rsx! {
                        div { class: "loading-state",
                            div { class: "spinner large" }
                            p { "Checking identity..." }
                        }
                    },
                    
                    "choose" => rsx! {
                        div { class: "choose-mode",
                            h2 { "Get Started" }
                            p { class: "subtitle", "Create a new identity or import an existing one" }
                            
                            div { class: "mode-buttons",
                                button {
                                    class: "mode-btn create",
                                    onclick: move |_| { mode.set("create".to_string()); },
                                    div { class: "mode-icon", "✨" }
                                    span { "Create New Identity" }
                                    p { "Generate a new RSA keypair" }
                                }
                                button {
                                    class: "mode-btn import",
                                    onclick: move |_| { mode.set("import".to_string()); },
                                    div { class: "mode-icon", "📥" }
                                    span { "Import Existing" }
                                    p { "Use your existing private key" }
                                }
                            }
                        }
                    },
                    
                    "login" => rsx! {
                        div { class: "login-mode",
                            div { class: "user-avatar",
                                svg {
                                    width: "80",
                                    height: "80",
                                    "data-jdenticon-value": "{my_pubkey}"
                                }
                            }
                            h2 { "Welcome back!" }
                            p { class: "user-name", "{display_name}" }
                            p { class: "user-pubkey", 
                                onclick: move |_| { show_pubkey.set(!show_pubkey()); },
                                if show_pubkey() {
                                    "{my_pubkey().chars().take(40).collect::<String>()}..."
                                } else {
                                    "Click to show public key"
                                }
                            }
                            
                            div { class: "pin-section",
                                p { "Enter your PIN to unlock" }
                                div { class: "pin-display",
                                    for i in 0..6 {
                                        div { 
                                            class: if i < pin().len() { "pin-dot filled" } else { "pin-dot" }
                                        }
                                    }
                                }
                                
                                div { class: "keypad compact",
                                    for row in [["1", "2", "3"], ["4", "5", "6"], ["7", "8", "9"]] {
                                        div { class: "keypad-row",
                                            for digit in row {
                                                button {
                                                    class: "key-btn",
                                                    disabled: loading(),
                                                    onclick: move |_| {
                                                        if pin().len() < 6 { pin.write().push_str(digit); }
                                                    },
                                                    "{digit}"
                                                }
                                            }
                                        }
                                    }
                                    div { class: "keypad-row",
                                        button {
                                            class: "key-btn text-btn",
                                            onclick: move |_| { 
                                                mode.set("choose".to_string()); 
                                                let _ = document::eval("localStorage.removeItem('p2p_identity');");
                                            },
                                            "Reset"
                                        }
                                        button {
                                            class: "key-btn",
                                            disabled: loading(),
                                            onclick: move |_| { if pin().len() < 6 { pin.write().push_str("0"); } },
                                            "0"
                                        }
                                        button {
                                            class: "key-btn",
                                            disabled: loading(),
                                            onclick: move |_| { pin.write().pop(); },
                                            "⌫"
                                        }
                                    }
                                }
                            }
                            
                            if let Some(err) = error() {
                                div { class: "error-badge", "{err}" }
                            }
                            
                            button {
                                class: "unlock-btn",
                                disabled: pin().len() < 4 || loading(),
                                onclick: login_with_pin,
                                if loading() { "🔓 Unlocking..." } else { "🔓 Unlock" }
                            }
                        }
                    },
                    
                    "create" => rsx! {
                        div { class: "create-mode",
                            button { 
                                class: "back-btn",
                                onclick: move |_| { mode.set("choose".to_string()); },
                                "← Back"
                            }
                            
                            h2 { "Create New Identity" }
                            
                            div { class: "form-group",
                                label { "Display Name" }
                                input {
                                    r#type: "text",
                                    placeholder: "Your name or alias",
                                    value: "{display_name}",
                                    oninput: move |e| display_name.set(e.value())
                                }
                            }
                            
                            div { class: "form-group",
                                label { "Create PIN (min 4 digits)" }
                                input {
                                    r#type: "password",
                                    placeholder: "Enter PIN",
                                    maxlength: "6",
                                    value: "{pin}",
                                    oninput: move |e| pin.set(e.value())
                                }
                            }
                            
                            div { class: "form-group",
                                label { "Confirm PIN" }
                                input {
                                    r#type: "password",
                                    placeholder: "Confirm PIN",
                                    maxlength: "6",
                                    value: "{confirm_pin}",
                                    oninput: move |e| confirm_pin.set(e.value())
                                }
                            }
                            
                            if let Some(err) = error() {
                                div { class: "error-badge", "{err}" }
                            }
                            
                            button {
                                class: "create-btn",
                                disabled: loading(),
                                onclick: create_identity,
                                if loading() { "🔐 Generating keypair..." } else { "🔐 Create Identity" }
                            }
                            
                            p { class: "security-note",
                                "⚠️ Your private key will be encrypted with your PIN and stored locally. "
                                "Make sure to backup your key after creation!"
                            }
                        }
                    },
                    
                    "import" => rsx! {
                        div { class: "import-mode",
                            button { 
                                class: "back-btn",
                                onclick: move |_| { mode.set("choose".to_string()); },
                                "← Back"
                            }
                            
                            h2 { "Import Identity" }
                            
                            div { class: "form-group",
                                label { "Display Name" }
                                input {
                                    r#type: "text",
                                    placeholder: "Your name or alias",
                                    value: "{display_name}",
                                    oninput: move |e| display_name.set(e.value())
                                }
                            }
                            
                            div { class: "form-group",
                                label { "Private Key (Base64 PKCS8)" }
                                textarea {
                                    placeholder: "Paste your private key here...",
                                    value: "{import_privkey}",
                                    oninput: move |e| import_privkey.set(e.value())
                                }
                            }
                            
                            div { class: "form-group",
                                label { "Create PIN (min 4 digits)" }
                                input {
                                    r#type: "password",
                                    placeholder: "Enter PIN",
                                    maxlength: "6",
                                    value: "{pin}",
                                    oninput: move |e| pin.set(e.value())
                                }
                            }
                            
                            div { class: "form-group",
                                label { "Confirm PIN" }
                                input {
                                    r#type: "password",
                                    placeholder: "Confirm PIN",
                                    maxlength: "6",
                                    value: "{confirm_pin}",
                                    oninput: move |e| confirm_pin.set(e.value())
                                }
                            }
                            
                            if let Some(err) = error() {
                                div { class: "error-badge", "{err}" }
                            }
                            
                            button {
                                class: "import-btn",
                                disabled: loading(),
                                onclick: import_identity,
                                if loading() { "📥 Importing..." } else { "📥 Import Identity" }
                            }
                        }
                    },
                    
                    _ => rsx! { div { "Unknown mode" } }
                }
            }
        }
    }
}
