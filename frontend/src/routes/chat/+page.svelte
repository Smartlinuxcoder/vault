<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';

	let contacts = $state([]);
	let onlinePeers = $state([]);
	let selectedContact = $state(null);
	let messages = $state({});
	let newMessage = $state('');
	let myPubkey = $state('');
	let myName = $state('');
	let connected = $state(false);
	let showAddContact = $state(false);
	let newContactPubkey = $state('');
	let newContactName = $state('');
	let error = $state(null);

	onMount(() => {
		const pubkey = sessionStorage.getItem('p2p_pubkey');
		const name = sessionStorage.getItem('p2p_name');

		if (!pubkey) {
			goto('/');
			return;
		}

		myPubkey = pubkey;
		myName = name || 'Anonymous';

		// Load saved data
		contacts = JSON.parse(localStorage.getItem('p2p_contacts') || '[]');
		messages = JSON.parse(localStorage.getItem('p2p_messages') || '{}');

		connectWebSocket();
		updateJdenticon();
	});

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (contacts.length >= 0 || onlinePeers.length >= 0) updateJdenticon();
	});

	function connectWebSocket() {
		if (window.p2pSocket) {
			try { window.p2pSocket.close(); } catch(e) {}
		}

		const ws = api.createWebSocket('/p2p/ws');
		window.p2pSocket = ws;

		ws.onopen = () => {
			console.log('[P2P Chat] WebSocket opened');
			ws.send(JSON.stringify({
				Register: { pubkey: myPubkey, signature: 'dev_mode' }
			}));
		};

		ws.onmessage = (event) => {
			try {
				const data = JSON.parse(event.data);
				console.log('[P2P Chat] Received:', data);

				if (data.Registered?.success) {
					connected = true;
					ws.send(JSON.stringify({ ListPeers: null }));
				} else if (data.PeerList) {
					onlinePeers = (data.PeerList.peers || []).map(p => p.pubkey);
				} else if (data.PeerStatus) {
					const { pubkey, online } = data.PeerStatus;
					if (online) {
						if (!onlinePeers.includes(pubkey)) {
							onlinePeers = [...onlinePeers, pubkey];
						}
					} else {
						onlinePeers = onlinePeers.filter(p => p !== pubkey);
					}
				} else if (data.IncomingMessage) {
					const payload = new Uint8Array(data.IncomingMessage.encrypted_payload);
					const text = new TextDecoder().decode(payload);
					const from = data.IncomingMessage.from_pubkey;
					const timestamp = data.IncomingMessage.timestamp || Math.floor(Date.now() / 1000);

					const newMsg = {
						id: `${from}-${timestamp}`,
						from_me: false,
						content: text,
						timestamp,
						delivered: true,
						read: selectedContact === from
					};

					if (!messages[from]) messages[from] = [];
					messages[from] = [...messages[from], newMsg];
					messages = { ...messages };
					saveMessages();
				} else if (data.Error) {
					error = data.Error.message;
				}
			} catch(e) {}
		};

		ws.onclose = () => {
			console.log('[P2P Chat] WebSocket closed');
			connected = false;
		};

		// Keep alive
		setInterval(() => {
			if (ws.readyState === WebSocket.OPEN) {
				ws.send(JSON.stringify({ Ping: null }));
			}
		}, 25000);
	}

	function sendMessage() {
		if (!newMessage.trim() || !selectedContact) return;

		const ws = window.p2pSocket;
		if (!ws || ws.readyState !== WebSocket.OPEN) {
			error = 'Not connected';
			return;
		}

		const timestamp = Math.floor(Date.now() / 1000);
		const payload = new TextEncoder().encode(newMessage);

		ws.send(JSON.stringify({
			SendMessage: {
				to_pubkey: selectedContact,
				encrypted_payload: Array.from(payload)
			}
		}));

		const newMsg = {
			id: `me-${timestamp}`,
			from_me: true,
			content: newMessage,
			timestamp,
			delivered: true,
			read: false
		};

		if (!messages[selectedContact]) messages[selectedContact] = [];
		messages[selectedContact] = [...messages[selectedContact], newMsg];
		messages = { ...messages };
		saveMessages();

		newMessage = '';
	}

	function addContact() {
		const pubkey = newContactPubkey.trim();
		const name = newContactName.trim();

		if (!pubkey || !name) return;

		if (pubkey === myPubkey) {
			error = 'Cannot add yourself';
			return;
		}

		if (contacts.find(c => c.pubkey === pubkey)) {
			error = 'Contact already exists';
			return;
		}

		const newContact = {
			pubkey,
			name,
			added_at: Math.floor(Date.now() / 1000),
			last_message: null
		};

		contacts = [...contacts, newContact];
		saveContacts();

		newContactPubkey = '';
		newContactName = '';
		showAddContact = false;
	}

	function selectContact(pubkey) {
		selectedContact = pubkey;
		// Mark messages as read
		if (messages[pubkey]) {
			messages[pubkey] = messages[pubkey].map(m => ({ ...m, read: true }));
			messages = { ...messages };
			saveMessages();
		}
	}

	function saveContacts() {
		localStorage.setItem('p2p_contacts', JSON.stringify(contacts));
	}

	function saveMessages() {
		localStorage.setItem('p2p_messages', JSON.stringify(messages));
	}

	function isOnline(pubkey) {
		return onlinePeers.includes(pubkey);
	}

	function getUnreadCount(pubkey) {
		const msgs = messages[pubkey] || [];
		return msgs.filter(m => !m.from_me && !m.read).length;
	}

	function formatTime(ts) {
		const now = Math.floor(Date.now() / 1000);
		const diff = now - ts;
		if (diff < 60) return 'Now';
		if (diff < 3600) return `${Math.floor(diff / 60)}m ago`;
		if (diff < 86400) return `${Math.floor(diff / 3600)}h ago`;
		return `${Math.floor(diff / 86400)}d ago`;
	}

	function shortKey(key) {
		if (key && key.length > 24) {
			return key.slice(0, 12) + '...' + key.slice(-8);
		}
		return key || '';
	}
</script>

<!-- Add Contact Modal -->
{#if showAddContact}
	<div class="fixed inset-0 bg-black/80 backdrop-blur-sm z-50 flex items-center justify-center p-4" onclick={() => showAddContact = false}>
		<div class="bg-gray-800 rounded-lg p-6 w-full max-w-md" onclick={(e) => e.stopPropagation()}>
			<h3 class="text-xl font-semibold mb-4">Add Contact</h3>
			
			<div class="space-y-4">
				<div>
					<label class="block text-sm text-gray-400 mb-1">Name</label>
					<input type="text" bind:value={newContactName} placeholder="Contact name" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
				</div>
				<div>
					<label class="block text-sm text-gray-400 mb-1">Public Key</label>
					<textarea bind:value={newContactPubkey} placeholder="Paste the contact's public key" rows="3" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none resize-none"></textarea>
				</div>
			</div>

			<div class="flex gap-3 mt-6">
				<button onclick={() => showAddContact = false} class="flex-1 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">Cancel</button>
				<button onclick={addContact} disabled={!newContactName.trim() || !newContactPubkey.trim()} class="flex-1 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded-lg transition">Add</button>
			</div>
		</div>
	</div>
{/if}

<!-- Error Toast -->
{#if error}
	<div class="fixed bottom-4 right-4 bg-red-600 text-white px-4 py-3 rounded-lg flex items-center gap-3 z-50">
		<span>{error}</span>
		<button onclick={() => error = null}>✕</button>
	</div>
{/if}

<div class="h-screen flex flex-col bg-gray-900">
	<!-- Top Navbar -->
	<header class="bg-gray-800/50 backdrop-blur-xl border-b border-gray-700 px-6 py-3 flex items-center justify-between shrink-0">
		<div class="flex items-center gap-4">
			<span class="text-2xl">🔥</span>
			<h1 class="text-lg font-bold">Arsonnet Chat</h1>
			<div class="flex items-center gap-2 text-sm">
				<span class="w-2 h-2 rounded-sm {connected ? 'bg-green-500' : 'bg-red-500'}"></span>
				<span class="text-gray-400">{connected ? 'Online' : 'Offline'}</span>
			</div>
		</div>
		<div class="flex gap-2">
			<button onclick={() => goto('/network')} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition text-sm">🌐 Network</button>
			<button onclick={() => goto('/vault')} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition text-sm">📁 Vault</button>
			<button onclick={() => { sessionStorage.clear(); goto('/'); }} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition text-sm">🔒 Lock</button>
		</div>
	</header>

	<div class="flex flex-1 overflow-hidden">
		<!-- Sidebar -->
		<aside class="w-72 bg-gray-800/50 border-r border-gray-700 flex flex-col shrink-0">
			<div class="p-4 border-b border-gray-700">
				<p class="text-sm text-gray-400">👤 {myName}</p>
				<div class="flex items-center gap-2 mt-1">
					<code class="text-xs text-gray-500 truncate flex-1">{myPubkey.slice(0, 20)}...</code>
					<button onclick={() => navigator.clipboard.writeText(myPubkey)} class="text-gray-400 hover:text-white transition text-xs" title="Copy">📋</button>
				</div>
			</div>

			<div class="flex-1 overflow-y-auto">
				{#if contacts.length === 0}
					<div class="p-4 text-center text-gray-500">
						<p>No contacts yet</p>
						<p class="text-sm">Add a contact to start chatting</p>
					</div>
				{:else}
					{#each contacts as contact}
						{@const online = isOnline(contact.pubkey)}
						{@const unread = getUnreadCount(contact.pubkey)}
						<button onclick={() => selectContact(contact.pubkey)} class="w-full p-4 flex items-center gap-3 hover:bg-gray-700/50 transition {selectedContact === contact.pubkey ? 'bg-gray-700/50 border-l-2 border-orange-500' : ''}">
							<div class="relative">
								<span class="absolute -top-1 -right-1 w-3 h-3 rounded-sm border-2 border-gray-800 {online ? 'bg-green-500' : 'bg-gray-500'}"></span>
								<div class="w-10 h-10 rounded-lg bg-gray-600 overflow-hidden">
									<svg width="40" height="40" data-jdenticon-value={contact.pubkey}></svg>
								</div>
							</div>
							<div class="flex-1 text-left">
								<p class="font-medium">{contact.name}</p>
								<p class="text-xs {online ? 'text-green-400' : 'text-gray-500'}">{online ? 'Online' : 'Offline'}</p>
							</div>
							{#if unread > 0}
								<span class="bg-orange-600 text-xs px-2 py-1 rounded-lg">{unread}</span>
							{/if}
						</button>
					{/each}
				{/if}
			</div>

			<div class="p-4 border-t border-gray-700">
				<button onclick={() => showAddContact = true} class="w-full py-2 bg-orange-600 hover:bg-orange-500 rounded-lg transition">➕ Add Contact</button>
			</div>
		</aside>

		<!-- Main Chat Area -->
		<main class="flex-1 flex flex-col bg-gradient-to-br from-gray-900 via-orange-950/30 to-gray-900">
			{#if selectedContact}
				{@const contact = contacts.find(c => c.pubkey === selectedContact)}
				{@const online = isOnline(selectedContact)}
				{@const chatMessages = messages[selectedContact] || []}

				<!-- Chat Header -->
				<div class="p-4 border-b border-gray-700 flex items-center gap-4 bg-gray-800/30">
					<div class="w-10 h-10 rounded-lg bg-gray-600 overflow-hidden">
						<svg width="40" height="40" data-jdenticon-value={selectedContact}></svg>
					</div>
					<div>
						<p class="font-semibold">{contact?.name || 'Unknown'}</p>
						<p class="text-sm {online ? 'text-green-400' : 'text-gray-500'}">{online ? '● Online' : '○ Offline'}</p>
					</div>
				</div>

				<!-- Messages -->
				<div class="flex-1 overflow-y-auto p-4 space-y-4">
					{#if chatMessages.length === 0}
						<div class="text-center text-gray-500 py-12">
							<p>No messages yet</p>
							<p class="text-sm">Send a message to start the conversation</p>
						</div>
					{:else}
						{#each chatMessages as msg}
							<div class="flex {msg.from_me ? 'justify-end' : 'justify-start'}">
								<div class="max-w-[70%] {msg.from_me ? 'bg-orange-600' : 'bg-gray-700'} rounded-lg px-4 py-2">
									<p class="break-words">{msg.content}</p>
									<p class="text-xs {msg.from_me ? 'text-orange-200' : 'text-gray-400'} mt-1">{formatTime(msg.timestamp)}</p>
								</div>
							</div>
						{/each}
					{/if}
				</div>

				<!-- Input -->
				<div class="p-4 border-t border-gray-700 bg-gray-800/30">
					<div class="flex gap-2">
						<input 
							type="text" 
							bind:value={newMessage} 
							placeholder={online ? 'Type a message...' : 'User is offline'}
							disabled={!online}
							onkeypress={(e) => e.key === 'Enter' && sendMessage()}
							class="flex-1 p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none disabled:opacity-50"
						/>
						<button onclick={sendMessage} disabled={!newMessage.trim() || !online} class="px-6 py-3 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded-lg transition">📤</button>
					</div>
				</div>
			{:else}
				<div class="flex-1 flex items-center justify-center">
					<div class="text-center text-gray-500">
						<div class="text-6xl mb-4">💬</div>
						<h2 class="text-xl font-semibold mb-2">Select a contact</h2>
						<p>Choose a contact from the list to start chatting</p>
					</div>
				</div>
			{/if}
		</main>
	</div>
</div>
