<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type DomainRule } from '../lib/api';
  import { toast } from './toast_state.svelte';

  let domains = $state<DomainRule[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);

  // Form for adding new domain
  let newDomain = $state('');
  let newIncludeSubdomains = $state(true);
  let adding = $state(false);

  async function loadDomains() {
    loading = true;
    try {
      domains = await api.getDomains();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to load domains: ' + e.message);
    } finally {
      loading = false;
    }
  }

  onMount(loadDomains);

  async function addDomain() {
    if (!newDomain) return;
    adding = true;
    try {
      await api.addDomain({
        domain: newDomain,
        include_subdomains: newIncludeSubdomains
      });
      newDomain = '';
      toast.success('Domain rule added');
      await loadDomains();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to add domain: ' + e.message);
    } finally {
      adding = false;
    }
  }

  async function deleteDomain(domain: string) {
    if (!confirm(`Remove ${domain}?`)) return;
    try {
      await api.removeDomain(domain);
      toast.success('Domain rule removed');
      await loadDomains();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to remove domain: ' + e.message);
    }
  }
</script>

<div class="space-y-6">
  <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">Domains</h2>

  {#if error}
    <div class="bg-red-900/20 text-red-400 p-4 border border-red-800">
      {error}
      <button onclick={() => error = null} class="ml-2 underline text-sm">Dismiss</button>
    </div>
  {/if}

  <!-- Add New Domain Form -->
  <div class="bg-zinc-900 p-4 border border-zinc-800 space-y-4">
    <h3 class="text-lg font-bold">Add new domain rule</h3>
    <div class="grid grid-cols-1 md:grid-cols-3 gap-4 items-end">
      <div class="flex flex-col gap-1">
        <label for="domain_input" class="text-sm text-zinc-400 font-bold">Domain</label>
        <input id="domain_input" bind:value={newDomain} placeholder="example.com" class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex items-center gap-2 h-10">
        <input type="checkbox" id="sub_domain" bind:checked={newIncludeSubdomains} class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white" />
        <label for="sub_domain" class="text-sm text-zinc-400 font-bold">Include subdomains?</label>
      </div>
      <button onclick={addDomain} disabled={adding} class="bg-white text-black px-4 py-2 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors">
        {adding ? 'Adding...' : 'Add domain'}
      </button>
    </div>
  </div>

  <!-- Domains Table -->
  <div class="overflow-x-auto">
    <table class="w-full border-collapse text-left">
      <thead>
        <tr class="border-b border-zinc-800">
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Domain</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Subdomains</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each domains as d}
          <tr class="border-b border-zinc-900 hover:bg-zinc-900/50">
            <td class="p-2 tracking-tight">{d.domain}</td>
            <td class="p-2 text-center text-sm font-mono">{d.include_subdomains ? 'YES' : 'NO'}</td>
            <td class="p-2 text-right">
              <button onclick={() => deleteDomain(d.domain)} class="text-red-500 hover:text-red-400 font-bold text-xs uppercase tracking-widest transition-colors">Delete</button>
            </td>
          </tr>
        {/each}
        {#if domains.length === 0 && !loading}
          <tr>
            <td colspan="3" class="p-12 text-center text-zinc-600 italic tracking-wide">No custom domain rules configured.</td>
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
</div>
