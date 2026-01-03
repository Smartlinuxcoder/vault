<script>
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { Flame, Loader2, Plus, Download, ArrowLeft, Eye, EyeOff, Copy, KeyRound, User, Trash2 } from 'lucide-svelte';
	
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

<div class="min-h-screen bg-zinc-950 flex items-center justify-center p-4">
	<div class="w-full max-w-md bg-zinc-900 border border-zinc-800 rounded-lg p-8">
		<!-- Header -->
		<div class="text-center mb-8">
			<div class="flex justify-center mb-3">
				<Flame class="w-10 h-10 text-orange-500" />
			</div>
			<h1 class="text-xl font-semibold text-zinc-50">Arsonnet</h1>
			<p class="text-sm text-zinc-400 mt-1">Secure, decentralized identity</p>
		</div>

		{#if mode === 'check'}
			<div class="flex flex-col items-center gap-4">
				<Loader2 class="w-8 h-8 text-orange-500 animate-spin" />
				<p class="text-sm text-zinc-400">Checking identity...</p>
			</div>

		{:else if mode === 'choose'}
			<div class="space-y-6">
				<div class="text-center">
					<h2 class="text-lg font-medium">Get Started</h2>
					<p class="text-sm text-zinc-400 mt-1">Create a new identity or import an existing one</p>
				</div>
				
				<div class="space-y-3">
					<button onclick={() => mode = 'create'} class="w-full p-4 bg-orange-600 hover:bg-orange-700 rounded-md text-left transition-colors flex items-center gap-3">
						<Plus class="w-5 h-5" />
						<div>
							<div class="font-medium">Create New Identity</div>
							<p class="text-sm text-orange-100 mt-0.5">Generate a new RSA keypair</p>
						</div>
					</button>
					<button onclick={() => mode = 'import'} class="w-full p-4 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-left transition-colors flex items-center gap-3">
						<Download class="w-5 h-5 text-zinc-400" />
						<div>
							<div class="font-medium">Import Existing</div>
							<p class="text-sm text-zinc-400 mt-0.5">Use your existing private key</p>
						</div>
					</button>
				</div>
			</div>

		{:else if mode === 'login'}
			<div class="space-y-6">
				<div class="flex flex-col items-center gap-3">
					<div class="w-16 h-16 rounded-md bg-zinc-800 border border-zinc-700 flex items-center justify-center overflow-hidden">
						<svg width="64" height="64" data-jdenticon-value={myPubkey}></svg>
					</div>
					<div class="text-center">
						<h2 class="font-medium">Welcome back</h2>
						<p class="text-sm text-zinc-400">{displayName}</p>
					</div>
					<button onclick={() => showPubkey = !showPubkey} class="text-xs text-zinc-500 hover:text-zinc-300 font-mono transition-colors flex items-center gap-1">
						{#if showPubkey}
							<EyeOff class="w-3 h-3" />
							{myPubkey.slice(0, 32) + '...'}
						{:else}
							<Eye class="w-3 h-3" />
							Show public key
						{/if}
					</button>
				</div>

				<div class="space-y-4">
					<p class="text-center text-sm text-zinc-400">Enter your PIN to unlock</p>
					<div class="flex justify-center gap-2">
						{#each Array(6) as _, i}
							<div class="w-3 h-3 rounded-sm {i < pin.length ? 'bg-orange-500' : 'bg-zinc-700'}"></div>
						{/each}
					</div>

					<div class="grid grid-cols-3 gap-2 max-w-[240px] mx-auto">
						{#each ['1','2','3','4','5','6','7','8','9'] as digit}
							<button onclick={() => handlePinInput(digit)} disabled={loading} class="h-12 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-lg font-medium transition-colors disabled:opacity-50">{digit}</button>
						{/each}
						<button onclick={resetIdentity} class="h-12 bg-zinc-800 hover:bg-red-900/50 border border-zinc-700 rounded-md transition-colors flex items-center justify-center">
							<Trash2 class="w-4 h-4 text-zinc-400" />
						</button>
						<button onclick={() => handlePinInput('0')} disabled={loading} class="h-12 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-lg font-medium transition-colors disabled:opacity-50">0</button>
						<button onclick={() => pin = pin.slice(0, -1)} disabled={loading} class="h-12 bg-zinc-800 hover:bg-zinc-700 border border-zinc-700 rounded-md text-lg transition-colors disabled:opacity-50">⌫</button>
					</div>
				</div>

				{#if error}
					<div class="bg-red-950 border border-red-900 rounded-md p-3 text-center text-sm text-red-400">{error}</div>
				{/if}

				<button onclick={loginWithPin} disabled={pin.length < 4 || loading} class="w-full h-10 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 disabled:cursor-not-allowed rounded-md font-medium transition-colors flex items-center justify-center gap-2">
					{#if loading}
						<Loader2 class="w-4 h-4 animate-spin" />
						Unlocking...
					{:else}
						<KeyRound class="w-4 h-4" />
						Unlock
					{/if}
				</button>
			</div>

		{:else if mode === 'create'}
			<div class="space-y-6">
				<button onclick={() => mode = 'choose'} class="text-sm text-zinc-400 hover:text-zinc-200 transition-colors flex items-center gap-1">
					<ArrowLeft class="w-4 h-4" />
					Back
				</button>
				<h2 class="text-lg font-medium">Create New Identity</h2>

				<div class="space-y-4">
					<div class="space-y-2">
						<label class="text-sm font-medium">Display Name</label>
						<input type="text" bind:value={displayName} placeholder="Your name or alias" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
					<div class="space-y-2">
						<label class="text-sm font-medium">Create PIN (min 4 digits)</label>
						<input type="password" bind:value={pin} placeholder="••••" maxlength="6" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
					<div class="space-y-2">
						<label class="text-sm font-medium">Confirm PIN</label>
						<input type="password" bind:value={confirmPin} placeholder="••••" maxlength="6" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
				</div>

				{#if error}
					<div class="bg-red-950 border border-red-900 rounded-md p-3 text-center text-sm text-red-400">{error}</div>
				{/if}

				<button onclick={createIdentity} disabled={loading} class="w-full h-10 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center justify-center gap-2">
					{#if loading}
						<Loader2 class="w-4 h-4 animate-spin" />
						Generating...
					{:else}
						<Plus class="w-4 h-4" />
						Create Identity
					{/if}
				</button>

				<p class="text-xs text-zinc-500 text-center">Your private key will be encrypted with your PIN and stored locally.</p>
			</div>

		{:else if mode === 'import'}
			<div class="space-y-6">
				<button onclick={() => mode = 'choose'} class="text-sm text-zinc-400 hover:text-zinc-200 transition-colors flex items-center gap-1">
					<ArrowLeft class="w-4 h-4" />
					Back
				</button>
				<h2 class="text-lg font-medium">Import Identity</h2>

				<div class="space-y-4">
					<div class="space-y-2">
						<label class="text-sm font-medium">Display Name</label>
						<input type="text" bind:value={displayName} placeholder="Your name or alias" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
					<div class="space-y-2">
						<label class="text-sm font-medium">Private Key (Base64 PKCS8)</label>
						<textarea bind:value={importPrivkey} placeholder="Paste your private key here..." rows="3" class="w-full px-3 py-2 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950 resize-none"></textarea>
					</div>
					<div class="space-y-2">
						<label class="text-sm font-medium">Create PIN (min 4 digits)</label>
						<input type="password" bind:value={pin} placeholder="••••" maxlength="6" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
					<div class="space-y-2">
						<label class="text-sm font-medium">Confirm PIN</label>
						<input type="password" bind:value={confirmPin} placeholder="••••" maxlength="6" class="w-full h-10 px-3 bg-zinc-900 border border-zinc-800 rounded-md text-sm placeholder:text-zinc-500 focus:outline-none focus:ring-2 focus:ring-orange-500 focus:ring-offset-2 focus:ring-offset-zinc-950" />
					</div>
				</div>

				{#if error}
					<div class="bg-red-950 border border-red-900 rounded-md p-3 text-center text-sm text-red-400">{error}</div>
				{/if}

				<button onclick={importIdentity} disabled={loading} class="w-full h-10 bg-orange-600 hover:bg-orange-700 disabled:opacity-50 rounded-md font-medium transition-colors flex items-center justify-center gap-2">
					{#if loading}
						<Loader2 class="w-4 h-4 animate-spin" />
						Importing...
					{:else}
						<Download class="w-4 h-4" />
						Import Identity
					{/if}
				</button>
			</div>
		{/if}
	</div>
</div>
