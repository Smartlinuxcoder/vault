<script>
	import { onMount, onDestroy, tick } from 'svelte';
	import { goto } from '$app/navigation';
	import { page } from '$app/stores';
	import { api, p2pApi } from '$lib/api.js';
	import { Flame, Globe, MessageSquare, FolderLock, Lock, Copy, UserPlus, Send, X, Menu, ArrowLeft, BookUser, Zap, Server, Check, CheckCheck, Edit2 } from 'lucide-svelte';

	let contacts = $state([]);
	let onlinePeers = $state([]);
	let knownNodes = $state([]);
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
	let sendingViaRelay = $state(false);

	// New state for rename and reconnection
	let showRenameContact = $state(false);
	let renameContactName = $state('');
	let reconnectAttempts = $state(0);
	let reconnectTimeout = $state(null);
	let messagesContainer = $state(null);
	let keepAliveInterval = $state(null);
	let lastPongTime = $state(Date.now());
	let connectionCheckInterval = $state(null);
	let isPageVisible = $state(true);

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

		// Check URL params for contact selection
		const urlParams = new URLSearchParams(window.location.search);
		const contactParam = urlParams.get('contact');
		if (contactParam) {
			selectedContact = contactParam;
			// Add contact if not exists
			if (!contacts.find(c => c.pubkey === contactParam)) {
				const fullContacts = JSON.parse(localStorage.getItem('p2p_contacts_full') || '[]');
				const fullContact = fullContacts.find(c => c.pubkey === contactParam);
				if (fullContact) {
					contacts = [...contacts, { pubkey: fullContact.pubkey, name: fullContact.name, added_at: fullContact.added_at }];
					saveContacts();
				}
				// Scroll to bottom after mount
				scrollToBottom();
			}
		}

		// Handle page visibility changes
		const handleVisibilityChange = () => {
			isPageVisible = !document.hidden;
			if (isPageVisible && !connected) {
				// Reconnect immediately when page becomes visible
				console.log('[P2P Chat] Page visible, checking connection...');
				reconnectAttempts = 0;
				connectWebSocket();
			}
		};
		document.addEventListener('visibilitychange', handleVisibilityChange);

		connectWebSocket();
		loadKnownNodes();
		updateJdenticon();

		// Cleanup visibility listener
		return () => {
			document.removeEventListener('visibilitychange', handleVisibilityChange);
		};
	});

	onDestroy(() => {
		// Cleanup on destroy
		if (reconnectTimeout) clearTimeout(reconnectTimeout);
		if (keepAliveInterval) clearInterval(keepAliveInterval);
		if (connectionCheckInterval) clearInterval(connectionCheckInterval);
		if (window.p2pSocket) {
			try { window.p2pSocket.close(1000, 'Component unmounted'); } catch(e) {}
			window.p2pSocket = null;
		}
	});

	async function loadKnownNodes() {
		try {
			knownNodes = await p2pApi.getKnownPeers();
		} catch (e) {
			console.error('Failed to load known nodes', e);
		}
	}

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (contacts.length >= 0 || onlinePeers.length >= 0) updateJdenticon();
	});

	// Scroll to bottom when selectedContact changes or new messages arrive
	$effect(() => {
		if (selectedContact && messages[selectedContact]) {
			scrollToBottom();
		}
	});

	async function scrollToBottom() {
		await tick();
		if (messagesContainer) {
			messagesContainer.scrollTop = messagesContainer.scrollHeight;
		}
	}

	function connectWebSocket() {
		if (window.p2pSocket) {
			try { window.p2pSocket.close(); } catch(e) {}
		}

		// Clear previous keep alive interval
		if (keepAliveInterval) {
			clearInterval(keepAliveInterval);
			keepAliveInterval = null;
		}

		const ws = api.createWebSocket('/p2p/ws');
		window.p2pSocket = ws;

		ws.onopen = () => {
			console.log('[P2P Chat] WebSocket opened');
			reconnectAttempts = 0; // Reset reconnect attempts on successful connection
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
					onlinePeers = (data.PeerList.peers || []).map(p => ({
						pubkey: p.pubkey,
						name: p.name,
						is_connected: p.is_connected !== false
					}));
				} else if (data.PeerStatus) {
					const { pubkey, online } = data.PeerStatus;
					if (online) {
						if (!onlinePeers.find(p => p.pubkey === pubkey)) {
							onlinePeers = [...onlinePeers, { pubkey, name: null, is_connected: true }];
						}
					} else {
						onlinePeers = onlinePeers.filter(p => p.pubkey !== pubkey);
					}
				} else if (data.IncomingMessage) {
					handleIncomingMessage(data.IncomingMessage, false);
				} else if (data.RelayedMessage) {
					handleRelayedMessage(data.RelayedMessage);
				} else if (data.MessageAck) {
					handleMessageAck(data.MessageAck);
				} else if (data.Error) {
					error = data.Error.message;
					setTimeout(() => error = null, 5000);
				} else if (data.Pong) {
					lastPongTime = Date.now();
				}
			} catch(e) {
				console.error('[P2P Chat] Parse error:', e);
			}
		};

		ws.onclose = (event) => {
			console.log('[P2P Chat] WebSocket closed', event.code, event.reason);
			connected = false;

			// Exponential backoff reconnection
			const baseDelay = 1000;
			const maxDelay = 30000;
			const delay = Math.min(baseDelay * Math.pow(2, reconnectAttempts), maxDelay);
			reconnectAttempts++;

			console.log(`[P2P Chat] Reconnecting in ${delay}ms (attempt ${reconnectAttempts})`);

			if (reconnectTimeout) clearTimeout(reconnectTimeout);
			reconnectTimeout = setTimeout(() => {
				if (!connected) {
					connectWebSocket();
				}
			}, delay);
		};

		ws.onerror = (err) => {
			console.error('[P2P Chat] WebSocket error:', err);
		};

		// Keep alive with proper interval management
		keepAliveInterval = setInterval(() => {
			if (ws.readyState === WebSocket.OPEN) {
				ws.send(JSON.stringify({ Ping: null }));
			}
		}, 25000);

		// Check connection status
		connectionCheckInterval = setInterval(() => {
			if (Date.now() - lastPongTime > 60000) {
				console.warn('[P2P Chat] No Pong received in the last 60 seconds, reconnecting...');
				ws.close();
			}
		}, 30000);
	}

	function handleIncomingMessage(msgData, viaRelay) {
		const payload = new Uint8Array(msgData.encrypted_payload);
		const text = new TextDecoder().decode(payload);
		const from = msgData.from_pubkey;
		const timestamp = msgData.timestamp || Math.floor(Date.now() / 1000);
		const messageId = msgData.message_id || `${from}-${timestamp}`;

		// Auto-add unknown sender to contacts
		autoAddContact(from);

		const newMsg = {
			id: messageId,
			from_me: false,
			content: text,
			timestamp,
			delivered: true,
			read: selectedContact === from,
			via_relay: viaRelay
		};

		if (!messages[from]) messages[from] = [];

		// Avoid duplicates
		if (!messages[from].find(m => m.id === messageId)) {
			messages[from] = [...messages[from], newMsg];
			messages = { ...messages };
			saveMessages();

			// Send ACK back
			sendMessageAck(from, messageId);
		}
	}

	function handleRelayedMessage(msgData) {
		const payload = new Uint8Array(msgData.payload);
		const text = new TextDecoder().decode(payload);
		const from = msgData.from_pubkey;
		const fromNode = msgData.from_node;
		const timestamp = msgData.timestamp || Math.floor(Date.now() / 1000);
		const messageId = msgData.message_id || `${from}-${timestamp}`;

		// Auto-add unknown sender to contacts
		autoAddContact(from);

		const newMsg = {
			id: messageId,
			from_me: false,
			content: text,
			timestamp,
			delivered: true,
			read: selectedContact === from,
			via_relay: true,
			relay_node: fromNode
		};

		if (!messages[from]) messages[from] = [];

		// Avoid duplicates
		if (!messages[from].find(m => m.id === messageId)) {
			messages[from] = [...messages[from], newMsg];
			messages = { ...messages };
			saveMessages();

			// Send ACK back
			sendMessageAck(from, messageId);
		}
	}

	function handleMessageAck(ackData) {
		const { from_pubkey, message_id } = ackData;

		// Find and update the message status
		for (const [pubkey, msgList] of Object.entries(messages)) {
			const msgIndex = msgList.findIndex(m => m.id === message_id);
			if (msgIndex !== -1) {
				messages[pubkey][msgIndex] = {
					...messages[pubkey][msgIndex],
					ack: true,
					ack_at: Math.floor(Date.now() / 1000)
				};
				messages = { ...messages };
				saveMessages();
				break;
			}
		}
	}

	function sendMessageAck(toPubkey, messageId) {
		const ws = window.p2pSocket;
		if (ws && ws.readyState === WebSocket.OPEN) {
			ws.send(JSON.stringify({
				MessageAck: {
					to_pubkey: toPubkey,
					message_id: messageId
				}
			}));
		}
	}

	function autoAddContact(pubkey) {
		if (pubkey === myPubkey) return;
		if (contacts.find(c => c.pubkey === pubkey)) return;

		const newContact = {
			pubkey,
			name: `User ${pubkey.slice(0, 8)}...`,
			added_at: Math.floor(Date.now() / 1000),
			last_message: null,
			auto_added: true
		};

		contacts = [...contacts, newContact];
		saveContacts();
		updateJdenticon();
	}

	async function sendMessage() {
		if (!newMessage.trim() || !selectedContact) return;

		const ws = window.p2pSocket;
		if (!ws || ws.readyState !== WebSocket.OPEN) {
			error = 'Not connected';
			return;
		}

		const timestamp = Math.floor(Date.now() / 1000);
		const messageId = `me-${timestamp}-${Math.random().toString(36).slice(2, 8)}`;
		const payload = new TextEncoder().encode(newMessage);
		const isOnlineLocally = isOnline(selectedContact);

		// Try sending via WebSocket first
		ws.send(JSON.stringify({
			SendMessage: {
				to_pubkey: selectedContact,
				encrypted_payload: Array.from(payload),
				message_id: messageId
			}
		}));

		const newMsg = {
			id: messageId,
			from_me: true,
			content: newMessage,
			timestamp,
			delivered: false,
			ack: false,
			read: false,
			via_relay: !isOnlineLocally
		};

		if (!messages[selectedContact]) messages[selectedContact] = [];
		messages[selectedContact] = [...messages[selectedContact], newMsg];
		messages = { ...messages };
		saveMessages();

		newMessage = '';

		// Mark as delivered after a short delay (optimistic)
		setTimeout(() => {
			const msgIndex = messages[selectedContact]?.findIndex(m => m.id === messageId);
			if (msgIndex !== -1 && messages[selectedContact]) {
				messages[selectedContact][msgIndex] = {
					...messages[selectedContact][msgIndex],
					delivered: true
				};
				messages = { ...messages };
				saveMessages();
			}
		}, 500);
	}

	function addContact() {
		const pubkey = newContactPubkey.trim();
		const name = newContactName.trim();

		if (!pubkey || !name) return;

		if (pubkey === myPubkey) {
			error = 'Cannot add yourself';
			return;
		}

		const existingIndex = contacts.findIndex(c => c.pubkey === pubkey);
		if (existingIndex !== -1) {
			// Update existing contact name
			contacts[existingIndex] = { ...contacts[existingIndex], name, auto_added: false };
			contacts = [...contacts];
		} else {
			const newContact = {
				pubkey,
				name,
				added_at: Math.floor(Date.now() / 1000),
				last_message: null
			};
			contacts = [...contacts, newContact];
		}

		saveContacts();

		newContactPubkey = '';
		newContactName = '';
		showAddContact = false;
	}

	function renameContact() {
		if (!selectedContact || !renameContactName.trim()) return;

		const idx = contacts.findIndex(c => c.pubkey === selectedContact);
		if (idx !== -1) {
			contacts[idx] = { 
				...contacts[idx], 
				name: renameContactName.trim(),
				auto_added: false 
			};
			contacts = [...contacts];
			saveContacts();

			// Also update in full contacts if exists
			const fullContacts = JSON.parse(localStorage.getItem('p2p_contacts_full') || '[]');
			const fullIdx = fullContacts.findIndex(c => c.pubkey === selectedContact);
			if (fullIdx !== -1) {
				fullContacts[fullIdx].name = renameContactName.trim();
				localStorage.setItem('p2p_contacts_full', JSON.stringify(fullContacts));
			}
		}

		showRenameContact = false;
		renameContactName = '';
	}

	function openRenameDialog() {
		const contact = contacts.find(c => c.pubkey === selectedContact);
		if (contact) {
			renameContactName = contact.name;
			showRenameContact = true;
		}
	}

	function selectContact(pubkey) {
		selectedContact = pubkey;
		// Mark messages as read
		if (messages[pubkey]) {
			messages[pubkey] = messages[pubkey].map(m => ({ ...m, read: true }));
			messages = { ...messages };
			saveMessages();
		}
		// Scroll to bottom
		scrollToBottom();
	}

	function saveContacts() {
		localStorage.setItem('p2p_contacts', JSON.stringify(contacts));
	}

	function saveMessages() {
		localStorage.setItem('p2p_messages', JSON.stringify(messages));
	}

	function isOnline(pubkey) {
		const peer = onlinePeers.find(p => p.pubkey === pubkey);
		return peer && peer.is_connected !== false;
	}

	function canReachViaRelay(pubkey) {
		return knownNodes.length > 0;
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
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" role="dialog" aria-modal="true" onclick={() => showAddContact = false}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-6 w-full max-w-md" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between mb-4">
				<h3 class="text-lg font-medium">Add Contact</h3>
				<button onclick={() => showAddContact = false} class="p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>
			
			<div class="space-y-4">
				<div class="space-y-2">
					<label for="quick-contact-name" class="text-sm font-medium">Name</label>
					<input id="quick-contact-name" type="text" bind:value={newContactName} placeholder="Contact name" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
				</div>
				<div class="space-y-2">
					<label for="quick-contact-pubkey" class="text-sm font-medium">Public Key</label>
					<textarea id="quick-contact-pubkey" bind:value={newContactPubkey} placeholder="Paste the contact's public key" rows="3" class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950 resize-none"></textarea>
				</div>
			</div>

			<div class="flex gap-2 mt-6">
				<button onclick={() => showAddContact = false} class="flex-1 h-9 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors">Cancel</button>
				<button onclick={addContact} disabled={!newContactName.trim() || !newContactPubkey.trim()} class="flex-1 h-9 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md text-sm font-medium transition-colors">Add</button>
			</div>
		</div>
	</div>
{/if}

<!-- Rename Contact Modal -->
{#if showRenameContact}
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" role="dialog" aria-modal="true" onclick={() => showRenameContact = false}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-6 w-full max-w-md" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between mb-4">
				<h3 class="text-lg font-medium">Rename Contact</h3>
				<button onclick={() => showRenameContact = false} class="p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>
			
			<div class="space-y-2">
				<label for="rename-contact-name" class="text-sm font-medium">New Name</label>
				<input 
					id="rename-contact-name" 
					type="text" 
					bind:value={renameContactName} 
					placeholder="Enter new name" 
					onkeypress={(e) => e.key === 'Enter' && renameContact()}
					class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" 
				/>
			</div>

			<div class="flex gap-2 mt-6">
				<button onclick={() => showRenameContact = false} class="flex-1 h-9 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors">Cancel</button>
				<button onclick={renameContact} disabled={!renameContactName.trim()} class="flex-1 h-9 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md text-sm font-medium transition-colors">Rename</button>
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

<!-- Reconnecting indicator -->
{#if !connected && reconnectAttempts > 0}
	<div class="fixed top-16 left-1/2 -translate-x-1/2 bg-yellow-950 border border-yellow-900 text-yellow-400 px-4 py-2 rounded-md flex items-center gap-2 z-50 text-sm">
		<span class="animate-spin">⟳</span>
		<span>Reconnecting... (attempt {reconnectAttempts})</span>
	</div>
{/if}

<div class="h-screen flex flex-col bg-zinc-950">
	<!-- Top Navbar -->
	<header class="border-b border-zinc-800 px-4 md:px-6 py-3 flex items-center justify-between shrink-0">
		<div class="flex items-center gap-3">
			<button onclick={() => showSidebar = !showSidebar} class="p-2 text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors md:hidden">
				<Menu class="w-5 h-5" />
			</button>
			<Flame class="w-6 h-6 text-orange-500" />
			<div>
				<h1 class="text-sm font-semibold">Arsonnet</h1>
				<div class="flex items-center gap-1.5 text-xs">
					<span class="w-1.5 h-1.5 rounded-full {connected ? 'bg-green-500' : 'bg-red-500'} {!connected && reconnectAttempts > 0 ? 'animate-pulse' : ''}"></span>
					<span class="text-zinc-400">{connected ? 'Online' : reconnectAttempts > 0 ? 'Reconnecting...' : 'Offline'}</span>
					{#if knownNodes.length > 0}
						<span class="text-zinc-600">•</span>
						<span class="text-zinc-500">{knownNodes.length} nodes</span>
					{/if}
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
			<button onclick={() => goto('/contacts')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Contacts">
				<BookUser class="w-4 h-4" />
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
			<button 
				class="fixed inset-0 bg-black/50 z-30 md:hidden border-0 cursor-default" 
				onclick={() => showSidebar = false}
				onkeydown={(e) => e.key === 'Escape' && (showSidebar = false)}
				aria-label="Close sidebar"
			></button>
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
						<button onclick={() => goto('/contacts')} class="mt-3 text-xs text-orange-500 hover:text-orange-400">
							Open Contacts Book →
						</button>
					</div>
				{:else}
					{#each contacts as contact}
						{@const online = isOnline(contact.pubkey)}
						{@const canRelay = canReachViaRelay(contact.pubkey)}
						{@const unread = getUnreadCount(contact.pubkey)}
						<button onclick={() => { selectContact(contact.pubkey); showSidebar = false; }} class="w-full p-3 flex items-center gap-3 hover:bg-zinc-800 transition-colors {selectedContact === contact.pubkey ? 'bg-zinc-800 border-l-2 border-orange-500' : 'border-l-2 border-transparent'}">
							<div class="relative shrink-0">
								<span class="absolute -top-0.5 -right-0.5 w-2.5 h-2.5 rounded-full border-2 border-zinc-900 {online ? 'bg-green-500' : canRelay ? 'bg-yellow-500' : 'bg-zinc-600'} z-10"></span>
								<div class="w-9 h-9 rounded-md bg-zinc-700 overflow-hidden flex items-center justify-center">
									<svg width="36" height="36" data-jdenticon-value={contact.pubkey}></svg>
								</div>
							</div>
							<div class="flex-1 text-left min-w-0">
								<div class="flex items-center gap-1">
									<p class="text-sm font-medium truncate">{contact.name}</p>
									{#if contact.auto_added}
										<span class="text-[10px] text-zinc-500 bg-zinc-800 px-1 rounded">new</span>
									{/if}
								</div>
								<p class="text-xs {online ? 'text-green-500' : canRelay ? 'text-yellow-500' : 'text-zinc-500'}">
									{online ? 'Online' : canRelay ? 'Via Relay' : 'Offline'}
								</p>
							</div>
							{#if unread > 0}
								<span class="bg-orange-600 text-xs px-1.5 py-0.5 rounded-md font-medium shrink-0">{unread}</span>
							{/if}
						</button>
					{/each}
				{/if}
			</div>

			<div class="p-3 border-t border-zinc-800 space-y-2">
				<button onclick={() => goto('/contacts')} class="w-full h-9 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors flex items-center justify-center gap-2">
					<BookUser class="w-4 h-4" />
					Contacts Book
				</button>
				<button onclick={() => showAddContact = true} class="w-full h-9 bg-orange-600 hover:bg-orange-700 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2">
					<UserPlus class="w-4 h-4" />
					Quick Add
				</button>
			</div>
		</aside>

		<!-- Main Chat Area -->
		<main class="flex-1 flex flex-col bg-zinc-950 w-full">
			{#if selectedContact}
				{@const contact = contacts.find(c => c.pubkey === selectedContact)}
				{@const online = isOnline(selectedContact)}
				{@const canRelay = canReachViaRelay(selectedContact)}
				{@const chatMessages = messages[selectedContact] || []}

				<!-- Chat Header -->
				<div class="p-3 border-b border-zinc-800 flex items-center gap-3">
					<button onclick={() => { selectedContact = null; showSidebar = true; }} class="p-2 text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors md:hidden -ml-1">
						<ArrowLeft class="w-5 h-5" />
					</button>
					<div class="w-9 h-9 rounded-md bg-zinc-700 overflow-hidden flex items-center justify-center shrink-0">
						<svg width="36" height="36" data-jdenticon-value={selectedContact}></svg>
					</div>
					<div class="min-w-0 flex-1">
						<div class="flex items-center gap-2">
							<p class="text-sm font-medium truncate">{contact?.name || 'Unknown'}</p>
							<button onclick={openRenameDialog} class="p-1 text-zinc-500 hover:text-zinc-300 hover:bg-zinc-800 rounded transition-colors" title="Rename contact">
								<Edit2 class="w-3 h-3" />
							</button>
						</div>
						<p class="text-xs {online ? 'text-green-500' : canRelay ? 'text-yellow-500' : 'text-zinc-500'}">
							{online ? 'Online' : canRelay ? 'Reachable via relay' : 'Offline'}
						</p>
					</div>
					{#if canRelay && !online}
						<div class="flex items-center gap-1 text-xs text-yellow-500 bg-yellow-500/10 px-2 py-1 rounded">
							<Server class="w-3 h-3" />
							Cross-node
						</div>
					{/if}
				</div>

				<!-- Messages -->
				<div bind:this={messagesContainer} class="flex-1 overflow-y-auto p-3 md:p-4 space-y-3">
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
									<div class="flex items-center gap-2 mt-1">
										<p class="text-xs {msg.from_me ? 'text-orange-200' : 'text-zinc-500'}">{formatTime(msg.timestamp)}</p>
										{#if msg.via_relay}
											<span class="text-xs {msg.from_me ? 'text-orange-200' : 'text-yellow-500'} flex items-center gap-0.5">
												<Zap class="w-3 h-3" />
												relay
											</span>
										{/if}
										{#if msg.from_me}
											<span class="text-xs text-orange-200 flex items-center gap-0.5" title={msg.ack ? 'Delivered & Read' : msg.delivered ? 'Sent' : 'Sending...'}>
												{#if msg.ack}
													<CheckCheck class="w-3 h-3" />
												{:else if msg.delivered}
													<Check class="w-3 h-3" />
												{:else}
													<span class="w-3 h-3 flex items-center justify-center">○</span>
												{/if}
											</span>
										{/if}
									</div>
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
							placeholder={online ? 'Type a message...' : canRelay ? 'Send via relay...' : 'User is offline'}
							disabled={!online && !canRelay}
							onkeypress={(e) => e.key === 'Enter' && sendMessage()}
							class="flex-1 h-11 md:h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950 disabled:opacity-50"
						/>
						{#if online}
							<button onclick={sendMessage} disabled={!newMessage.trim()} class="h-11 md:h-10 px-4 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center gap-2">
								<Send class="w-4 h-4" />
							</button>
						{:else if canRelay}
							<button onclick={sendMessage} disabled={!newMessage.trim() || sendingViaRelay} class="h-11 md:h-10 px-4 bg-yellow-600 hover:bg-yellow-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center gap-2">
								{#if sendingViaRelay}
									<span class="animate-spin">⟳</span>
								{:else}
									<Zap class="w-4 h-4" />
								{/if}
							</button>
						{:else}
							<button disabled class="h-11 md:h-10 px-4 bg-zinc-700 opacity-50 rounded-md font-medium flex items-center gap-2">
								<Send class="w-4 h-4" />
							</button>
						{/if}
					</div>
					{#if !online && canRelay}
						<p class="text-xs text-yellow-500 mt-2 flex items-center gap-1">
							<Server class="w-3 h-3" />
							Message will be sent through {knownNodes.length} relay node(s)
						</p>
					{/if}
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
