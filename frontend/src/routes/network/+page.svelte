<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';
	import { Flame, Globe, MessageSquare, FolderLock, Lock, Copy, KeyRound, Server, Users, Eye, EyeOff, AlertTriangle } from 'lucide-svelte';

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

<div class="min-h-screen bg-zinc-950">
	<!-- Header -->
	<header class="border-b border-zinc-800 px-6 py-3">
		<div class="max-w-5xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-3">
				<Flame class="w-6 h-6 text-orange-500" />
				<div>
					<h1 class="text-sm font-semibold">Arsonnet</h1>
					<div class="flex items-center gap-1.5 text-xs">
						<span class="w-1.5 h-1.5 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'}"></span>
						<span class="text-zinc-400">{connected ? 'Connected' : 'Disconnected'}</span>
					</div>
				</div>
			</div>
			<nav class="flex gap-1">
				<button onclick={() => goto('/network')} class="p-2 text-sm bg-zinc-800 text-zinc-100 rounded-md" title="Network">
					<Globe class="w-4 h-4" />
				</button>
				<button onclick={() => goto('/chat')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Chat">
					<MessageSquare class="w-4 h-4" />
				</button>
				<button onclick={() => goto('/vault')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Vault">
					<FolderLock class="w-4 h-4" />
				</button>
				<button onclick={logout} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Lock">
					<Lock class="w-4 h-4" />
				</button>
			</nav>
		</div>
	</header>

	<main class="max-w-5xl mx-auto p-6 space-y-6">
		<!-- My Identity Card -->
		<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
			<div class="flex items-center justify-between mb-4">
				<h2 class="text-sm font-medium flex items-center gap-2">
					<KeyRound class="w-4 h-4 text-zinc-400" />
					My Identity
				</h2>
				<button onclick={() => showExport = !showExport} class="px-2.5 py-1 text-xs bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md transition-colors flex items-center gap-1">
					{#if showExport}
						<EyeOff class="w-3 h-3" />
						Hide Key
					{:else}
						<Eye class="w-3 h-3" />
						Export Key
					{/if}
				</button>
			</div>

			<div class="flex items-center gap-4">
				<div class="w-14 h-14 rounded-md bg-zinc-800 border border-zinc-700 overflow-hidden">
					<svg width="56" height="56" data-jdenticon-value={myPubkey}></svg>
				</div>
				<div class="flex-1 min-w-0">
					<h3 class="font-medium">{myName}</h3>
					<div class="flex items-center gap-2 mt-1">
						<button onclick={() => showMyKey = !showMyKey} class="font-mono text-xs text-zinc-500 hover:text-zinc-300 transition-colors truncate">
							{showMyKey ? myPubkey.slice(0, 48) + '...' : shortKey(myPubkey)}
						</button>
						<button onclick={copyPubkey} class="text-zinc-500 hover:text-zinc-300 transition-colors shrink-0" title="Copy">
							<Copy class="w-3.5 h-3.5" />
						</button>
					</div>
				</div>
			</div>

			{#if showExport}
				<div class="mt-4 p-3 bg-red-950/50 border border-red-900 rounded-md">
					<p class="text-xs text-red-400 mb-2 flex items-center gap-1">
						<AlertTriangle class="w-3 h-3" />
						Keep your private key secret. Anyone with this key can impersonate you.
					</p>
					<div class="flex items-center gap-2">
						<code class="flex-1 text-xs text-zinc-500 break-all truncate">{exportPrivkey.slice(0, 64)}...</code>
						<button onclick={copyPrivkey} class="px-2.5 py-1 text-xs bg-red-900 hover:bg-red-800 border border-red-800 rounded-md transition-colors shrink-0 flex items-center gap-1">
							<Copy class="w-3 h-3" />
							Copy Key
						</button>
					</div>
				</div>
			{/if}
		</section>

		<!-- Node Info Card -->
		{#if nodeInfo}
			<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
				<h2 class="text-sm font-medium mb-4 flex items-center gap-2">
					<Server class="w-4 h-4 text-zinc-400" />
					Server Node
				</h2>
				<div class="grid grid-cols-2 md:grid-cols-4 gap-4">
					<div>
						<span class="text-xs text-zinc-500">Name</span>
						<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.name || 'Unknown'}</p>
					</div>
					<div>
						<span class="text-xs text-zinc-500">Address</span>
						<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.address}:{nodeInfo.node?.public_port}</p>
					</div>
					<div>
						<span class="text-xs text-zinc-500">Version</span>
						<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.version || '—'}</p>
					</div>
					<div>
						<span class="text-xs text-zinc-500">Secure</span>
						<p class="text-sm font-medium mt-0.5 flex items-center gap-1">
							{#if nodeInfo.node?.secure}
								<Lock class="w-3 h-3 text-green-500" />
								Yes
							{:else}
								No
							{/if}
						</p>
					</div>
				</div>
				<div class="mt-3 pt-3 border-t border-zinc-800">
					<span class="text-xs text-zinc-500">Node Public Key</span>
					<code class="block text-xs text-zinc-400 mt-0.5 font-mono">{shortKey(nodeInfo.node?.pubkey)}</code>
				</div>
			</section>
		{/if}

		<!-- Online Peers -->
		<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
			<div class="flex items-center justify-between mb-4">
				<h2 class="text-sm font-medium flex items-center gap-2">
					<Users class="w-4 h-4 text-zinc-400" />
					Online Peers
				</h2>
				<span class="text-xs text-zinc-500">{onlinePeers.length} online</span>
			</div>

			{#if onlinePeers.length === 0}
				<div class="text-center py-8">
					<p class="text-sm text-zinc-400">No other peers online</p>
					<p class="text-xs text-zinc-500 mt-1">Share your public key to connect with others</p>
				</div>
			{:else}
				<div class="space-y-2">
					{#each onlinePeers as peer}
						<div class="flex items-center gap-3 p-3 bg-zinc-800/50 border border-zinc-800 rounded-md">
							<div class="relative">
								<span class="absolute -top-0.5 -right-0.5 w-2 h-2 bg-green-500 rounded-full border border-zinc-900"></span>
								<div class="w-8 h-8 rounded-md bg-zinc-700 overflow-hidden">
									<svg width="32" height="32" data-jdenticon-value={peer.pubkey}></svg>
								</div>
							</div>
							<div class="flex-1 min-w-0">
								<code class="text-xs text-zinc-300 font-mono">{shortKey(peer.pubkey)}</code>
								<p class="text-xs text-green-500">Online</p>
							</div>
							<div class="flex gap-1">
								<button onclick={() => navigator.clipboard.writeText(peer.pubkey)} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md transition-colors" title="Copy">
									<Copy class="w-3.5 h-3.5" />
								</button>
								<button onclick={() => goto('/chat')} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md transition-colors" title="Chat">
									<MessageSquare class="w-3.5 h-3.5" />
								</button>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</section>
	</main>
</div>
