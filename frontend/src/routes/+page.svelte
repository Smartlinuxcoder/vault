<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	
	let mode = $state('check'); // check, login, create, import, choose
	let pin = $state('');
	let confirmPin = $state('');
	let displayName = $state('');
	let importPrivkey = $state('');
	let error = $state(null);
	let loading = $state(false);
	let myPubkey = $state('');
	let showPubkey = $state(false);

	onMount(async () => {
		const identity = localStorage.getItem('p2p_identity');
		if (identity) {
			const parsed = JSON.parse(identity);
			myPubkey = parsed.pubkey;
			displayName = parsed.name;
			mode = 'login';
		} else {
			mode = 'choose';
		}
		updateJdenticon();
	});

	function updateJdenticon() {
		setTimeout(() => {
			if (typeof jdenticon !== 'undefined') jdenticon();
		}, 100);
	}

	$effect(() => {
		if (myPubkey) updateJdenticon();
	});

	async function createIdentity() {
		if (pin.length < 4) {
			error = 'PIN must be at least 4 digits';
			return;
		}
		if (pin !== confirmPin) {
			error = "PINs don't match";
			return;
		}
		if (!displayName.trim()) {
			error = 'Please enter a display name';
			return;
		}

		loading = true;
		error = null;

		try {
			const keyPair = await crypto.subtle.generateKey(
				{ name: 'RSA-OAEP', modulusLength: 2048, publicExponent: new Uint8Array([1, 0, 1]), hash: 'SHA-256' },
				true,
				['encrypt', 'decrypt']
			);

			const pubKeySpki = await crypto.subtle.exportKey('spki', keyPair.publicKey);
			const privKeyPkcs8 = await crypto.subtle.exportKey('pkcs8', keyPair.privateKey);

			const pubkeyB64 = btoa(String.fromCharCode(...new Uint8Array(pubKeySpki)));
			const privkeyB64 = btoa(String.fromCharCode(...new Uint8Array(privKeyPkcs8)));

			const encoder = new TextEncoder();
			const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
			const aesKey = await crypto.subtle.importKey('raw', pinHash, { name: 'AES-GCM' }, false, ['encrypt']);

			const iv = crypto.getRandomValues(new Uint8Array(12));
			const encryptedPrivkey = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, aesKey, encoder.encode(privkeyB64));

			const combined = new Uint8Array(iv.length + encryptedPrivkey.byteLength);
			combined.set(iv);
			combined.set(new Uint8Array(encryptedPrivkey), iv.length);
			const encryptedB64 = btoa(String.fromCharCode(...combined));

			const identity = {
				pubkey: pubkeyB64,
				name: displayName,
				encryptedPrivkey: encryptedB64,
				createdAt: Date.now()
			};
			localStorage.setItem('p2p_identity', JSON.stringify(identity));

			sessionStorage.setItem('p2p_pubkey', pubkeyB64);
			sessionStorage.setItem('p2p_privkey', privkeyB64);
			sessionStorage.setItem('p2p_name', displayName);
			sessionStorage.setItem('user_pin', pin);

			const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkeyB64));
			const hashArray = Array.from(new Uint8Array(hashBuffer));
			const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
			sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
			sessionStorage.setItem('environment', vaultId);

			goto('/network');
		} catch (e) {
			error = e.toString();
		}
		loading = false;
	}

	async function loginWithPin() {
		if (pin.length < 4) {
			error = 'Enter your PIN';
			return;
		}

		loading = true;
		error = null;

		try {
			const identity = JSON.parse(localStorage.getItem('p2p_identity'));
			if (!identity) {
				error = 'No identity found';
				loading = false;
				return;
			}

			const encoder = new TextEncoder();
			const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
			const aesKey = await crypto.subtle.importKey('raw', pinHash, { name: 'AES-GCM' }, false, ['decrypt']);

			const encryptedData = Uint8Array.from(atob(identity.encryptedPrivkey), c => c.charCodeAt(0));
			const iv = encryptedData.slice(0, 12);
			const ciphertext = encryptedData.slice(12);

			const decrypted = await crypto.subtle.decrypt({ name: 'AES-GCM', iv }, aesKey, ciphertext);
			const privkeyB64 = new TextDecoder().decode(decrypted);

			// Verify key is valid
			const privKeyBytes = Uint8Array.from(atob(privkeyB64), c => c.charCodeAt(0));
			await crypto.subtle.importKey('pkcs8', privKeyBytes, { name: 'RSA-OAEP', hash: 'SHA-256' }, false, ['decrypt']);

			sessionStorage.setItem('p2p_pubkey', identity.pubkey);
			sessionStorage.setItem('p2p_privkey', privkeyB64);
			sessionStorage.setItem('p2p_name', identity.name);
			sessionStorage.setItem('user_pin', pin);

			const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(identity.pubkey));
			const hashArray = Array.from(new Uint8Array(hashBuffer));
			const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
			sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
			sessionStorage.setItem('environment', vaultId);

			goto('/network');
		} catch (e) {
			error = 'Invalid PIN';
			pin = '';
		}
		loading = false;
	}

	async function importIdentity() {
		if (pin.length < 4 || pin !== confirmPin) {
			error = 'PINs must match and be at least 4 digits';
			return;
		}
		if (!importPrivkey.trim()) {
			error = 'Please paste your private key';
			return;
		}
		if (!displayName.trim()) {
			error = 'Please enter a display name';
			return;
		}

		loading = true;
		error = null;

		try {
			const privkeyB64 = importPrivkey.trim();
			const privKeyBytes = Uint8Array.from(atob(privkeyB64), c => c.charCodeAt(0));

			const privateKey = await crypto.subtle.importKey('pkcs8', privKeyBytes, { name: 'RSA-OAEP', hash: 'SHA-256' }, true, ['decrypt']);

			const jwk = await crypto.subtle.exportKey('jwk', privateKey);
			delete jwk.d; delete jwk.p; delete jwk.q; delete jwk.dp; delete jwk.dq; delete jwk.qi;
			jwk.key_ops = ['encrypt'];

			const publicKey = await crypto.subtle.importKey('jwk', jwk, { name: 'RSA-OAEP', hash: 'SHA-256' }, true, ['encrypt']);
			const pubKeySpki = await crypto.subtle.exportKey('spki', publicKey);
			const pubkeyB64 = btoa(String.fromCharCode(...new Uint8Array(pubKeySpki)));

			const encoder = new TextEncoder();
			const pinHash = await crypto.subtle.digest('SHA-256', encoder.encode(pin));
			const aesKey = await crypto.subtle.importKey('raw', pinHash, { name: 'AES-GCM' }, false, ['encrypt']);

			const iv = crypto.getRandomValues(new Uint8Array(12));
			const encryptedPrivkey = await crypto.subtle.encrypt({ name: 'AES-GCM', iv }, aesKey, encoder.encode(privkeyB64));

			const combined = new Uint8Array(iv.length + encryptedPrivkey.byteLength);
			combined.set(iv);
			combined.set(new Uint8Array(encryptedPrivkey), iv.length);
			const encryptedB64 = btoa(String.fromCharCode(...combined));

			const identity = {
				pubkey: pubkeyB64,
				name: displayName,
				encryptedPrivkey: encryptedB64,
				createdAt: Date.now()
			};
			localStorage.setItem('p2p_identity', JSON.stringify(identity));

			sessionStorage.setItem('p2p_pubkey', pubkeyB64);
			sessionStorage.setItem('p2p_privkey', privkeyB64);
			sessionStorage.setItem('p2p_name', displayName);
			sessionStorage.setItem('user_pin', pin);

			const hashBuffer = await crypto.subtle.digest('SHA-256', encoder.encode(pubkeyB64));
			const hashArray = Array.from(new Uint8Array(hashBuffer));
			const vaultId = hashArray.slice(0, 8).map(b => b.toString(16).padStart(2, '0')).join('');
			sessionStorage.setItem('session_token', vaultId + '_' + Date.now());
			sessionStorage.setItem('environment', vaultId);

			goto('/network');
		} catch (e) {
			error = 'Invalid private key: ' + e.toString();
		}
		loading = false;
	}

	function handlePinInput(digit) {
		if (pin.length < 6) pin += digit;
	}

	function resetIdentity() {
		localStorage.removeItem('p2p_identity');
		mode = 'choose';
	}
</script>

<div class="min-h-screen bg-gradient-to-br from-gray-900 via-orange-950 to-gray-900 flex items-center justify-center p-4">
	<div class="w-full max-w-md bg-gray-800/50 backdrop-blur-xl rounded-lg border border-gray-700 p-8">
		<!-- Header -->
		<div class="text-center mb-8">
			<div class="text-5xl mb-4">🔥</div>
			<h1 class="text-2xl font-bold text-white">Arsonnet Identity</h1>
			<p class="text-gray-400">Secure, decentralized identity management</p>
		</div>

		{#if mode === 'check'}
			<div class="flex flex-col items-center gap-4">
				<div class="w-12 h-12 border-4 border-orange-500 border-t-transparent rounded-full animate-spin"></div>
				<p class="text-gray-400">Checking identity...</p>
			</div>

		{:else if mode === 'choose'}
			<div class="space-y-6">
				<h2 class="text-xl font-semibold text-center">Get Started</h2>
				<p class="text-gray-400 text-center">Create a new identity or import an existing one</p>
				
				<div class="grid gap-4">
					<button onclick={() => mode = 'create'} class="p-6 bg-gradient-to-r from-orange-600 to-red-600 rounded-lg hover:opacity-90 transition text-left">
						<div class="text-2xl mb-2">✨</div>
						<div class="font-semibold">Create New Identity</div>
						<p class="text-sm text-gray-200">Generate a new RSA keypair</p>
					</button>
					<button onclick={() => mode = 'import'} class="p-6 bg-gray-700 rounded-lg hover:bg-gray-600 transition text-left">
						<div class="text-2xl mb-2">📥</div>
						<div class="font-semibold">Import Existing</div>
						<p class="text-sm text-gray-400">Use your existing private key</p>
					</button>
				</div>
			</div>

		{:else if mode === 'login'}
			<div class="space-y-6">
				<div class="flex flex-col items-center gap-4">
					<div class="w-20 h-20 rounded-lg bg-gray-700 flex items-center justify-center overflow-hidden">
						<svg width="80" height="80" data-jdenticon-value={myPubkey}></svg>
					</div>
					<h2 class="text-xl font-semibold">Welcome back!</h2>
					<p class="text-gray-400">{displayName}</p>
					<button onclick={() => showPubkey = !showPubkey} class="text-xs text-gray-500 hover:text-gray-300 font-mono">
						{showPubkey ? myPubkey.slice(0, 40) + '...' : 'Click to show public key'}
					</button>
				</div>

				<div class="space-y-4">
					<p class="text-center text-gray-400">Enter your PIN to unlock</p>
					<div class="flex justify-center gap-2">
						{#each Array(6) as _, i}
							<div class="w-4 h-4 rounded-sm {i < pin.length ? 'bg-orange-500' : 'bg-gray-600'}"></div>
						{/each}
					</div>

					<div class="grid grid-cols-3 gap-2 max-w-xs mx-auto">
						{#each ['1','2','3','4','5','6','7','8','9'] as digit}
							<button onclick={() => handlePinInput(digit)} disabled={loading} class="p-4 bg-gray-700 rounded-lg hover:bg-gray-600 text-xl font-semibold transition">{digit}</button>
						{/each}
						<button onclick={resetIdentity} class="p-4 bg-red-900/50 rounded-lg hover:bg-red-800/50 text-sm transition">Reset</button>
						<button onclick={() => handlePinInput('0')} disabled={loading} class="p-4 bg-gray-700 rounded-lg hover:bg-gray-600 text-xl font-semibold transition">0</button>
						<button onclick={() => pin = pin.slice(0, -1)} disabled={loading} class="p-4 bg-gray-700 rounded-lg hover:bg-gray-600 text-xl transition">⌫</button>
					</div>
				</div>

				{#if error}
					<div class="bg-red-500/20 border border-red-500 rounded-lg p-3 text-center text-red-300">{error}</div>
				{/if}

				<button onclick={loginWithPin} disabled={pin.length < 4 || loading} class="w-full py-3 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 disabled:cursor-not-allowed rounded-lg font-semibold transition">
					{loading ? '🔓 Unlocking...' : '🔓 Unlock'}
				</button>
			</div>

		{:else if mode === 'create'}
			<div class="space-y-6">
				<button onclick={() => mode = 'choose'} class="text-gray-400 hover:text-white transition">← Back</button>
				<h2 class="text-xl font-semibold">Create New Identity</h2>

				<div class="space-y-4">
					<div>
						<label class="block text-sm text-gray-400 mb-1">Display Name</label>
						<input type="text" bind:value={displayName} placeholder="Your name or alias" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
					<div>
						<label class="block text-sm text-gray-400 mb-1">Create PIN (min 4 digits)</label>
						<input type="password" bind:value={pin} placeholder="Enter PIN" maxlength="6" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
					<div>
						<label class="block text-sm text-gray-400 mb-1">Confirm PIN</label>
						<input type="password" bind:value={confirmPin} placeholder="Confirm PIN" maxlength="6" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
				</div>

				{#if error}
					<div class="bg-red-500/20 border border-red-500 rounded-lg p-3 text-center text-red-300">{error}</div>
				{/if}

				<button onclick={createIdentity} disabled={loading} class="w-full py-3 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded-lg font-semibold transition">
					{loading ? '🔐 Generating keypair...' : '🔐 Create Identity'}
				</button>

				<p class="text-xs text-gray-500 text-center">⚠️ Your private key will be encrypted with your PIN and stored locally. Make sure to backup your key after creation!</p>
			</div>

		{:else if mode === 'import'}
			<div class="space-y-6">
				<button onclick={() => mode = 'choose'} class="text-gray-400 hover:text-white transition">← Back</button>
				<h2 class="text-xl font-semibold">Import Identity</h2>

				<div class="space-y-4">
					<div>
						<label class="block text-sm text-gray-400 mb-1">Display Name</label>
						<input type="text" bind:value={displayName} placeholder="Your name or alias" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
					<div>
						<label class="block text-sm text-gray-400 mb-1">Private Key (Base64 PKCS8)</label>
						<textarea bind:value={importPrivkey} placeholder="Paste your private key here..." rows="4" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none resize-none"></textarea>
					</div>
					<div>
						<label class="block text-sm text-gray-400 mb-1">Create PIN (min 4 digits)</label>
						<input type="password" bind:value={pin} placeholder="Enter PIN" maxlength="6" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
					<div>
						<label class="block text-sm text-gray-400 mb-1">Confirm PIN</label>
						<input type="password" bind:value={confirmPin} placeholder="Confirm PIN" maxlength="6" class="w-full p-3 bg-gray-700 rounded-lg border border-gray-600 focus:border-orange-500 focus:outline-none" />
					</div>
				</div>

				{#if error}
					<div class="bg-red-500/20 border border-red-500 rounded-lg p-3 text-center text-red-300">{error}</div>
				{/if}

				<button onclick={importIdentity} disabled={loading} class="w-full py-3 bg-orange-600 hover:bg-orange-500 disabled:opacity-50 rounded-lg font-semibold transition">
					{loading ? '📥 Importing...' : '📥 Import Identity'}
				</button>
			</div>
		{/if}
	</div>
</div>
