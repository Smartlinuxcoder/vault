<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { api } from '$lib/api.js';

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

					const { file_id } = await startRes.json();

					for (let i = 0; i < totalChunks; i++) {
						const start = i * CHUNK_SIZE;
						const end = Math.min(start + CHUNK_SIZE, encryptedBytes.length);
						const chunk = encryptedBytes.slice(start, end);

						await api.postRaw(`/api/upload_chunk?token=${sessionToken}&file_id=${file_id}&chunk=${i}`, chunk);
					}

					const finishRes = await api.post('/api/finish_upload', { session_token: sessionToken, file_id });
					const result = await finishRes.json();

					if (result.success && result.item) {
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
					}
				} catch (e) {
					error = e.toString();
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
	<div class="fixed inset-0 bg-black/80 backdrop-blur-sm z-50 flex items-center justify-center p-4" onclick={() => { viewingItem = null; viewContent = null; }}>
		<div class="bg-gray-800 rounded-lg max-w-4xl w-full max-h-[90vh] overflow-hidden" onclick={(e) => e.stopPropagation()}>
			<div class="flex items-center justify-between p-4 border-b border-gray-700">
				<div class="flex items-center gap-3">
					<span class="text-2xl">{getIcon(viewingItem.item_type)}</span>
					<h3 class="font-semibold truncate">{viewingItem.name}</h3>
				</div>
				<button onclick={() => { viewingItem = null; viewContent = null; }} class="p-2 hover:bg-gray-700 rounded-lg transition">✕</button>
			</div>
			<div class="p-4 max-h-[60vh] overflow-auto flex items-center justify-center">
				{#if viewLoading}
					<div class="text-center py-12">
						<div class="w-12 h-12 border-4 border-orange-500 border-t-transparent rounded-full animate-spin mx-auto mb-4"></div>
						<p class="text-gray-400">Decrypting...</p>
					</div>
				{:else if viewContent}
					{#if viewContent.type === 'image'}
						<img src={viewContent.url} alt={viewingItem.name} class="max-w-full max-h-[55vh] object-contain rounded-lg" />
					{:else if viewContent.type === 'video'}
						<video src={viewContent.url} controls autoplay class="max-w-full max-h-[55vh] rounded-lg"></video>
					{:else if viewContent.type === 'audio'}
						<div class="text-center">
							<div class="text-6xl mb-4">🎵</div>
							<audio src={viewContent.url} controls autoplay class="w-full"></audio>
						</div>
					{:else if viewContent.type === 'text'}
						<pre class="bg-gray-900 p-4 rounded-lg overflow-auto max-h-[55vh] w-full text-sm">{viewContent.content}</pre>
					{/if}
				{/if}
			</div>
			<div class="flex items-center justify-between p-4 border-t border-gray-700">
				<span class="text-gray-400 text-sm">{formatSize(viewingItem.size)}</span>
				<button onclick={() => deleteItem(viewingItem.id)} class="px-4 py-2 bg-red-600 hover:bg-red-500 rounded-lg transition">🗑️ Delete</button>
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

<div class="min-h-screen bg-gradient-to-br from-gray-900 via-orange-950 to-gray-900">
	<!-- Header -->
	<header class="bg-gray-800/50 backdrop-blur-xl border-b border-gray-700 px-6 py-4">
		<div class="max-w-6xl mx-auto flex items-center justify-between">
			<div class="flex items-center gap-4">
				<span class="text-3xl">🔐</span>
				<div>
					<h1 class="text-xl font-bold">{vaultName}</h1>
					<p class="text-gray-400 text-sm">{vaultItems.length} encrypted items</p>
				</div>
			</div>
			<div class="flex items-center gap-2">
				<div class="flex bg-gray-700 rounded-lg p-1">
					<button onclick={() => viewMode = 'grid'} class="px-3 py-1 rounded-lg {viewMode === 'grid' ? 'bg-orange-600' : ''} transition">⊞</button>
					<button onclick={() => viewMode = 'list'} class="px-3 py-1 rounded-lg {viewMode === 'list' ? 'bg-orange-600' : ''} transition">☰</button>
				</div>
				<button onclick={handleFileUpload} disabled={uploading} class="px-4 py-2 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded-lg transition">
					{uploading ? '⏳ Uploading...' : '➕ Upload'}
				</button>
				<button onclick={() => goto('/network')} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">🌐 Network</button>
				<button onclick={() => goto('/chat')} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">💬 Chat</button>
				<button onclick={logout} class="px-4 py-2 bg-gray-700 hover:bg-gray-600 rounded-lg transition">🔒</button>
			</div>
		</div>
	</header>

	<main class="max-w-6xl mx-auto p-6">
		{#if loading}
			<div class="text-center py-24">
				<div class="w-12 h-12 border-4 border-orange-500 border-t-transparent rounded-full animate-spin mx-auto mb-4"></div>
				<p class="text-gray-400">Decrypting vault...</p>
			</div>
		{:else if vaultItems.length === 0}
			<div class="text-center py-24">
				<div class="text-6xl mb-4">📂</div>
				<h2 class="text-2xl font-semibold mb-2">Your vault is empty</h2>
				<p class="text-gray-400 mb-6">Upload files to encrypt and store them securely</p>
				<button onclick={handleFileUpload} class="px-6 py-3 bg-orange-600 hover:bg-orange-500 rounded-lg font-semibold transition">📁 Upload Files</button>
			</div>
		{:else}
			<div class={viewMode === 'grid' ? 'grid grid-cols-2 md:grid-cols-4 lg:grid-cols-6 gap-4' : 'space-y-2'}>
				{#each vaultItems as item}
					<button onclick={() => openFile(item)} class={viewMode === 'grid' 
						? 'bg-gray-800/50 backdrop-blur border border-gray-700 rounded-lg p-4 hover:border-orange-500 transition text-left group'
						: 'w-full bg-gray-800/50 backdrop-blur border border-gray-700 rounded-lg p-4 hover:border-orange-500 transition flex items-center gap-4'}>
						{#if item.previewUrl}
							<img src={item.previewUrl} alt="" class={viewMode === 'grid' ? 'w-full aspect-square object-cover rounded-lg mb-2' : 'w-12 h-12 object-cover rounded-lg'} />
						{:else}
							<div class={viewMode === 'grid' ? 'w-full aspect-square bg-gray-700 rounded-lg mb-2 flex items-center justify-center text-3xl' : 'w-12 h-12 bg-gray-700 rounded-lg flex items-center justify-center text-xl'}>
								{getIcon(item.item_type)}
							</div>
						{/if}
						{#if viewMode === 'list'}
							<div class="flex-1 text-left">
								<p class="font-medium truncate">{item.name}</p>
								<p class="text-sm text-gray-400">{item.item_type} • {formatSize(item.size)}</p>
							</div>
						{/if}
					</button>
				{/each}
			</div>
		{/if}
	</main>
</div>
