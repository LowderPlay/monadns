<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type DomainList } from '../lib/api';
  import { toast } from './toast_state.svelte';

  let lists = $state<DomainList[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);

  // Form for adding new list
  let newUrl = $state('');
  let newInterval = $state(86400);
  let newIncludeSubdomains = $state(true);
  let adding = $state(false);

  async function loadLists() {
    loading = true;
    try {
      lists = await api.getLists();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to load lists: ' + e.message);
    } finally {
      loading = false;
    }
  }

  onMount(loadLists);

  async function addList() {
    if (!newUrl) return;
    adding = true;
    try {
      await api.addList({
        url: newUrl,
        update_interval_seconds: newInterval,
        include_subdomains: newIncludeSubdomains,
        last_updated: null
      });
      newUrl = '';
      toast.success('Domain list added');
      await loadLists();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to add list: ' + e.message);
    } finally {
      adding = false;
    }
  }

  async function deleteList(id: number) {
    if (!confirm('Are you sure?')) return;
    try {
      await api.removeList(id);
      toast.success('Domain list removed');
      await loadLists();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to remove list: ' + e.message);
    }
  }

  async function syncList(id: number) {
    try {
      await api.syncList(id);
      toast.success('Sync started');
      await loadLists();
    } catch (e: any) {
      error = e.message;
      toast.error('Sync failed: ' + e.message);
    }
  }

  function formatDate(dateStr: string | null) {
    if (!dateStr) return 'Never';
    return new Date(dateStr).toLocaleString();
  }
</script>

<div class="space-y-6">
  <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">Domain lists</h2>

  {#if error}
    <div class="bg-red-900/20 text-red-400 p-4 border border-red-800">
      {error}
      <button onclick={() => error = null} class="ml-2 underline text-sm">Dismiss</button>
    </div>
  {/if}

  <!-- Add New List Form -->
  <div class="bg-zinc-900 p-4 border border-zinc-800 space-y-4">
    <h3 class="text-lg font-bold">Add new list</h3>
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-4 gap-4 items-end">
      <div class="flex flex-col gap-1">
        <label for="list_url" class="text-sm text-zinc-400 font-bold">URL</label>
        <input id="list_url" bind:value={newUrl} placeholder="https://example.com/list.txt" class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="update_interval" class="text-sm text-zinc-400 font-bold">Update interval (sec)</label>
        <input id="update_interval" type="number" bind:value={newInterval} class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex items-center gap-2 h-10">
        <input type="checkbox" id="subdomains" bind:checked={newIncludeSubdomains} class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white" />
        <label for="subdomains" class="text-sm text-zinc-400 font-bold">Include subdomains?</label>
      </div>
      <button onclick={addList} disabled={adding} class="bg-white text-black px-4 py-2 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors">
        {adding ? 'Adding...' : 'Add list'}
      </button>
    </div>
  </div>

  <!-- Lists Table -->
  <div class="overflow-x-auto">
    <table class="w-full border-collapse">
      <thead>
        <tr class="text-left border-b border-zinc-800">
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">#</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">URL</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Subdomains</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Last Updated</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Interval</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each lists as list}
          <tr class="border-b border-zinc-900 hover:bg-zinc-900/50">
            <td class="p-2 truncate max-w-xs" title={list.id?.toString()}>{list.id}</td>
            <td class="p-2 truncate max-w-xs" title={list.url}>{list.url}</td>
            <td class="p-2 text-center text-sm font-mono">{list.include_subdomains ? 'YES' : 'NO'}</td>
            <td class="p-2 text-sm text-zinc-400">{formatDate(list.last_updated)}</td>
            <td class="p-2 text-sm text-zinc-400">{list.update_interval_seconds}s</td>
            <td class="p-2 text-right space-x-4">
              <button onclick={() => syncList(list.id ?? 0)} class="text-zinc-400 hover:text-white font-bold text-xs uppercase tracking-widest transition-colors">Sync</button>
              <button onclick={() => deleteList(list.id ?? 0)} class="text-red-500 hover:text-red-400 font-bold text-xs uppercase tracking-widest transition-colors">Delete</button>
            </td>
          </tr>
        {/each}
        {#if lists.length === 0 && !loading}
          <tr>
            <td colspan="5" class="p-12 text-center text-zinc-600 italic tracking-wide">No domain lists configured.</td>
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
</div>
