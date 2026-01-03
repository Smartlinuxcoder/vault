<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';
	import { Flame, Globe, MessageSquare, FolderLock, Lock, Upload, Grid3x3, List, Loader2, X, Trash2, Image, Video, Music, FileText, File, Key } from 'lucide-svelte';

	let vaultItems = $state([]);
	let vaultName = $state('');
	let loading = $state(true);
	let error = $state(null);
	let uploading = $state(false);
	let viewMode = $state('grid');
	let viewingItem = $state(null);
	let viewContent = $state(null);
	let viewLoading = $state(false);

	let sessionToken = '';
	let userPin = '';
	let environment = '';

	onMount(async () => {
		const pubkey = sessionStorage.getItem('p2p_pubkey');
		const pin = sessionStorage.getItem('user_pin');
		const name = sessionStorage.getItem('p2p_name');

		if (!pubkey || !pin) {
			goto('/');
			return;
		}

		userPin = pin;
		vaultName = `${name || 'My'}'s Vault`;

		const encoder = new TextEncoder();
		const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkey));
		const hashArray = Array.from(new Uint8Array(hashBuffer));
		const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
		
		sessionToken = vaultId + '_' + Date.now();
		environment = vaultId;

		await loadVault();
	});

	async function loadVault() {
		loading = true;
		try {
			const res = await api.fetch(`/api/metadata/${environment}`);
			const encryptedBlob = new Uint8Array(await res.arrayBuffer());

			if (encryptedBlob.length === 0) {
				loading = false;
				return;
			}

			const encoder = new TextEncoder();
			const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(userPin));
			const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['decrypt']);

			const metaNonce = encryptedBlob.slice(0, 12);
			const metaEnc = encryptedBlob.slice(12);

			const decryptedMetaBytes = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: metaNonce }, cryptoKey, metaEnc);
			const metadata = JSON.parse(new TextDecoder().decode(decryptedMetaBytes));

			const decryptedItems = [];
			for (const item of metadata.items) {
				try {
					const encName = new Uint8Array(item.encrypted_name);
					const nameNonce = new Uint8Array(item.name_nonce);
					const decryptedNameBytes = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nameNonce }, cryptoKey, encName);
					const name = new TextDecoder().decode(decryptedNameBytes);

					decryptedItems.push({
						...item,
						name,
						nonce: item.nonce,
						previewUrl: null
					});
				} catch (e) {
					decryptedItems.push({ ...item, name: '[Encrypted]', previewUrl: null });
				}
			}

			vaultItems = decryptedItems;
			if (metadata.name) vaultName = metadata.name;
		} catch (e) {
			error = e.toString();
		}
		loading = false;
		loadPreviews();
	}

	async function loadPreviews() {
		const encoder = new TextEncoder();
		const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(userPin));
		const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['decrypt']);

		for (const item of vaultItems) {
			if ((item.item_type === 'photo' || item.item_type === 'video') && item.preview_id && !item.previewUrl) {
				try {
					const res = await api.fetch(`/api/get_preview/${environment}/${item.preview_id}`);
					const previewData = new Uint8Array(await res.arrayBuffer());
					const nonce = previewData.slice(0, 12);
					const encryptedBytes = previewData.slice(12);

					const decrypted = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, cryptoKey, encryptedBytes);
					const blob = new Blob([decrypted], { type: 'image/jpeg' });
					
					vaultItems = vaultItems.map(i => i.id === item.id ? { ...i, previewUrl: URL.createObjectURL(blob) } : i);
				} catch (e) {
					console.error('Preview load failed', e);
				}
			}
		}
	}

	async function saveVaultState() {
		const exportItems = vaultItems.map(i => ({
			id: i.id,
			encrypted_name: i.encrypted_name,
			name_nonce: i.name_nonce,
			item_type: i.item_type,
			size: i.size,
			nonce: i.nonce,
			content_id: i.content_id,
			preview_id: i.preview_id
		}));

		const metadata = { name: vaultName, items: exportItems };
		const jsonStr = JSON.stringify(metadata);

		const encoder = new TextEncoder();
		const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(userPin));
		const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['encrypt']);

		const nonce = crypto.getRandomValues(new Uint8Array(12));
		const encrypted = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, cryptoKey, encoder.encode(jsonStr));

		const combined = new Uint8Array(nonce.byteLength + encrypted.byteLength);
		combined.set(nonce, 0);
		combined.set(new Uint8Array(encrypted), nonce.byteLength);

		await api.postRaw(`/api/metadata/${environment}`, combined);
	}

	async function handleFileUpload() {
		const input = document.createElement('input');
		input.type = 'file';
		input.multiple = true;
		input.click();

		input.onchange = async () => {
			const files = Array.from(input.files);
			if (files.length === 0) return;

			uploading = true;

			const encoder = new TextEncoder();
			const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(userPin));
			const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['encrypt']);

			const CHUNK_SIZE = 1024 * 1024;

			for (const file of files) {
				try {
					const arrayBuffer = await file.arrayBuffer();
					const data = new Uint8Array(arrayBuffer);

					const fileNonce = crypto.getRandomValues(new Uint8Array(12));
					const nameNonce = crypto.getRandomValues(new Uint8Array(12));

					const encryptedFile = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: fileNonce }, cryptoKey, data);
					const encryptedBytes = new Uint8Array(encryptedFile);

					const nameBytes = encoder.encode(file.name);
					const encryptedName = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nameNonce }, cryptoKey, nameBytes);

					const nameArr = Array.from(new Uint8Array(encryptedName));
					const fileNonceArr = Array.from(fileNonce);
					const nameNonceArr = Array.from(nameNonce);

					let itemType = 'document';
					const ext = file.name.split('.').pop().toLowerCase();
					if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) itemType = 'photo';
					else if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(ext)) itemType = 'video';
					else if (['mp3', 'wav', 'ogg', 'm4a', 'flac'].includes(ext)) itemType = 'audio';
					else if (['txt', 'md', 'log', 'rs', 'js', 'ts', 'html', 'css', 'json', 'toml', 'yaml', 'xml', 'c', 'cpp', 'h', 'py', 'sh', 'bat'].includes(ext)) itemType = 'text';
					else if (['key', 'pem', 'env'].includes(ext)) itemType = 'password';

					let previewArr = null;
					let previewNonceArr = null;

					if (itemType === 'photo' || itemType === 'video') {
						const previewData = await generatePreview(file, itemType);
						if (previewData) {
							const previewNonce = crypto.getRandomValues(new Uint8Array(12));
							const encryptedPreview = await crypto.subtle.encrypt({ name: 'AES-GCM', iv: previewNonce }, cryptoKey, previewData);
							previewArr = Array.from(new Uint8Array(encryptedPreview));
							previewNonceArr = Array.from(previewNonce);
						}
					}

					const totalChunks = Math.ceil(encryptedBytes.length / CHUNK_SIZE);

					const startRes = await api.post('/api/start_upload', {
						session_token: sessionToken,
						encrypted_name: nameArr,
						name_nonce: nameNonceArr,
						item_type: itemType,
						nonce: fileNonceArr,
						total_chunks: totalChunks,
						preview: previewArr,
						preview_nonce: previewNonceArr
					});

					if (!startRes.ok) {
						const errText = await startRes.text();
						throw new Error(`Upload start failed: ${errText}`);
					}

					const startData = await startRes.json();
					if (!startData || !startData.file_id) {
						throw new Error('Invalid server response: missing file_id');
					}
					const file_id = startData.file_id;

					for (let i = 0; i < totalChunks; i++) {
						const start = i * CHUNK_SIZE;
						const end = Math.min(start + CHUNK_SIZE, encryptedBytes.length);
						const chunk = encryptedBytes.slice(start, end);

						const chunkRes = await api.postRaw(`/api/upload_chunk?token=${sessionToken}&file_id=${file_id}&chunk=${i}`, chunk);
						if (!chunkRes.ok) {
							throw new Error(`Chunk ${i} upload failed`);
						}
					}

					const finishRes = await api.post('/api/finish_upload', { session_token: sessionToken, file_id });
					if (!finishRes.ok) {
						const errText = await finishRes.text();
						throw new Error(`Upload finish failed: ${errText}`);
					}

					const result = await finishRes.json();

					if (result && result.success && result.item) {
						vaultItems = [...vaultItems, {
							id: result.item.id,
							name: file.name,
							encrypted_name: result.item.encrypted_name,
							item_type: itemType,
							size: result.item.size,
							nonce: fileNonceArr,
							name_nonce: nameNonceArr,
							content_id: result.item.content_id,
							preview_id: result.item.preview_id,
							previewUrl: null
						}];
					} else {
						throw new Error('Upload completed but item data is missing');
					}
				} catch (e) {
					error = e.toString();
					console.error('Upload error:', e);
				}
			}

			await saveVaultState();
			uploading = false;
			loadPreviews();
		};
	}

	async function generatePreview(file, itemType) {
		return new Promise((resolve) => {
			const timeout = setTimeout(() => resolve(null), 15000);

			if (itemType === 'photo') {
				const img = new Image();
				img.onload = () => {
					clearTimeout(timeout);
					const canvas = document.createElement('canvas');
					canvas.width = 320;
					canvas.height = 320;
					const ctx = canvas.getContext('2d');
					const size = Math.min(img.width, img.height);
					const x = (img.width - size) / 2;
					const y = (img.height - size) / 2;
					ctx.drawImage(img, x, y, size, size, 0, 0, 320, 320);
					canvas.toBlob((blob) => {
						URL.revokeObjectURL(img.src);
						if (blob) blob.arrayBuffer().then(ab => resolve(new Uint8Array(ab)));
						else resolve(null);
					}, 'image/jpeg', 0.7);
				};
				img.onerror = () => { clearTimeout(timeout); resolve(null); };
				img.src = URL.createObjectURL(file);
			} else if (itemType === 'video') {
				const video = document.createElement('video');
				video.src = URL.createObjectURL(file);
				video.muted = true;
				video.playsInline = true;
				video.onloadedmetadata = () => {
					video.currentTime = Math.min(1, video.duration / 2);
				};
				video.onseeked = () => {
					clearTimeout(timeout);
					const canvas = document.createElement('canvas');
					canvas.width = 320;
					canvas.height = 320;
					const ctx = canvas.getContext('2d');
					const size = Math.min(video.videoWidth, video.videoHeight);
					const x = (video.videoWidth - size) / 2;
					const y = (video.videoHeight - size) / 2;
					ctx.drawImage(video, x, y, size, size, 0, 0, 320, 320);
					canvas.toBlob((blob) => {
						URL.revokeObjectURL(video.src);
						if (blob) blob.arrayBuffer().then(ab => resolve(new Uint8Array(ab)));
						else resolve(null);
					}, 'image/jpeg', 0.7);
				};
				video.onerror = () => { clearTimeout(timeout); resolve(null); };
			} else {
				resolve(null);
			}
		});
	}

	async function openFile(item) {
		viewLoading = true;
		viewingItem = item;
		viewContent = null;

		try {
			const res = await api.fetch(`/api/get_file/${environment}/${item.content_id}`);
			const encryptedBytes = new Uint8Array(await res.arrayBuffer());

			const encoder = new TextEncoder();
			const keyHash = await crypto.subtle.digest('SHA-256', encoder.encode(userPin));
			const cryptoKey = await crypto.subtle.importKey('raw', keyHash, { name: 'AES-GCM' }, false, ['decrypt']);

			const nonce = new Uint8Array(item.nonce);
			const decrypted = await crypto.subtle.decrypt({ name: 'AES-GCM', iv: nonce }, cryptoKey, encryptedBytes);
			const data = new Uint8Array(decrypted);

			const ext = item.name.split('.').pop().toLowerCase();

			if (['jpg', 'jpeg', 'png', 'gif', 'webp', 'bmp', 'svg'].includes(ext)) {
				const mimeTypes = { jpg: 'image/jpeg', jpeg: 'image/jpeg', png: 'image/png', gif: 'image/gif', webp: 'image/webp', bmp: 'image/bmp', svg: 'image/svg+xml' };
				const blob = new Blob([data], { type: mimeTypes[ext] || 'image/png' });
				viewContent = { type: 'image', url: URL.createObjectURL(blob) };
			} else if (['mp4', 'webm', 'mov', 'avi', 'mkv'].includes(ext)) {
				const mimeTypes = { mp4: 'video/mp4', webm: 'video/webm', mov: 'video/quicktime', avi: 'video/x-msvideo', mkv: 'video/x-matroska' };
				const blob = new Blob([data], { type: mimeTypes[ext] || 'video/mp4' });
				viewContent = { type: 'video', url: URL.createObjectURL(blob) };
			} else if (['mp3', 'wav', 'ogg', 'm4a', 'flac'].includes(ext)) {
				const mimeTypes = { mp3: 'audio/mpeg', wav: 'audio/wav', ogg: 'audio/ogg', m4a: 'audio/mp4', flac: 'audio/flac' };
				const blob = new Blob([data], { type: mimeTypes[ext] || 'audio/mpeg' });
				viewContent = { type: 'audio', url: URL.createObjectURL(blob) };
			} else if (['txt', 'md', 'log', 'rs', 'js', 'ts', 'html', 'css', 'json', 'toml', 'yaml', 'xml', 'c', 'cpp', 'h', 'py', 'sh', 'bat', 'key', 'pem', 'env'].includes(ext)) {
				viewContent = { type: 'text', content: new TextDecoder().decode(data) };
			} else {
				const blob = new Blob([data]);
				const url = URL.createObjectURL(blob);
				const a = document.createElement('a');
				a.href = url;
				a.download = item.name;
				a.click();
				URL.revokeObjectURL(url);
				viewingItem = null;
			}
		} catch (e) {
			error = e.toString();
			viewingItem = null;
		}
		viewLoading = false;
	}

	async function deleteItem(itemId) {
		const item = vaultItems.find(i => i.id === itemId);
		if (!item) return;

		const filesToDelete = [item.content_id];
		if (item.preview_id) filesToDelete.push(item.preview_id);

		await api.post('/api/delete_files', { session_token: sessionToken, file_ids: filesToDelete });

		vaultItems = vaultItems.filter(i => i.id !== itemId);
		viewingItem = null;
		viewContent = null;
		await saveVaultState();
	}

	function getIcon(itemType) {
		const icons = { photo: '🖼️', video: '🎬', audio: '🎵', text: '📝', document: '📄', password: '🔑', note: '📝' };
		return icons[itemType] || '📦';
	}

	function formatSize(size) {
		if (size < 1024) return `${size} B`;
		if (size < 1024 * 1024) return `${(size / 1024).toFixed(1)} KB`;
		return `${(size / (1024 * 1024)).toFixed(1)} MB`;
	}

	function logout() {
		sessionStorage.clear();
		goto('/');
	}
</script>

<!-- Modal Viewer -->
{#if viewingItem}
	<div class="fixed inset-0 bg-black/80 z-50 flex items-center justify-center p-4" onclick={() => { viewingItem = null; viewContent = null; }}>
		<div class="bg-zinc-900 border border-zinc-800 rounded-lg max-w-4xl w-full max-h-[90vh] overflow-hidden" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between p-4 border-b border-zinc-800">
				<div class="flex items-center gap-3">
					{#if viewingItem.item_type === 'photo'}
						<Image class="w-5 h-5 text-zinc-400" />
					{:else if viewingItem.item_type === 'video'}
						<Video class="w-5 h-5 text-zinc-400" />
					{:else if viewingItem.item_type === 'audio'}
						<Music class="w-5 h-5 text-zinc-400" />
					{:else if viewingItem.item_type === 'text'}
						<FileText class="w-5 h-5 text-zinc-400" />
					{:else if viewingItem.item_type === 'password'}
						<Key class="w-5 h-5 text-zinc-400" />
					{:else}
						<File class="w-5 h-5 text-zinc-400" />
					{/if}
					<h3 class="text-sm font-medium truncate">{viewingItem.name}</h3>
				</div>
				<button onclick={() => { viewingItem = null; viewContent = null; }} class="p-1.5 text-zinc-400 hover:text-zinc-200 hover:bg-zinc-800 rounded-md transition-colors">
					<X class="w-4 h-4" />
				</button>
			</div>
			<div class="p-4 max-h-[60vh] overflow-auto flex items-center justify-center">
				{#if viewLoading}
					<div class="text-center py-12">
						<Loader2 class="w-6 h-6 text-orange-500 animate-spin mx-auto mb-3" />
						<p class="text-sm text-zinc-400">Decrypting...</p>
					</div>
				{:else if viewContent}
					{#if viewContent.type === 'image'}
						<img src={viewContent.url} alt={viewingItem.name} class="max-w-full max-h-[55vh] object-contain rounded-md" />
					{:else if viewContent.type === 'video'}
						<video src={viewContent.url} controls autoplay class="max-w-full max-h-[55vh] rounded-md">
							<track kind="captions" />
						</video>
					{:else if viewContent.type === 'audio'}
						<div class="text-center w-full">
							<Music class="w-12 h-12 text-zinc-600 mx-auto mb-4" />
							<audio src={viewContent.url} controls autoplay class="w-full"></audio>
						</div>
					{:else if viewContent.type === 'text'}
						<pre class="bg-zinc-950 border border-zinc-800 p-4 rounded-md overflow-auto max-h-[55vh] w-full text-sm font-mono">{viewContent.content}</pre>
					{/if}
				{/if}
			</div>
			<div class="flex items-center justify-between p-4 border-t border-zinc-800">
				<span class="text-xs text-zinc-500">{formatSize(viewingItem.size)}</span>
				<button onclick={() => deleteItem(viewingItem.id)} class="h-8 px-3 text-sm bg-red-900 hover:bg-red-800 border border-red-800 rounded-md transition-colors flex items-center gap-1.5">
					<Trash2 class="w-3.5 h-3.5" />
					Delete
				</button>
			</div>
		</div>
	</div>
{/if}

<!-- Error Toast -->
{#if error}
	<div class="fixed bottom-4 right-4 bg-red-950 border border-red-900 text-red-400 px-4 py-3 rounded-md flex items-center gap-3 z-50 text-sm">
		<span>{error}</span>
		<button onclick={() => error = null} class="text-red-400 hover:text-red-300">
			<X class="w-4 h-4" />
		</button>
	</div>
{/if}

<div class="min-h-screen bg-zinc-950">
	<!-- Header -->
	<header class="border-b border-zinc-800 px-6 py-3">
		<div class="max-w-5xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-3">
				<Flame class="w-6 h-6 text-orange-500" />
				<div>
					<h1 class="text-sm font-semibold">Arsonnet</h1>
					<p class="text-xs text-zinc-400">{vaultItems.length} encrypted items</p>
				</div>
			</div>
			<div class="flex items-center gap-2">
				<div class="flex bg-zinc-800 border border-zinc-700 rounded-md p-0.5">
					<button onclick={() => viewMode = 'grid'} class="p-1.5 rounded {viewMode === 'grid' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-400'} transition-colors">
						<Grid3x3 class="w-4 h-4" />
					</button>
					<button onclick={() => viewMode = 'list'} class="p-1.5 rounded {viewMode === 'list' ? 'bg-zinc-700 text-zinc-100' : 'text-zinc-400'} transition-colors">
						<List class="w-4 h-4" />
					</button>
				</div>
				<button onclick={handleFileUpload} disabled={uploading} class="h-8 px-3 text-sm bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center gap-1.5">
					{#if uploading}
						<Loader2 class="w-4 h-4 animate-spin" />
						Uploading...
					{:else}
						<Upload class="w-4 h-4" />
						Upload
					{/if}
				</button>
				<nav class="flex gap-1 ml-2">
					<button onclick={() => goto('/network')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Network">
						<Globe class="w-4 h-4" />
					</button>
					<button onclick={() => goto('/chat')} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Chat">
						<MessageSquare class="w-4 h-4" />
					</button>
					<button onclick={() => goto('/vault')} class="p-2 text-sm bg-zinc-800 text-zinc-100 rounded-md" title="Vault">
						<FolderLock class="w-4 h-4" />
					</button>
					<button onclick={logout} class="p-2 text-sm text-zinc-400 hover:text-zinc-100 hover:bg-zinc-800 rounded-md transition-colors" title="Lock">
						<Lock class="w-4 h-4" />
					</button>
				</nav>
			</div>
		</div>
	</header>

	<main class="max-w-5xl mx-auto p-6">
		{#if loading}
			<div class="text-center py-16">
				<Loader2 class="w-6 h-6 text-orange-500 animate-spin mx-auto mb-3" />
				<p class="text-sm text-zinc-400">Decrypting vault...</p>
			</div>
		{:else if vaultItems.length === 0}
			<div class="text-center py-16">
				<FolderLock class="w-12 h-12 text-zinc-700 mx-auto mb-4" />
				<p class="text-sm text-zinc-400 mb-4">Your vault is empty</p>
				<button onclick={handleFileUpload} class="h-9 px-4 bg-orange-600 hover:bg-orange-700 rounded-md text-sm font-medium transition-colors flex items-center gap-2 mx-auto">
					<Upload class="w-4 h-4" />
					Upload Files
				</button>
			</div>
		{:else}
			<div class={viewMode === 'grid' ? 'grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-3' : 'space-y-2'}>
				{#each vaultItems as item}
					<button onclick={() => openFile(item)} class={viewMode === 'grid' 
						? 'bg-zinc-900 border border-zinc-800 rounded-md p-3 hover:border-zinc-700 transition-colors text-left'
						: 'w-full bg-zinc-900 border border-zinc-800 rounded-md p-3 hover:border-zinc-700 transition-colors flex items-center gap-3'}>
						{#if item.previewUrl}
							<img src={item.previewUrl} alt="" class={viewMode === 'grid' ? 'w-full aspect-square object-cover rounded-md mb-2' : 'w-10 h-10 object-cover rounded-md'} />
						{:else}
							<div class={viewMode === 'grid' ? 'w-full aspect-square bg-zinc-800 rounded-md mb-2 flex items-center justify-center' : 'w-10 h-10 bg-zinc-800 rounded-md flex items-center justify-center'}>
								{#if item.item_type === 'photo'}
									<Image class="w-6 h-6 text-zinc-500" />
								{:else if item.item_type === 'video'}
									<Video class="w-6 h-6 text-zinc-500" />
								{:else if item.item_type === 'audio'}
									<Music class="w-6 h-6 text-zinc-500" />
								{:else if item.item_type === 'text'}
									<FileText class="w-6 h-6 text-zinc-500" />
								{:else if item.item_type === 'password'}
									<Key class="w-6 h-6 text-zinc-500" />
								{:else}
									<File class="w-6 h-6 text-zinc-500" />
								{/if}
							</div>
						{/if}
						{#if viewMode === 'grid'}
							<p class="text-xs text-zinc-300 truncate">{item.name}</p>
						{:else}
							<div class="flex-1 text-left min-w-0">
								<p class="text-sm font-medium truncate">{item.name}</p>
								<p class="text-xs text-zinc-500">{item.item_type} • {formatSize(item.size)}</p>
							</div>
						{/if}
					</button>
				{/each}
			</div>
		{/if}
	</main>
</div>
