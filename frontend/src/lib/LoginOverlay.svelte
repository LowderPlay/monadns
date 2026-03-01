<script lang="ts">
  import { auth } from './auth_state.svelte';
  import { toast } from './toast_state.svelte';
  
  let password = $state('');

  function handleSubmit() {
    if (password) {
      auth.setKey(password);
      password = '';
      toast.success('API key updated. Reloading data...');
      window.location.reload(); // Simplest way to retry all pending data loads
    }
  }
</script>

{#if auth.needsLogin}
<div class="fixed inset-0 z-[100] bg-black/90 flex items-center justify-center p-4">
  <div class="bg-zinc-950 border border-zinc-800 p-8 w-full max-w-md space-y-6">
    <div class="text-center">
      <h2 class="text-2xl font-black tracking-tighter">Authentication required</h2>
    </div>

    <form onsubmit={(e) => { e.preventDefault(); handleSubmit(); }} class="space-y-4">
      <div class="flex flex-col gap-1">
        <label for="pwd" class="text-xs font-bold text-zinc-400 uppercase">Enter password</label>
        <input 
          id="pwd"
          type="password" 
          bind:value={password}
          placeholder="Enter password"
          class="bg-zinc-900 border border-zinc-700 p-3 focus:outline-none focus:border-white transition-colors text-white"
          autofocus
        />
      </div>
      <button 
        type="submit" 
        class="w-full bg-white text-black py-3 font-bold hover:bg-zinc-200 transition-colors uppercase tracking-widest"
      >
        Login
      </button>
    </form>
  </div>
</div>
{/if}
