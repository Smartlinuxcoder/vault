// API utilities - automatically uses current host
const getBaseUrl = () => {
	if (typeof window === 'undefined') return '';
	return window.location.origin;
};

const getWsUrl = () => {
	if (typeof window === 'undefined') return '';
	const protocol = window.location.protocol === 'https:' ? 'wss:' : 'ws:';
	return `${protocol}//${window.location.host}`;
};

export const api = {
	get baseUrl() { return getBaseUrl(); },
	get wsUrl() { return getWsUrl(); },
	
	async fetch(path, options = {}) {
		return fetch(`${getBaseUrl()}${path}`, options);
	},
	
	async post(path, body, options = {}) {
		return fetch(`${getBaseUrl()}${path}`, {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', ...options.headers },
			body: typeof body === 'string' ? body : JSON.stringify(body),
			...options
		});
	},
	
	async postRaw(path, body, options = {}) {
		return fetch(`${getBaseUrl()}${path}`, {
			method: 'POST',
			body,
			...options
		});
	},
	
	createWebSocket(path) {
		return new WebSocket(`${getWsUrl()}${path}`);
	}
};

// P2P Network API
export const p2pApi = {
	// Get current node info
	async getNodeInfo() {
		const res = await api.fetch('/p2p/info');
		return res.json();
	},

	// Get online peers (connected via WebSocket)
	async getOnlinePeers() {
		const res = await api.fetch('/p2p/peers');
		return res.json();
	},

	// Get configured peers from node.json
	async getConfiguredPeers() {
		const res = await api.fetch('/p2p/configured_peers');
		return res.json();
	},

	// Get discovered peers via discovery protocol
	async getKnownPeers() {
		const res = await api.fetch('/p2p/known_peers');
		return res.json();
	},

	// Get discovery info (x25519 pubkey, etc.)
	async getDiscoveryInfo() {
		const res = await api.fetch('/p2p/discovery');
		return res.json();
	},

	// Send message via onion routing
	async sendOnionMessage(hops, payload, messageType = 'chat') {
		const res = await api.post('/p2p/onion/send', {
			hops,
			payload: btoa(String.fromCharCode(...new Uint8Array(
				typeof payload === 'string' ? new TextEncoder().encode(payload) : payload
			))),
			message_type: messageType
		});
		return res.json();
	},

	// Relay a message to another node
	async relayMessage(toPubkey, message) {
		const res = await api.post('/p2p/relay', {
			to_pubkey: toPubkey,
			message
		});
		return res.json();
	}
};

export default api;
