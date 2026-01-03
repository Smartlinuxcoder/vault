<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';
	import { Flame, Globe, MessageSquare, FolderLock, Lock, Copy, UserPlus, Send, X, Menu, ArrowLeft } from 'lucide-svelte';

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
	let showSidebar = $state(true);

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
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" onclick={() => showAddContact = false}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-6 w-full max-w-md" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between mb-4">
				<h3 class="text-lg font-medium">Add Contact</h3>
				<button onclick={() => showAddContact = false} class="p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>
			
			<div class="space-y-4">
				<div class="space-y-2">
					<label class="text-sm font-medium">Name</label>
					<input type="text" bind:value={newContactName} placeholder="Contact name" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
				</div>
				<div class="space-y-2">
					<label class="text-sm font-medium">Public Key</label>
					<textarea bind:value={newContactPubkey} placeholder="Paste the contact's public key" rows="3" class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950 resize-none"></textarea>
				</div>
			</div>

			<div class="flex gap-2 mt-6">
				<button onclick={() => showAddContact = false} class="flex-1 h-9 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors">Cancel</button>
				<button onclick={addContact} disabled={!newContactName.trim() || !newContactPubkey.trim()} class="flex-1 h-9 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md text-sm font-medium transition-colors">Add</button>
			</div>
		</div>
	</div>
{/if}

<!-- Error Toast -->
{#if error}
	<div class="fixed bottom-4 right-4 bg-red-950 border border-red-900 text-red-400 px-4 py-3 rounded-md flex items-center gap-3 z-50 text-sm max-w-[90vw]">
		<span>{error}</span>
		<button onclick={() => error = null} class="text-red-400 hover:text-red-300">
			<X class="w-4 h-4" />
		</button>
	</div>
{/if}

<div class="h-screen flex flex-col bg-zinc-950">
	<!-- Top Navbar -->
	<header class="border-b border-zinc-800 px-4 md:px-6 py-3 flex items-center justify-between shrink-0">
		<div class="flex items-center gap-3">
			<!-- Mobile menu button -->
			<button onclick={() => showSidebar = !showSidebar} class="p-2 text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors md:hidden">
				<Menu class="w-5 h-5" />
			</button>
			<Flame class="w-6 h-6 text-orange-500" />
			<div>
				<h1 class="text-sm font-semibold">Arsonnet</h1>
				<div class="flex items-center gap-1.5 text-xs">
					<span class="w-1.5 h-1.5 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'}"></span>
					<span class="text-zinc-400">{connected ? 'Online' : 'Offline'}</span>
				</div>
			</div>
		</div>
		<nav class="flex gap-1">
			<button onclick={() => goto('/network')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Network">
				<Globe class="w-4 h-4" />
			</button>
			<button onclick={() => goto('/chat')} class="p-2 text-sm bg-zinc-800 text-zinc-100 rounded-md" title="Chat">
				<MessageSquare class="w-4 h-4" />
			</button>
			<button onclick={() => goto('/vault')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Vault">
				<FolderLock class="w-4 h-4" />
			</button>
			<button onclick={() => { sessionStorage.clear(); goto('/'); }} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Lock">
				<Lock class="w-4 h-4" />
			</button>
		</nav>
	</header>

	<div class="flex flex-1 overflow-hidden relative">
		<!-- Mobile Overlay -->
		{#if showSidebar}
			<div class="fixed inset-0 bg-black/50 z-30 md:hidden" onclick={() => showSidebar = false}></div>
		{/if}

		<!-- Sidebar -->
		<aside class="w-72 md:w-64 bg-zinc-900 border-r border-zinc-800 flex flex-col shrink-0 fixed md:relative inset-y-0 left-0 z-40 transform transition-transform duration-200 ease-in-out {showSidebar ? 'translate-x-0' : '-translate-x-full'} md:translate-x-0 top-[57px] md:top-0 h-[calc(100vh-57px)] md:h-auto">
			<div class="p-3 border-b border-zinc-800">
				<p class="text-xs text-zinc-400">{myName}</p>
				<div class="flex items-center gap-2 mt-0.5">
					<code class="text-xs text-zinc-500 truncate flex-1 font-mono">{myPubkey.slice(0, 16)}...</code>
					<button onclick={() => navigator.clipboard.writeText(myPubkey)} class="text-zinc-500 hover:text-zinc-300 transition-colors" title="Copy">
						<Copy class="w-3 h-3" />
					</button>
				</div>
			</div>

			<div class="flex-1 overflow-y-auto">
				{#if contacts.length === 0}
					<div class="p-4 text-center">
						<p class="text-sm text-zinc-500">No contacts yet</p>
						<p class="text-xs text-zinc-600 mt-1">Add a contact to start chatting</p>
					</div>
				{:else}
					{#each contacts as contact}
						{@const online = isOnline(contact.pubkey)}
						{@const unread = getUnreadCount(contact.pubkey)}
						<button onclick={() => { selectContact(contact.pubkey); showSidebar = false; }} class="w-full p-3 flex items-center gap-3 hover:bg-zinc-800 transition-colors {selectedContact === contact.pubkey ? 'bg-zinc-800 border-l-2 border-orange-500' : 'border-l-2 border-transparent'}">
							<div class="relative shrink-0">
								<span class="absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 border-zinc-900 {online ? 'bg-green-500' : 'bg-zinc-600'} z-10"></span>
								<div class="w-9 h-9 rounded-md bg-zinc-700 overflow-hidden flex items-center justify-center">
									<svg width="36" height="36" data-jdenticon-value={contact.pubkey}></svg>
								</div>
							</div>
							<div class="flex-1 text-left min-w-0">
								<p class="text-sm font-medium truncate">{contact.name}</p>
								<p class="text-xs {online ? 'text-green-500' : 'text-zinc-500'}">{online ? 'Online' : 'Offline'}</p>
							</div>
							{#if unread > 0}
								<span class="bg-orange-600 text-xs px-1.5 py-0.5 rounded-md font-medium shrink-0">{unread}</span>
							{/if}
						</button>
					{/each}
				{/if}
			</div>

			<div class="p-3 border-t border-zinc-800">
				<button onclick={() => showAddContact = true} class="w-full h-9 bg-orange-600 hover:bg-orange-700 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2">
					<UserPlus class="w-4 h-4" />
					Add Contact
				</button>
			</div>
		</aside>

		<!-- Main Chat Area -->
		<main class="flex-1 flex flex-col bg-zinc-950 w-full">
			{#if selectedContact}
				{@const contact = contacts.find(c => c.pubkey === selectedContact)}
				{@const online = isOnline(selectedContact)}
				{@const chatMessages = messages[selectedContact] || []}

				<!-- Chat Header -->
				<div class="p-3 border-b border-zinc-800 flex items-center gap-3">
					<!-- Back button for mobile -->
					<button onclick={() => { selectedContact = null; showSidebar = true; }} class="p-2 text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors md:hidden -ml-1">
						<ArrowLeft class="w-5 h-5" />
					</button>
					<div class="w-9 h-9 rounded-md bg-zinc-700 overflow-hidden flex items-center justify-center shrink-0">
						<svg width="36" height="36" data-jdenticon-value={selectedContact}></svg>
					</div>
					<div class="min-w-0 flex-1">
						<p class="text-sm font-medium truncate">{contact?.name || 'Unknown'}</p>
						<p class="text-xs {online ? 'text-green-500' : 'text-zinc-500'}">{online ? 'Online' : 'Offline'}</p>
					</div>
				</div>

				<!-- Messages -->
				<div class="flex-1 overflow-y-auto p-3 md:p-4 space-y-3">
					{#if chatMessages.length === 0}
						<div class="text-center py-12">
							<p class="text-sm text-zinc-400">No messages yet</p>
							<p class="text-xs text-zinc-500 mt-1">Send a message to start the conversation</p>
						</div>
					{:else}
						{#each chatMessages as msg}
							<div class="flex {msg.from_me ? 'justify-end' : 'justify-start'}">
								<div class="max-w-[85%] md:max-w-[70%] {msg.from_me ? 'bg-orange-600' : 'bg-zinc-800 border border-zinc-700'} rounded-lg px-3 py-2">
									<p class="text-sm break-words">{msg.content}</p>
									<p class="text-xs {msg.from_me ? 'text-orange-200' : 'text-zinc-500'} mt-1">{formatTime(msg.timestamp)}</p>
								</div>
							</div>
						{/each}
					{/if}
				</div>

				<!-- Input -->
				<div class="p-3 border-t border-zinc-800">
					<div class="flex gap-2">
						<input 
							type="text" 
							bind:value={newMessage} 
							placeholder={online ? 'Type a message...' : 'User is offline'}
							disabled={!online}
							onkeypress={(e) => e.key === 'Enter' && sendMessage()}
							class="flex-1 h-11 md:h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950 disabled:opacity-50"
						/>
						<button onclick={sendMessage} disabled={!newMessage.trim() || !online} class="h-11 md:h-10 px-4 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center gap-2">
							<Send class="w-4 h-4" />
						</button>
					</div>
				</div>
			{:else}
				<div class="flex-1 flex items-center justify-center p-4">
					<div class="text-center">
						<MessageSquare class="w-12 h-12 text-zinc-700 mx-auto mb-3" />
						<p class="text-sm text-zinc-400">Select a contact</p>
						<p class="text-xs text-zinc-500 mt-1">Choose a contact from the list to start chatting</p>
						<button onclick={() => showSidebar = true} class="mt-4 px-4 py-2 bg-zinc-800 hover:bg-zinc-700 rounded-md text-sm transition-colors md:hidden">
							View Contacts
						</button>
					</div>
				</div>
			{/if}
		</main>
	</div>
</div>
