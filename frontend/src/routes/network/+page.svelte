<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';

	let myPubkey = $state('');
	let myName = $state('');
	let onlinePeers = $state([]);
	let nodeInfo = $state(null);
	let connected = $state(false);
	let showMyKey = $state(false);
	let showExport = $state(false);
	let exportPrivkey = $state('');

	onMount(async () => {
		const pubkey = sessionStorage.getItem('p2p_pubkey');
		const name = sessionStorage.getItem('p2p_name');
		const privkey = sessionStorage.getItem('p2p_privkey');

		if (!pubkey || !privkey) {
			goto('/');
			return;
		}

		myPubkey = pubkey;
		myName = name || 'Anonymous';
		exportPrivkey = privkey;

		// Fetch node info
		try {
			const res = await api.fetch('/p2p/info');
			nodeInfo = await res.json();
		} catch (e) {
			console.error('Failed to fetch node info', e);
		}

		// Connect WebSocket
		connectWebSocket();
		updateJdenticon();
	});

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (onlinePeers.length >= 0) updateJdenticon();
	});

	function connectWebSocket() {
		if (window.p2pSocket) {
			try { window.p2pSocket.close(); } catch(e) {}
		}

		const ws = api.createWebSocket('/p2p/ws');
		window.p2pSocket = ws;

		ws.onopen = () => {
			console.log('[P2P Network] WebSocket opened');
			ws.send(JSON.stringify({
				Register: { pubkey: myPubkey, signature: 'dev_mode' }
			}));
		};

		ws.onmessage = (event) => {
			try {
				const data = JSON.parse(event.data);
				console.log('[P2P Network] Received:', data);

				if (data.Registered?.success) {
					connected = true;
					ws.send(JSON.stringify({ ListPeers: null }));
				} else if (data.PeerList) {
					onlinePeers = (data.PeerList.peers || []).map(p => ({
						pubkey: p.pubkey,
						name: p.name || null
					}));
				} else if (data.PeerStatus) {
					const { pubkey, online } = data.PeerStatus;
					if (online) {
						if (!onlinePeers.find(p => p.pubkey === pubkey)) {
							onlinePeers = [...onlinePeers, { pubkey, name: null }];
						}
					} else {
						onlinePeers = onlinePeers.filter(p => p.pubkey !== pubkey);
					}
				}
			} catch(e) {}
		};

		ws.onclose = () => {
			console.log('[P2P Network] WebSocket closed');
			connected = false;
		};

		// Keep alive
		setInterval(() => {
			if (ws.readyState === WebSocket.OPEN) {
				ws.send(JSON.stringify({ Ping: null }));
			}
		}, 30000);
	}

	function logout() {
		sessionStorage.clear();
		goto('/');
	}

	function copyPubkey() {
		navigator.clipboard.writeText(myPubkey);
	}

	function copyPrivkey() {
		navigator.clipboard.writeText(exportPrivkey);
	}

	function shortKey(key) {
		if (key && key.length > 16) {
			return key.slice(0, 8) + '...' + key.slice(-8);
		}
		return key || '';
	}
</script>

<div class="min-h-screen bg-gradient-to-br from-gray-900 via-orange-950 to-gray-900">
	<!-- Header -->
	<header class="bg-gray-800/50 backdrop-blur-xl border-b border-gray-700 px-6 py-4">
		<div class="max-w-6xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-4">
				<span class="text-3xl">🔥</span>
				<div>
					<h1 class="text-xl font-bold">Arsonnet Network</h1>
					<div class="flex items-center gap-2 text-sm">
						<span class="w-2 h-2 rounded-sm {connected ? 'bg-green-500' : 'bg-red-500'}"></span>
						<span class="text-gray-400">{connected ? 'Connected' : 'Disconnected'}</span>
					</div>
				</div>
			</div>
			<div class="flex gap-2">
				<button onclick={() => goto('/chat')} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 rounded-lg transition">💬 Chat</button>
				<button onclick={() => goto('/vault')} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">📁 Vault</button>
				<button onclick={logout} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">🔒 Lock</button>
			</div>
		</div>
	</header>

	<main class="max-w-6xl mx-auto p-6 space-y-6">
		<!-- My Identity Card -->
		<section class="bg-gray-800/50 backdrop-blur-xl border border-gray-700 rounded-lg p-6">
			<div class="flex items-center justify-between mb-4">
				<h2 class="text-lg font-semibold">👤 My Identity</h2>
				<button onclick={() => showExport = !showExport} class="px-3 py-1 text-sm bg-gray-700 hover:bg-gray-600 rounded-lg transition">
					{showExport ? 'Hide Key' : 'Export Key'}
				</button>
			</div>

			<div class="flex items-center gap-6">
				<div class="w-20 h-20 rounded-lg bg-gray-700 overflow-hidden">
					<svg width="80" height="80" data-jdenticon-value={myPubkey}></svg>
				</div>
				<div class="flex-1">
					<h3 class="text-xl font-semibold">{myName}</h3>
					<div class="flex items-center gap-2 mt-2">
						<button onclick={() => showMyKey = !showMyKey} class="font-mono text-sm text-gray-400 hover:text-gray-200 transition">
							{showMyKey ? myPubkey.slice(0, 60) + '...' : shortKey(myPubkey)}
						</button>
						<button onclick={copyPubkey} class="text-gray-400 hover:text-white transition" title="Copy public key">📋</button>
					</div>
				</div>
			</div>

			{#if showExport}
				<div class="mt-6 p-4 bg-red-900/20 border border-red-800 rounded-lg">
					<p class="text-red-300 text-sm mb-3">⚠️ Keep your private key secret! Anyone with this key can impersonate you.</p>
					<div class="flex items-center gap-2">
						<code class="flex-1 text-xs text-gray-400 break-all">{exportPrivkey.slice(0, 80)}...</code>
						<button onclick={copyPrivkey} class="px-4 py-2 bg-red-700 hover:bg-red-600 rounded-lg text-sm transition">📋 Copy Full Key</button>
					</div>
				</div>
			{/if}
		</section>

		<!-- Node Info Card -->
		{#if nodeInfo}
			<section class="bg-gray-800/50 backdrop-blur-xl border border-gray-700 rounded-lg p-6">
				<h2 class="text-lg font-semibold mb-4">🖥️ Server Node</h2>
				<div class="grid grid-cols-2 md:grid-cols-4 gap-4">
					<div>
						<span class="text-gray-400 text-sm">Name</span>
						<p class="font-medium">{nodeInfo.node?.name || 'Unknown'}</p>
					</div>
					<div>
						<span class="text-gray-400 text-sm">Address</span>
						<p class="font-medium">{nodeInfo.node?.address}:{nodeInfo.node?.public_port}</p>
					</div>
					<div>
						<span class="text-gray-400 text-sm">Version</span>
						<p class="font-medium">{nodeInfo.node?.version || '?'}</p>
					</div>
					<div>
						<span class="text-gray-400 text-sm">Secure</span>
						<p class="font-medium">{nodeInfo.node?.secure ? '🔒 Yes' : '🔓 No'}</p>
					</div>
				</div>
				<div class="mt-4">
					<span class="text-gray-400 text-sm">Node Public Key</span>
					<code class="block text-xs text-gray-400 mt-1">{shortKey(nodeInfo.node?.pubkey)}</code>
				</div>
			</section>
		{/if}

		<!-- Online Peers -->
		<section class="bg-gray-800/50 backdrop-blur-xl border border-gray-700 rounded-lg p-6">
			<div class="flex items-center justify-between mb-4">
				<h2 class="text-lg font-semibold">👥 Online Peers</h2>
				<span class="text-sm text-gray-400">{onlinePeers.length} online</span>
			</div>

			{#if onlinePeers.length === 0}
				<div class="text-center py-12">
					<div class="text-4xl mb-4">🔍</div>
					<p class="text-gray-400">No other peers online</p>
					<p class="text-gray-500 text-sm mt-2">Share your public key to connect with others!</p>
				</div>
			{:else}
				<div class="space-y-3">
					{#each onlinePeers as peer}
						<div class="flex items-center gap-4 p-4 bg-gray-700/50 rounded-lg">
							<div class="relative">
								<span class="absolute -top-1 -right-1 w-3 h-3 bg-green-500 rounded-sm border-2 border-gray-800"></span>
								<div class="w-10 h-10 rounded-lg bg-gray-600 overflow-hidden">
									<svg width="40" height="40" data-jdenticon-value={peer.pubkey}></svg>
								</div>
							</div>
							<div class="flex-1">
								<code class="text-sm text-gray-300">{shortKey(peer.pubkey)}</code>
								<p class="text-xs text-green-400">Online</p>
							</div>
							<div class="flex gap-2">
								<button onclick={() => navigator.clipboard.writeText(peer.pubkey)} class="p-2 bg-gray-600 hover:bg-gray-500 rounded-lg transition" title="Copy">📋</button>
								<button onclick={() => goto('/chat')} class="p-2 bg-orange-600 hover:bg-orange-500 rounded-lg transition" title="Chat">💬</button>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</section>
	</main>
</div>
