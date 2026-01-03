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

export default api;
