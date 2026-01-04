<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api, p2pApi } from '$lib/api.js';
	import { 
		Flame, Globe, MessageSquare, FolderLock, Lock, Copy, UserPlus, Search, 
		X, Trash2, Edit2, Check, Users, QrCode, Download, Upload, Star, StarOff,
		Phone, Mail, MapPin, Calendar, MoreVertical, BookUser
	} from 'lucide-svelte';

	let contacts = $state([]);
	let searchQuery = $state('');
	let showAddContact = $state(false);
	let showImportExport = $state(false);
	let editingContact = $state(null);
	let error = $state(null);
	let success = $state(null);

	// New contact form
	let newContact = $state({
		name: '',
		pubkey: '',
		email: '',
		phone: '',
		notes: '',
		isFavorite: false
	});

	// Import/Export
	let importData = $state('');

	onMount(() => {
		const pubkey = sessionStorage.getItem('p2p_pubkey');
		if (!pubkey) {
			goto('/');
			return;
		}

		loadContacts();
		updateJdenticon();
	});

	function loadContacts() {
		const saved = localStorage.getItem('p2p_contacts_full');
		if (saved) {
			contacts = JSON.parse(saved);
		} else {
			// Migrate from old contacts format
			const oldContacts = localStorage.getItem('p2p_contacts');
			if (oldContacts) {
				const old = JSON.parse(oldContacts);
				contacts = old.map(c => ({
					...c,
					email: '',
					phone: '',
					notes: '',
					isFavorite: false,
					nodeAddress: null
				}));
				saveContacts();
			}
		}
	}

	function saveContacts() {
		localStorage.setItem('p2p_contacts_full', JSON.stringify(contacts));
		// Also save simplified version for chat compatibility
		const simplified = contacts.map(c => ({
			pubkey: c.pubkey,
			name: c.name,
			added_at: c.added_at,
			last_message: c.last_message
		}));
		localStorage.setItem('p2p_contacts', JSON.stringify(simplified));
	}

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (contacts.length >= 0) updateJdenticon();
	});

	function addContact() {
		if (!newContact.name.trim() || !newContact.pubkey.trim()) {
			error = 'Name and public key are required';
			return;
		}

		const myPubkey = sessionStorage.getItem('p2p_pubkey');
		if (newContact.pubkey.trim() === myPubkey) {
			error = 'Cannot add yourself as a contact';
			return;
		}

		if (contacts.find(c => c.pubkey === newContact.pubkey.trim())) {
			error = 'Contact already exists';
			return;
		}

		const contact = {
			id: crypto.randomUUID(),
			pubkey: newContact.pubkey.trim(),
			name: newContact.name.trim(),
			email: newContact.email.trim(),
			phone: newContact.phone.trim(),
			notes: newContact.notes.trim(),
			isFavorite: newContact.isFavorite,
			added_at: Math.floor(Date.now() / 1000),
			last_message: null,
			nodeAddress: null
		};

		contacts = [...contacts, contact];
		saveContacts();
		resetForm();
		showAddContact = false;
		success = 'Contact added successfully';
		setTimeout(() => success = null, 3000);
	}

	function updateContact() {
		if (!editingContact) return;

		const idx = contacts.findIndex(c => c.id === editingContact.id);
		if (idx !== -1) {
			contacts[idx] = { ...editingContact };
			contacts = [...contacts];
			saveContacts();
			editingContact = null;
			success = 'Contact updated';
			setTimeout(() => success = null, 3000);
		}
	}

	function deleteContact(id) {
		if (!confirm('Are you sure you want to delete this contact?')) return;
		contacts = contacts.filter(c => c.id !== id);
		saveContacts();
	}

	function toggleFavorite(id) {
		const idx = contacts.findIndex(c => c.id === id);
		if (idx !== -1) {
			contacts[idx].isFavorite = !contacts[idx].isFavorite;
			contacts = [...contacts];
			saveContacts();
		}
	}

	function resetForm() {
		newContact = {
			name: '',
			pubkey: '',
			email: '',
			phone: '',
			notes: '',
			isFavorite: false
		};
	}

	function exportContacts() {
		const data = JSON.stringify(contacts, null, 2);
		const blob = new Blob([data], { type: 'application/json' });
		const url = URL.createObjectURL(blob);
		const a = document.createElement('a');
		a.href = url;
		a.download = `arsonnet-contacts-${new Date().toISOString().split('T')[0]}.json`;
		a.click();
		URL.revokeObjectURL(url);
		success = 'Contacts exported';
		setTimeout(() => success = null, 3000);
	}

	function importContacts() {
		try {
			const imported = JSON.parse(importData);
			if (!Array.isArray(imported)) {
				error = 'Invalid format: expected an array';
				return;
			}

			let added = 0;
			for (const c of imported) {
				if (!c.pubkey || !c.name) continue;
				if (contacts.find(existing => existing.pubkey === c.pubkey)) continue;

				contacts.push({
					id: c.id || crypto.randomUUID(),
					pubkey: c.pubkey,
					name: c.name,
					email: c.email || '',
					phone: c.phone || '',
					notes: c.notes || '',
					isFavorite: c.isFavorite || false,
					added_at: c.added_at || Math.floor(Date.now() / 1000),
					last_message: null,
					nodeAddress: c.nodeAddress || null
				});
				added++;
			}

			contacts = [...contacts];
			saveContacts();
			importData = '';
			showImportExport = false;
			success = `Imported ${added} contacts`;
			setTimeout(() => success = null, 3000);
		} catch (e) {
			error = 'Invalid JSON format';
		}
	}

	function startChat(pubkey) {
		goto(`/chat?contact=${pubkey}`);
	}

	function copyPubkey(pubkey) {
		navigator.clipboard.writeText(pubkey);
		success = 'Public key copied';
		setTimeout(() => success = null, 2000);
	}

	function shortKey(key) {
		if (key && key.length > 32) {
			return key.slice(0, 16) + '...' + key.slice(-8);
		}
		return key || '';
	}

	function formatDate(ts) {
		if (!ts) return 'Unknown';
		return new Date(ts * 1000).toLocaleDateString();
	}

	// Filtered and sorted contacts
	let filteredContacts = $derived(() => {
		let result = contacts;
		
		if (searchQuery.trim()) {
			const q = searchQuery.toLowerCase();
			result = result.filter(c => 
				c.name.toLowerCase().includes(q) ||
				c.pubkey.toLowerCase().includes(q) ||
				(c.email && c.email.toLowerCase().includes(q)) ||
				(c.notes && c.notes.toLowerCase().includes(q))
			);
		}

		// Sort: favorites first, then alphabetically
		return result.sort((a, b) => {
			if (a.isFavorite && !b.isFavorite) return -1;
			if (!a.isFavorite && b.isFavorite) return 1;
			return a.name.localeCompare(b.name);
		});
	});
</script>

<!-- Add/Edit Contact Modal -->
{#if showAddContact || editingContact}
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" role="dialog" aria-modal="true" onclick={() => { showAddContact = false; editingContact = null; }}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-6 w-full max-w-lg max-h-[90vh] overflow-y-auto" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between mb-4">
				<h3 class="text-lg font-medium">{editingContact ? 'Edit Contact' : 'Add Contact'}</h3>
				<button onclick={() => { showAddContact = false; editingContact = null; resetForm(); }} class="p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>
			
			<div class="space-y-4">
				<div class="space-y-2">
					<label for="contact-name" class="text-sm font-medium">Name *</label>
					{#if editingContact}
						<input 
							id="contact-name"
							type="text" 
							bind:value={editingContact.name} 
							placeholder="Contact name" 
							class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
						/>
					{:else}
						<input 
							id="contact-name"
							type="text" 
							bind:value={newContact.name} 
							placeholder="Contact name" 
							class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
						/>
					{/if}
				</div>

				<div class="space-y-2">
					<label for="contact-pubkey" class="text-sm font-medium">Public Key *</label>
					{#if editingContact}
						<textarea 
							id="contact-pubkey"
							bind:value={editingContact.pubkey} 
							placeholder="Paste the contact's public key"
							rows="3"
							disabled
							class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 resize-none disabled:opacity-50 font-mono text-xs"
						></textarea>
					{:else}
						<textarea 
							id="contact-pubkey"
							bind:value={newContact.pubkey} 
							placeholder="Paste the contact's public key"
							rows="3"
							class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 resize-none font-mono text-xs"
						></textarea>
					{/if}
				</div>

				<div class="grid grid-cols-2 gap-4">
					<div class="space-y-2">
						<label for="contact-email" class="text-sm font-medium flex items-center gap-2">
							<Mail class="w-3.5 h-3.5 text-zinc-400" />
							Email
						</label>
						{#if editingContact}
							<input 
								id="contact-email"
								type="email" 
								bind:value={editingContact.email} 
								placeholder="email@example.com" 
								class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
							/>
						{:else}
							<input 
								id="contact-email"
								type="email" 
								bind:value={newContact.email} 
								placeholder="email@example.com" 
								class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
							/>
						{/if}
					</div>
					<div class="space-y-2">
						<label for="contact-phone" class="text-sm font-medium flex items-center gap-2">
							<Phone class="w-3.5 h-3.5 text-zinc-400" />
							Phone
						</label>
						{#if editingContact}
							<input 
								id="contact-phone"
								type="tel" 
								bind:value={editingContact.phone} 
								placeholder="+1 234 567 890" 
								class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
							/>
						{:else}
							<input 
								id="contact-phone"
								type="tel" 
								bind:value={newContact.phone} 
								placeholder="+1 234 567 890" 
								class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500" 
							/>
						{/if}
					</div>
				</div>

				<div class="space-y-2">
					<label for="contact-notes" class="text-sm font-medium">Notes</label>
					{#if editingContact}
						<textarea 
							id="contact-notes"
							bind:value={editingContact.notes} 
							placeholder="Additional notes..."
							rows="2"
							class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 resize-none"
						></textarea>
					{:else}
						<textarea 
							id="contact-notes"
							bind:value={newContact.notes} 
							placeholder="Additional notes..."
							rows="2"
							class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 resize-none"
						></textarea>
					{/if}
				</div>

				<label class="flex items-center gap-2 cursor-pointer">
					{#if editingContact}
						<input 
							type="checkbox" 
							bind:checked={editingContact.isFavorite}
							class="w-4 h-4 rounded border-zinc-700 text-orange-500 focus:ring-orange-500 focus:ring-offset-zinc-900"
						/>
					{:else}
						<input 
							type="checkbox" 
							bind:checked={newContact.isFavorite}
							class="w-4 h-4 rounded border-zinc-700 text-orange-500 focus:ring-orange-500 focus:ring-offset-zinc-900"
						/>
					{/if}
					<Star class="w-4 h-4 text-yellow-500" />
					<span class="text-sm">Mark as favorite</span>
				</label>
			</div>

			<div class="flex gap-2 mt-6">
				<button onclick={() => { showAddContact = false; editingContact = null; resetForm(); }} class="flex-1 h-9 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors">
					Cancel
				</button>
				<button 
					onclick={editingContact ? updateContact : addContact} 
					class="flex-1 h-9 bg-orange-600 hover:bg-orange-700 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2"
				>
					{#if editingContact}
						<Check class="w-4 h-4" />
						Save
					{:else}
						<UserPlus class="w-4 h-4" />
						Add
					{/if}
				</button>
			</div>
		</div>
	</div>
{/if}

<!-- Import/Export Modal -->
{#if showImportExport}
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" onclick={() => showImportExport = false}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-6 w-full max-w-lg" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between mb-4">
				<h3 class="text-lg font-medium">Import / Export Contacts</h3>
				<button onclick={() => showImportExport = false} class="p-1 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>

			<div class="space-y-4">
				<div>
					<button onclick={exportContacts} class="w-full h-10 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors flex items-center justify-center gap-2">
						<Download class="w-4 h-4" />
						Export All Contacts ({contacts.length})
					</button>
				</div>

				<div class="border-t border-zinc-800 pt-4">
						<label for="import-json" class="text-sm font-medium">Import from JSON</label>
						<textarea 
							id="import-json"
							bind:value={importData}
							placeholder='Paste JSON data here...'
							rows="6"
							class="w-full mt-2 px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 resize-none font-mono"
						></textarea>
						<button 
							onclick={importContacts} 
							disabled={!importData.trim()}
							class="w-full mt-2 h-10 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md text-sm font-medium transition-colors flex items-center justify-center gap-2"
						>
							<Upload class="w-4 h-4" />
							Import Contacts
						</button>
					</div>
			</div>
		</div>
	</div>
{/if}

<!-- Toast notifications -->
{#if error}
	<div class="fixed bottom-4 right-4 bg-red-950 border border-red-900 text-red-400 px-4 py-3 rounded-md flex items-center gap-3 z-50 text-sm max-w-[90vw]">
		<span>{error}</span>
		<button onclick={() => error = null} class="text-red-400 hover:text-red-300">
			<X class="w-4 h-4" />
		</button>
	</div>
{/if}

{#if success}
	<div class="fixed bottom-4 right-4 bg-green-950 border border-green-900 text-green-400 px-4 py-3 rounded-md flex items-center gap-3 z-50 text-sm max-w-[90vw]">
		<span>{success}</span>
		<button onclick={() => success = null} class="text-green-400 hover:text-green-300">
			<X class="w-4 h-4" />
		</button>
	</div>
{/if}

<div class="min-h-screen bg-zinc-950">
	<!-- Header -->
	<header class="border-b border-zinc-800 px-6 py-3">
		<div class="max-w-4xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-3">
				<Flame class="w-6 h-6 text-orange-500" />
				<div>
					<h1 class="text-sm font-semibold">Arsonnet</h1>
					<p class="text-xs text-zinc-400">Contacts</p>
				</div>
			</div>
			<nav class="flex gap-1">
				<button onclick={() => goto('/network')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Network">
					<Globe class="w-4 h-4" />
				</button>
				<button onclick={() => goto('/chat')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Chat">
					<MessageSquare class="w-4 h-4" />
				</button>
				<button onclick={() => goto('/contacts')} class="p-2 text-sm bg-zinc-800 text-zinc-100 rounded-md" title="Contacts">
					<BookUser class="w-4 h-4" />
				</button>
				<button onclick={() => goto('/vault')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Vault">
					<FolderLock class="w-4 h-4" />
				</button>
				<button onclick={() => { sessionStorage.clear(); goto('/'); }} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Lock">
					<Lock class="w-4 h-4" />
				</button>
			</nav>
		</div>
	</header>

	<main class="max-w-4xl mx-auto p-6 space-y-6">
		<!-- Actions bar -->
		<div class="flex flex-col sm:flex-row gap-4 items-start sm:items-center justify-between">
			<div class="relative flex-1 max-w-md w-full">
				<Search class="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4 text-zinc-500" />
				<input 
					type="text"
					bind:value={searchQuery}
					placeholder="Search contacts..."
					class="w-full h-10 pl-10 pr-4 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500"
				/>
			</div>
			<div class="flex gap-2">
				<button onclick={() => showImportExport = true} class="h-10 px-4 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-sm transition-colors flex items-center gap-2">
					<Download class="w-4 h-4" />
					<span class="hidden sm:inline">Import/Export</span>
				</button>
				<button onclick={() => showAddContact = true} class="h-10 px-4 bg-orange-600 hover:bg-orange-700 rounded-md text-sm font-medium transition-colors flex items-center gap-2">
					<UserPlus class="w-4 h-4" />
					Add Contact
				</button>
			</div>
		</div>

		<!-- Stats -->
		<div class="grid grid-cols-3 gap-4">
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 text-center">
				<p class="text-2xl font-semibold text-orange-500">{contacts.length}</p>
				<p class="text-xs text-zinc-500 mt-1">Total Contacts</p>
			</div>
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 text-center">
				<p class="text-2xl font-semibold text-yellow-500">{contacts.filter(c => c.isFavorite).length}</p>
				<p class="text-xs text-zinc-500 mt-1">Favorites</p>
			</div>
			<div class="bg-zinc-900 border border-zinc-800 rounded-lg p-4 text-center">
				<p class="text-2xl font-semibold text-green-500">{filteredContacts().length}</p>
				<p class="text-xs text-zinc-500 mt-1">Showing</p>
			</div>
		</div>

		<!-- Contacts list -->
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg overflow-hidden">
			{#if filteredContacts().length === 0}
				<div class="p-12 text-center">
					<Users class="w-12 h-12 text-zinc-700 mx-auto mb-3" />
					{#if searchQuery.trim()}
						<p class="text-sm text-zinc-400">No contacts match your search</p>
					{:else}
						<p class="text-sm text-zinc-400">No contacts yet</p>
						<p class="text-xs text-zinc-500 mt-1">Add a contact to start chatting</p>
					{/if}
				</div>
			{:else}
				<div class="divide-y divide-zinc-800">
					{#each filteredContacts() as contact}
						<div class="p-4 hover:bg-zinc-800/50 transition-colors">
							<div class="flex items-start gap-4">
								<div class="relative shrink-0">
									{#if contact.isFavorite}
										<Star class="absolute -top-1 -right-1 w-4 h-4 text-yellow-500 fill-yellow-500 z-10" />
									{/if}
									<div class="w-12 h-12 rounded-lg bg-zinc-800 overflow-hidden flex items-center justify-center">
										<svg width="48" height="48" data-jdenticon-value={contact.pubkey}></svg>
									</div>
								</div>

								<div class="flex-1 min-w-0">
									<div class="flex items-start justify-between gap-2">
										<div>
											<h3 class="font-medium">{contact.name}</h3>
											<button onclick={() => copyPubkey(contact.pubkey)} class="text-xs text-zinc-500 hover:text-zinc-300 font-mono flex items-center gap-1 mt-0.5">
												{shortKey(contact.pubkey)}
												<Copy class="w-3 h-3" />
											</button>
										</div>
										<div class="flex items-center gap-1 shrink-0">
											<button onclick={() => startChat(contact.pubkey)} class="p-2 text-zinc-400 hover:text-orange-500 hover:bg-zinc-800 rounded-md transition-colors" title="Chat">
												<MessageSquare class="w-4 h-4" />
											</button>
											<button onclick={() => toggleFavorite(contact.id)} class="p-2 text-zinc-400 hover:text-yellow-500 hover:bg-zinc-800 rounded-md transition-colors" title="Toggle favorite">
												{#if contact.isFavorite}
													<Star class="w-4 h-4 fill-yellow-500 text-yellow-500" />
												{:else}
													<StarOff class="w-4 h-4" />
												{/if}
											</button>
											<button onclick={() => editingContact = {...contact}} class="p-2 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors" title="Edit">
												<Edit2 class="w-4 h-4" />
											</button>
											<button onclick={() => deleteContact(contact.id)} class="p-2 text-zinc-400 hover:text-red-500 hover:bg-zinc-800 rounded-md transition-colors" title="Delete">
												<Trash2 class="w-4 h-4" />
											</button>
										</div>
									</div>

									<div class="flex flex-wrap gap-x-4 gap-y-1 mt-2 text-xs text-zinc-500">
										{#if contact.email}
											<span class="flex items-center gap-1">
												<Mail class="w-3 h-3" />
												{contact.email}
											</span>
										{/if}
										{#if contact.phone}
											<span class="flex items-center gap-1">
												<Phone class="w-3 h-3" />
												{contact.phone}
											</span>
										{/if}
										<span class="flex items-center gap-1">
											<Calendar class="w-3 h-3" />
											Added {formatDate(contact.added_at)}
										</span>
									</div>

									{#if contact.notes}
										<p class="text-xs text-zinc-400 mt-2 line-clamp-2">{contact.notes}</p>
									{/if}
								</div>
							</div>
						</div>
					{/each}
				</div>
			{/if}
		</div>
	</main>
</div>
