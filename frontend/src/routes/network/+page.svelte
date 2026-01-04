<script>
	import { onMount, onDestroy } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, p2pApi } from '$lib/api.js';
	import { 
		Flame, Globe, MessageSquare, FolderLock, Lock, Copy, KeyRound, Server, Users, 
		Eye, EyeOff, AlertTriangle, Network, Wifi, WifiOff, Shield, Zap, RefreshCw,
		ChevronDown, ChevronRight, Activity, Clock, Star, Radio, Layers, BookUser
	} from 'lucide-svelte';

	let myPubkey = $state('');
	let myName = $state('');
	let onlinePeers = $state([]);
	let nodeInfo = $state(null);
	let discoveryInfo = $state(null);
	let knownPeers = $state([]);
	let configuredPeers = $state([]);
	let connected = $state(false);
	let showMyKey = $state(false);
	let showExport = $state(false);
	let exportPrivkey = $state('');
	let activeTab = $state('tree'); // 'tree', 'discovered', 'online'
	let refreshing = $state(false);
	let expandedNodes = $state(new Set(['local']));
	let ws = $state(null);

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

		await refreshAll();
		connectWebSocket();
		updateJdenticon();

		// Auto-refresh ogni 30 secondi
		const interval = setInterval(refreshAll, 30000);
		return () => clearInterval(interval);
	});

	onDestroy(() => {
		if (ws) {
			try { ws.close(); } catch(e) {}
		}
	});

	async function refreshAll() {
		refreshing = true;
		try {
			const [nodeRes, discRes, knownRes, configRes] = await Promise.all([
				p2pApi.getNodeInfo().catch(() => null),
				p2pApi.getDiscoveryInfo().catch(() => null),
				p2pApi.getKnownPeers().catch(() => []),
				p2pApi.getConfiguredPeers().catch(() => [])
			]);
			
			nodeInfo = nodeRes;
			discoveryInfo = discRes;
			knownPeers = knownRes || [];
			configuredPeers = configRes || [];
		} catch (e) {
			console.error('Failed to refresh network data', e);
		}
		refreshing = false;
		updateJdenticon();
	}

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (onlinePeers.length >= 0 || knownPeers.length >= 0) updateJdenticon();
	});

	function connectWebSocket() {
		if (window.p2pSocket) {
			try { window.p2pSocket.close(); } catch(e) {}
		}

		ws = api.createWebSocket('/p2p/ws');
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

				if (data.Registered?.success) {
					connected = true;
					ws.send(JSON.stringify({ ListPeers: null }));
				} else if (data.PeerList) {
					onlinePeers = (data.PeerList.peers || []).map(p => ({
						pubkey: p.pubkey,
						name: p.name || null,
						http_port: p.http_port,
						arson_port: p.arson_port,
						protocols: p.protocols || [],
						is_connected: p.is_connected
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
				updateJdenticon();
			} catch(e) {}
		};

		ws.onclose = () => {
			connected = false;
			// Reconnect after 5 seconds
			setTimeout(connectWebSocket, 5000);
		};

		// Keep alive
		setInterval(() => {
			if (ws && ws.readyState === WebSocket.OPEN) {
				ws.send(JSON.stringify({ Ping: null }));
			}
		}, 30000);
	}

	function logout() {
		sessionStorage.clear();
		goto('/');
	}

	function copyText(text) {
		navigator.clipboard.writeText(text);
	}

	function shortKey(key) {
		if (key && key.length > 16) {
			return key.slice(0, 8) + '...' + key.slice(-8);
		}
		return key || '';
	}

	function toggleExpand(nodeId) {
		const newSet = new Set(expandedNodes);
		if (newSet.has(nodeId)) {
			newSet.delete(nodeId);
		} else {
			newSet.add(nodeId);
		}
		expandedNodes = newSet;
	}

	function getProtocolIcon(protocol) {
		switch(protocol?.toLowerCase()) {
			case 'wss': return { icon: Shield, color: 'text-green-500' };
			case 'ws': return { icon: Wifi, color: 'text-blue-500' };
			case 'arson': return { icon: Zap, color: 'text-orange-500' };
			default: return { icon: Radio, color: 'text-zinc-500' };
		}
	}

	function getTrustColor(score) {
		if (score >= 80) return 'text-green-500';
		if (score >= 50) return 'text-yellow-500';
		if (score >= 20) return 'text-orange-500';
		return 'text-red-500';
	}

	function formatLatency(ms) {
		if (!ms) return '—';
		if (ms < 100) return `${ms}ms`;
		if (ms < 1000) return `${ms}ms`;
		return `${(ms/1000).toFixed(1)}s`;
	}
</script>

<div class="min-h-screen bg-zinc-950">
	<!-- Header -->
	<header class="border-b border-zinc-800 px-6 py-3">
		<div class="max-w-6xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-3">
				<Flame class="w-6 h-6 text-orange-500" />
				<div>
					<h1 class="text-sm font-semibold">Arsonnet</h1>
					<div class="flex items-center gap-1.5 text-xs">
						<span class="w-1.5 h-1.5 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'}"></span>
						<span class="text-zinc-400">{connected ? 'Connected' : 'Disconnected'}</span>
						{#if knownPeers.length > 0}
							<span class="text-zinc-600">•</span>
							<span class="text-zinc-500">{knownPeers.length} peers discovered</span>
						{/if}
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
				<button onclick={() => goto('/contacts')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Contacts">
					<BookUser class="w-4 h-4" />
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

	<main class="max-w-6xl mx-auto p-6 space-y-6">
		<!-- Stats Bar -->
		<div class="grid grid-cols-2 md:grid-cols-4 gap-4">
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
				<div class="flex items-center gap-2 text-zinc-500 text-xs mb-1">
					<Server class="w-3.5 h-3.5" />
					This Node
				</div>
				<p class="text-lg font-semibold">{nodeInfo?.node?.name || 'Loading...'}</p>
			</div>
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
				<div class="flex items-center gap-2 text-zinc-500 text-xs mb-1">
					<Users class="w-3.5 h-3.5" />
					Online Peers
				</div>
				<p class="text-lg font-semibold text-green-500">{onlinePeers.filter(p => p.is_connected !== false).length}</p>
			</div>
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
				<div class="flex items-center gap-2 text-zinc-500 text-xs mb-1">
					<Network class="w-3.5 h-3.5" />
					Discovered
				</div>
				<p class="text-lg font-semibold text-blue-500">{knownPeers.length}</p>
			</div>
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4">
				<div class="flex items-center gap-2 text-zinc-500 text-xs mb-1">
					<Layers class="w-3.5 h-3.5" />
					Configured
				</div>
				<p class="text-lg font-semibold text-orange-500">{configuredPeers.length}</p>
			</div>
		</div>

		<!-- My Identity Card (Collapsible) -->
		<section class="bg-zinc-900 border border-zinc-800 rounded-lg">
			<button onclick={() => toggleExpand('identity')} class="w-full p-4 flex items-center justify-between text-left">
				<h2 class="text-sm font-medium flex items-center gap-2">
					<KeyRound class="w-4 h-4 text-zinc-400" />
					My Identity
				</h2>
				{#if expandedNodes.has('identity')}
					<ChevronDown class="w-4 h-4 text-zinc-500" />
				{:else}
					<ChevronRight class="w-4 h-4 text-zinc-500" />
				{/if}
			</button>
			
			{#if expandedNodes.has('identity')}
				<div class="px-4 pb-4 border-t border-zinc-800 pt-4">
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
								<button onclick={() => copyText(myPubkey)} class="text-zinc-500 hover:text-zinc-300 transition-colors shrink-0" title="Copy">
									<Copy class="w-3.5 h-3.5" />
								</button>
							</div>
						</div>
						<button onclick={() => showExport = !showExport} class="px-2.5 py-1 text-xs bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md transition-colors flex items-center gap-1">
							{#if showExport}
								<EyeOff class="w-3 h-3" />
								Hide
							{:else}
								<Eye class="w-3 h-3" />
								Export
							{/if}
						</button>
					</div>

					{#if showExport}
						<div class="mt-4 p-3 bg-red-950/50 border border-red-900 rounded-md">
							<p class="text-xs text-red-400 mb-2 flex items-center gap-1">
								<AlertTriangle class="w-3 h-3" />
								Keep your private key secret!
							</p>
							<div class="flex items-center gap-2">
								<code class="flex-1 text-xs text-zinc-500 break-all truncate">{exportPrivkey.slice(0, 64)}...</code>
								<button onclick={() => copyText(exportPrivkey)} class="px-2.5 py-1 text-xs bg-red-900 hover:bg-red-800 border border-red-800 rounded-md transition-colors shrink-0">
									<Copy class="w-3 h-3" />
								</button>
							</div>
						</div>
					{/if}
				</div>
			{/if}
		</section>

		<!-- Tabs -->
		<div class="flex items-center gap-2 border-b border-zinc-800 pb-2">
			<button 
				onclick={() => activeTab = 'tree'} 
				class="px-3 py-1.5 text-sm rounded-md transition-colors {activeTab === 'tree' ? 'bg-zinc-800 text-zinc-100' : 'text-zinc-400 hover:text-zinc-200'}"
			>
				<Network class="w-4 h-4 inline mr-1.5" />
				Network Tree
			</button>
			<button 
				onclick={() => activeTab = 'discovered'} 
				class="px-3 py-1.5 text-sm rounded-md transition-colors {activeTab === 'discovered' ? 'bg-zinc-800 text-zinc-100' : 'text-zinc-400 hover:text-zinc-200'}"
			>
				<Radio class="w-4 h-4 inline mr-1.5" />
				Discovered ({knownPeers.length})
			</button>
			<button 
				onclick={() => activeTab = 'online'} 
				class="px-3 py-1.5 text-sm rounded-md transition-colors {activeTab === 'online' ? 'bg-zinc-800 text-zinc-100' : 'text-zinc-400 hover:text-zinc-200'}"
			>
				<Users class="w-4 h-4 inline mr-1.5" />
				Online ({onlinePeers.length})
			</button>
			<div class="flex-1"></div>
			<button 
				onclick={refreshAll} 
				class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors"
				disabled={refreshing}
			>
				<RefreshCw class="w-4 h-4 {refreshing ? 'animate-spin' : ''}" />
			</button>
		</div>

		<!-- Network Tree View -->
		{#if activeTab === 'tree'}
			<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
				<!-- Local Node -->
				<div class="space-y-2">
					<button onclick={() => toggleExpand('local')} class="w-full flex items-center gap-2 p-3 bg-zinc-800/50 border border-orange-500/30 rounded-md hover:bg-zinc-800 transition-colors">
						{#if expandedNodes.has('local')}
							<ChevronDown class="w-4 h-4 text-zinc-500" />
						{:else}
							<ChevronRight class="w-4 h-4 text-zinc-500" />
						{/if}
						<div class="w-8 h-8 rounded-md bg-orange-500/20 flex items-center justify-center">
							<Server class="w-4 h-4 text-orange-500" />
						</div>
						<div class="flex-1 text-left">
							<div class="flex items-center gap-2">
								<span class="text-sm font-medium">{nodeInfo?.node?.name || 'This Node'}</span>
								<span class="px-1.5 py-0.5 text-[10px] bg-orange-500/20 text-orange-400 rounded">LOCAL</span>
							</div>
							<div class="flex items-center gap-3 text-xs text-zinc-500 mt-0.5">
								<span>{nodeInfo?.node?.address}</span>
								{#if nodeInfo?.node?.http_port}
									<span class="flex items-center gap-1">
										<Wifi class="w-3 h-3" />
										:{nodeInfo.node.http_port}
									</span>
								{/if}
								{#if nodeInfo?.node?.arson_port}
									<span class="flex items-center gap-1">
										<Zap class="w-3 h-3 text-orange-500" />
										:{nodeInfo.node.arson_port}
									</span>
								{/if}
							</div>
						</div>
						<span class="w-2 h-2 bg-green-500 rounded-full"></span>
					</button>

					{#if expandedNodes.has('local')}
						<div class="ml-6 pl-4 border-l border-zinc-700 space-y-2">
							<!-- X25519 Key -->
							{#if discoveryInfo?.x25519_pubkey}
								<div class="p-3 bg-zinc-800/30 border border-zinc-800 rounded-md">
									<div class="flex items-center gap-2 text-xs text-zinc-500 mb-1">
										<Shield class="w-3 h-3" />
										X25519 Public Key (Onion Routing)
									</div>
									<code class="text-xs text-zinc-400 font-mono">{discoveryInfo.x25519_pubkey.slice(0, 32)}...</code>
								</div>
							{/if}

							<!-- Configured Peers -->
							{#if configuredPeers.length > 0}
								<div class="text-xs text-zinc-500 mt-3 mb-2 flex items-center gap-2">
									<Layers class="w-3 h-3" />
									Configured Peers ({configuredPeers.length})
								</div>
								{#each configuredPeers as peer}
									{@const isOnline = onlinePeers.some(p => p.pubkey === peer.pubkey && p.is_connected !== false)}
									<div class="flex items-center gap-3 p-3 bg-zinc-800/30 border border-zinc-800 rounded-md">
										<div class="relative">
											<span class="absolute -top-0.5 -right-0.5 w-2 h-2 {isOnline ? 'bg-green-500' : 'bg-zinc-600'} rounded-full border border-zinc-900"></span>
											<div class="w-8 h-8 rounded-md bg-zinc-700 overflow-hidden">
												<svg width="32" height="32" data-jdenticon-value={peer.pubkey}></svg>
											</div>
										</div>
										<div class="flex-1 min-w-0">
											<div class="flex items-center gap-2">
												<span class="text-sm font-medium">{peer.name || 'Peer'}</span>
												{#each peer.protocols || [] as proto}
													{@const { icon: ProtoIcon, color } = getProtocolIcon(proto)}
													<span class="{color}" title={proto}>
														<svelte:component this={ProtoIcon} class="w-3 h-3" />
													</span>
												{/each}
											</div>
											<div class="flex items-center gap-2 text-xs text-zinc-500">
												<span>{peer.address}</span>
												{#if peer.http_port}
													<span>:{peer.http_port}</span>
												{/if}
												{#if peer.arson_port}
													<span class="text-orange-500">:{peer.arson_port}</span>
												{/if}
											</div>
										</div>
										<div class="flex gap-1">
											<button onclick={() => copyText(peer.pubkey)} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md" title="Copy pubkey">
												<Copy class="w-3.5 h-3.5" />
											</button>
											<button onclick={() => goto('/chat')} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md" title="Chat">
												<MessageSquare class="w-3.5 h-3.5" />
											</button>
										</div>
									</div>
								{/each}
							{:else}
								<div class="p-4 text-center text-zinc-500 text-sm">
									No configured peers. Add peers in node.json config.
								</div>
							{/if}
						</div>
					{/if}
				</div>
			</section>
		{/if}

		<!-- Discovered Peers View -->
		{#if activeTab === 'discovered'}
			<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
				<div class="flex items-center justify-between mb-4">
					<h2 class="text-sm font-medium flex items-center gap-2">
						<Radio class="w-4 h-4 text-zinc-400" />
						Discovered Peers
					</h2>
					<span class="text-xs text-zinc-500">via P2P Discovery Protocol</span>
				</div>

				{#if knownPeers.length === 0}
					<div class="text-center py-8">
						<Radio class="w-8 h-8 text-zinc-600 mx-auto mb-2" />
						<p class="text-sm text-zinc-400">No peers discovered yet</p>
						<p class="text-xs text-zinc-500 mt-1">Peers will appear as they announce themselves on the network</p>
					</div>
				{:else}
					<div class="space-y-2">
						{#each knownPeers as peer}
							<div class="flex items-center gap-3 p-3 bg-zinc-800/50 border border-zinc-800 rounded-md hover:bg-zinc-800/80 transition-colors">
								<div class="w-10 h-10 rounded-md bg-zinc-700 overflow-hidden">
									<svg width="40" height="40" data-jdenticon-value={peer.node?.pubkey}></svg>
								</div>
								<div class="flex-1 min-w-0">
									<div class="flex items-center gap-2">
										<span class="text-sm font-medium">{peer.node?.name || 'Unknown'}</span>
										{#if peer.x25519_pubkey}
											<span class="text-green-500" title="Onion routing enabled">
												<Shield class="w-3 h-3" />
											</span>
										{/if}
									</div>
									<div class="flex items-center gap-3 text-xs text-zinc-500 mt-0.5">
										<span>{peer.node?.address}</span>
										{#if peer.node?.http_port}
											<span class="flex items-center gap-1">
												<Wifi class="w-3 h-3" />
												:{peer.node.http_port}
											</span>
										{/if}
										{#if peer.node?.arson_port}
											<span class="flex items-center gap-1 text-orange-500">
												<Zap class="w-3 h-3" />
												:{peer.node.arson_port}
											</span>
										{/if}
									</div>
								</div>
								<div class="flex flex-col items-end gap-1 text-xs">
									<div class="flex items-center gap-1 {getTrustColor(peer.trust_score)}">
										<Star class="w-3 h-3" />
										<span>{peer.trust_score || 0}</span>
									</div>
									{#if peer.latency_ms}
										<div class="flex items-center gap-1 text-zinc-500">
											<Activity class="w-3 h-3" />
											<span>{formatLatency(peer.latency_ms)}</span>
										</div>
									{/if}
								</div>
								<div class="flex gap-1">
									<button onclick={() => copyText(peer.node?.pubkey)} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md" title="Copy pubkey">
										<Copy class="w-3.5 h-3.5" />
									</button>
									<button onclick={() => goto('/chat')} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md" title="Chat">
										<MessageSquare class="w-3.5 h-3.5" />
									</button>
								</div>
							</div>
						{/each}
					</div>
				{/if}
			</section>
		{/if}

		<!-- Online Peers View -->
		{#if activeTab === 'online'}
			<section class="bg-zinc-900 border border-zinc-800 rounded-lg p-5">
				<div class="flex items-center justify-between mb-4">
					<h2 class="text-sm font-medium flex items-center gap-2">
						<Users class="w-4 h-4 text-zinc-400" />
						Online Peers
					</h2>
					<span class="text-xs text-zinc-500">{onlinePeers.length} connected</span>
				</div>

				{#if onlinePeers.length === 0}
					<div class="text-center py-8">
						<Users class="w-8 h-8 text-zinc-600 mx-auto mb-2" />
						<p class="text-sm text-zinc-400">No other peers online</p>
						<p class="text-xs text-zinc-500 mt-1">Share your public key to connect with others</p>
					</div>
				{:else}
					<div class="space-y-2">
						{#each onlinePeers as peer}
							{@const isConnected = peer.is_connected !== false}
							<div class="flex items-center gap-3 p-3 bg-zinc-800/50 border border-zinc-800 rounded-md">
								<div class="relative">
									<span class="absolute -top-0.5 -right-0.5 w-2 h-2 {isConnected ? 'bg-green-500' : 'bg-yellow-500'} rounded-full border border-zinc-900"></span>
									<div class="w-8 h-8 rounded-md bg-zinc-700 overflow-hidden">
										<svg width="32" height="32" data-jdenticon-value={peer.pubkey}></svg>
									</div>
								</div>
								<div class="flex-1 min-w-0">
									<div class="flex items-center gap-2">
										<code class="text-xs text-zinc-300 font-mono">{shortKey(peer.pubkey)}</code>
										{#each peer.protocols || [] as proto}
											{@const { icon: ProtoIcon, color } = getProtocolIcon(proto)}
											<span class="{color}" title={proto}>
												<svelte:component this={ProtoIcon} class="w-3 h-3" />
											</span>
										{/each}
									</div>
									<p class="text-xs {isConnected ? 'text-green-500' : 'text-yellow-500'}">
										{isConnected ? 'Online' : 'Configured'}
									</p>
								</div>
								<div class="flex gap-1">
									<button onclick={() => copyText(peer.pubkey)} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-700 rounded-md transition-colors" title="Copy">
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
		{/if}

		<!-- Node Details Card -->
		{#if nodeInfo}
			<section class="bg-zinc-900 border border-zinc-800 rounded-lg">
				<button onclick={() => toggleExpand('nodeinfo')} class="w-full p-4 flex items-center justify-between text-left">
					<h2 class="text-sm font-medium flex items-center gap-2">
						<Server class="w-4 h-4 text-zinc-400" />
						Server Node Details
					</h2>
					{#if expandedNodes.has('nodeinfo')}
						<ChevronDown class="w-4 h-4 text-zinc-500" />
					{:else}
						<ChevronRight class="w-4 h-4 text-zinc-500" />
					{/if}
				</button>

				{#if expandedNodes.has('nodeinfo')}
					<div class="px-4 pb-4 border-t border-zinc-800 pt-4">
						<div class="grid grid-cols-2 md:grid-cols-4 gap-4">
							<div>
								<span class="text-xs text-zinc-500">Name</span>
								<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.name || 'Unknown'}</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">Address</span>
								<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.address}</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">HTTP Port</span>
								<p class="text-sm font-medium mt-0.5 flex items-center gap-1">
									<Wifi class="w-3 h-3 text-blue-500" />
									{nodeInfo.node?.http_port || '—'}
								</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">Arson Port</span>
								<p class="text-sm font-medium mt-0.5 flex items-center gap-1">
									<Zap class="w-3 h-3 text-orange-500" />
									{nodeInfo.node?.arson_port || '—'}
								</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">Version</span>
								<p class="text-sm font-medium mt-0.5">{nodeInfo.node?.version || '—'}</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">Secure (TLS)</span>
								<p class="text-sm font-medium mt-0.5 flex items-center gap-1">
									{#if nodeInfo.node?.secure}
										<Lock class="w-3 h-3 text-green-500" />
										Yes
									{:else}
										<WifiOff class="w-3 h-3 text-zinc-500" />
										No
									{/if}
								</p>
							</div>
							<div>
								<span class="text-xs text-zinc-500">Protocols</span>
								<div class="flex items-center gap-2 mt-0.5">
									{#each nodeInfo.node?.protocols || [] as proto}
										{@const { icon: ProtoIcon, color } = getProtocolIcon(proto)}
										<span class="flex items-center gap-1 text-xs {color}">
											<svelte:component this={ProtoIcon} class="w-3 h-3" />
											{proto}
										</span>
									{/each}
								</div>
							</div>
						</div>
						<div class="mt-4 pt-4 border-t border-zinc-800">
							<span class="text-xs text-zinc-500">Node Public Key</span>
							<div class="flex items-center gap-2 mt-1">
								<code class="flex-1 text-xs text-zinc-400 font-mono truncate">{nodeInfo.node?.pubkey}</code>
								<button onclick={() => copyText(nodeInfo.node?.pubkey)} class="p-1 text-zinc-500 hover:text-zinc-300">
									<Copy class="w-3.5 h-3.5" />
								</button>
							</div>
						</div>
					</div>
				{/if}
			</section>
		{/if}
	</main>
</div>
