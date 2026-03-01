<script lang="ts">
  import ConfigTab from './lib/ConfigTab.svelte';
  import DomainListsTab from './lib/DomainListsTab.svelte';
  import DomainsTab from './lib/DomainsTab.svelte';
  import Toast from "./lib/Toast.svelte";
  import Icon from "./assets/Icon.svelte";
  import LoginOverlay from "./lib/LoginOverlay.svelte";
  import { auth } from "./lib/auth_state.svelte";

  type Tab = 'config' | 'lists' | 'domains';
  let activeTab = $state<Tab>('config');

  const tabs: { id: Tab; label: string }[] = [
    { id: 'config', label: 'Configuration' },
    { id: 'lists', label: 'Domain Lists' },
    { id: 'domains', label: 'Domains' },
  ];
</script>

<div class="min-h-screen bg-[#0a0a0a] text-[#e5e5e5] flex flex-col items-center p-4 md:p-8">
  <div class="w-full max-w-5xl">
    <!-- Header -->
    <header class="flex items-center justify-between mb-8">
      <div class="flex items-center gap-4">
        <Icon />
        <h1 class="text-3xl font-black tracking-tighter">MonaDNS</h1>
      </div>
      {#if auth.key}
        <button 
          onclick={() => auth.logout()} 
          class="text-xs font-bold uppercase tracking-widest text-zinc-500 hover:text-white transition-colors border border-zinc-800 px-4 py-2"
        >
          Logout
        </button>
      {/if}
    </header>

    <!-- Navigation -->
    <nav class="flex border-b border-zinc-800 mb-8">
      {#each tabs as tab}
        <button
          onclick={() => activeTab = tab.id}
          class="px-6 py-3 font-bold transition-colors relative {activeTab === tab.id ? 'text-white' : 'text-zinc-500 hover:text-zinc-300'}"
        >
          {tab.label}
          {#if activeTab === tab.id}
            <div class="absolute bottom-0 left-0 right-0 h-1 bg-white"></div>
          {/if}
        </button>
      {/each}
    </nav>

    <!-- Content -->
    <main class="bg-zinc-950 border border-zinc-900 p-6 md:p-8">
      {#if activeTab === 'config'}
        <ConfigTab />
      {:else if activeTab === 'lists'}
        <DomainListsTab />
      {:else if activeTab === 'domains'}
        <DomainsTab />
      {/if}
    </main>

    <footer class="mt-12 pt-8 border-t border-zinc-900 text-center text-zinc-600 text-sm">
      &copy; {new Date().getFullYear()} MonaDNS &bull; <a href="https://github.com/LowderPlay/monadns" class="underline">Github</a>
    </footer>
  </div>
  <Toast />
  <LoginOverlay />
</div>

<style>
  :global(body) {
    margin: 0;
    padding: 0;
  }
</style>
