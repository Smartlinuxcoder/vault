# Arsonnet 🔥

A decentralized, end-to-end encrypted file vault and P2P chat application.

## Architecture

- **Backend**: Rust + Axum (REST API + WebSocket + Static file serving)
- **Frontend**: SvelteKit + Tailwind CSS v4 (built as static SPA)

## Features

- 🔐 **Identity Management**: RSA keypair generation, PIN-protected private key storage
- 📁 **Encrypted Vault**: Client-side AES-256-GCM encryption for files
- 🖼️ **Media Previews**: Encrypted thumbnails for photos and videos
- 💬 **P2P Chat**: Real-time messaging via WebSocket
- 🌐 **Network View**: See online peers and node info

## Quick Start

### Build and Run (Single Server)

```bash
# Build the frontend
cd frontend
npm install
npm run build

# Start the backend (serves everything on port 3000)
cd ../backend
cargo run
```

Open `http://localhost:3000` in your browser.

### Development Mode

**Terminal 1 - Backend:**
```bash
cd backend
cargo run
```

**Terminal 2 - Frontend (with hot-reload):**
```bash
cd frontend
npm run dev
```

Open `http://localhost:5173` (Vite proxies API calls to the backend).

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `STATIC_DIR` | `../frontend/build` | Path to static files |
| `RUST_LOG` | `info` | Log level |

## Project Structure

```
vault/
├── backend/
│   └── src/
│       ├── main.rs      # Axum server + static serving
│       ├── types.rs     # Shared types
│       └── crypto.rs    # RSA signing/verification
├── frontend/
│   └── src/
│       ├── lib/api.js   # API utilities (auto-detects host)
│       └── routes/
│           ├── +page.svelte         # Identity/Login
│           ├── network/+page.svelte # Network view
│           ├── vault/+page.svelte   # File vault
│           └── chat/+page.svelte    # P2P Chat
├── vault_data/          # Encrypted file storage
└── config/              # Node configuration
```

## Security

- All encryption happens **client-side** using Web Crypto API
- Files encrypted with AES-256-GCM before upload
- Private keys encrypted with user PIN
- Server never sees unencrypted data

## License

MIT
