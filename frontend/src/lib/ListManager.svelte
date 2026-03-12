<script lang="ts" generics="T extends { id?: number; url: string; update_interval_seconds: number; last_updated: string | null; interface?: string | null; }">
  import { onMount } from 'svelte';
  import { toast } from './toast_state.svelte';
  import { api, type AvailableGeoOptions } from './api';
  import type { Snippet } from 'svelte';
  import InterfaceSelect from './InterfaceSelect.svelte';

  interface Props {
    title: string;
    addLabel: string;
    emptyMessage: string;
    fetchData: () => Promise<T[]>;
    addEntity: (entity: T) => Promise<any>;
    removeEntity: (id: number) => Promise<any>;
    syncEntity: (id: number) => Promise<any>;
    reorderEntities?: (ids: number[]) => Promise<any>;
    initialNewEntity: T;
    // Snippets for customization
    extraFields?: Snippet<[{ entity: T, useGeo: boolean }]>;
    extraHeaders?: Snippet;
    extraCells?: Snippet<[{ entity: T }]>;
    geoPrefix?: 'geosite://' | 'geoip://';
    showInterface?: boolean;
  }

  let { 
    title, addLabel, emptyMessage, 
    fetchData, addEntity, removeEntity, syncEntity, reorderEntities,
    initialNewEntity,
    extraFields, extraHeaders, extraCells,
    geoPrefix,
    showInterface = true
  }: Props = $props();


  let items = $state<T[]>([]);
  let error = $state<string | null>(null);
  let loading = $state(true);

  // Form for adding new list
  let newEntity = $state<T>({ ...initialNewEntity });
  let adding = $state(false);

  let geoOptions = $state<AvailableGeoOptions | null>(null);
  let useGeo = $state(false);
  let selectedGeo = $state('');

  async function loadData() {
    loading = true;
    try {
      items = await fetchData();
      if (geoPrefix) {
        geoOptions = await api.getGeoOptions();
      }
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to load data: ' + e.message);
    } finally {
      loading = false;
    }
  }

  onMount(loadData);

  $effect(() => {
    if (useGeo && geoPrefix && selectedGeo) {
      newEntity.url = geoPrefix + selectedGeo;
    }
  });

  async function addItem() {
    if (!newEntity.url) return;
    adding = true;
    try {
      await addEntity(newEntity);
      newEntity = { ...initialNewEntity };
      toast.success(`${title} added`);
      await loadData();
    } catch (e: any) {
      error = e.message;
      toast.error(`Failed to add ${title.toLowerCase()}: ` + e.message);
    } finally {
      adding = false;
    }
  }

  async function deleteItem(id: number) {
    if (!confirm('Are you sure?')) return;
    try {
      await removeEntity(id);
      toast.success(`${title} removed`);
      await loadData();
    } catch (e: any) {
      error = e.message;
      toast.error(`Failed to remove ${title.toLowerCase()}: ` + e.message);
    }
  }

  async function syncItem(id: number) {
    try {
      await syncEntity(id);
      toast.success('Sync started');
      await loadData();
    } catch (e: any) {
      error = e.message;
      toast.error('Sync failed: ' + e.message);
    }
  }

  async function moveItem(index: number, direction: 'up' | 'down') {
    if (!reorderEntities) return;
    
    const newItems = [...items];
    const targetIndex = direction === 'up' ? index - 1 : index + 1;
    
    if (targetIndex < 0 || targetIndex >= newItems.length) return;
    
    const [movedItem] = newItems.splice(index, 1);
    newItems.splice(targetIndex, 0, movedItem);
    
    try {
      const ids = newItems.map(item => item.id!).filter(id => id !== undefined);
      await reorderEntities(ids);
      items = newItems;
      toast.success('Order updated');
    } catch (e: any) {
      toast.error('Failed to reorder: ' + e.message);
    }
  }

  function formatDate(dateStr: string | null) {
    if (!dateStr) return 'Never';
    return new Date(dateStr).toLocaleString();
  }
</script>

<div class="space-y-6">
  <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">{title}s</h2>

  {#if error}
    <div class="bg-red-900/20 text-red-400 p-4 border border-red-800">
      {error}
      <button onclick={() => error = null} class="ml-2 underline text-sm">Dismiss</button>
    </div>
  {/if}

  <!-- Add New Entity Form -->
  <div class="bg-zinc-900 p-4 border border-zinc-800 space-y-4">
    <div class="flex items-center justify-between">
      <h3 class="text-lg font-bold">Add new {title.toLowerCase()}</h3>
      {#if geoPrefix && geoOptions}
        <div class="flex gap-2 p-1 bg-zinc-950 border border-zinc-800 rounded">
          <button 
            onclick={() => useGeo = false} 
            class="px-3 py-1 text-xs font-bold uppercase tracking-widest transition-colors {!useGeo ? 'bg-white text-black' : 'text-zinc-500 hover:text-white'}"
          >Custom URL</button>
          <button 
            onclick={() => useGeo = true} 
            class="px-3 py-1 text-xs font-bold uppercase tracking-widest transition-colors {useGeo ? 'bg-white text-black' : 'text-zinc-500 hover:text-white'}"
          >Geo Category</button>
        </div>
      {/if}
    </div>
    <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-6 gap-4 items-end">
      {#if useGeo && geoPrefix && geoOptions}
        <div class="flex flex-col gap-1 lg:col-span-2">
          <label for="entity_geo" class="text-sm text-zinc-400 font-bold">Category</label>
          <select id="entity_geo" bind:value={selectedGeo} class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500 text-sm h-10">
            <option value="" disabled>Select a category</option>
            {#each geoPrefix === 'geosite://' ? geoOptions.geosite : geoOptions.geoip as cat}
              <option value={cat}>{cat}</option>
            {/each}
          </select>
        </div>
      {:else}
        <div class="flex flex-col gap-1 lg:col-span-2">
          <label for="entity_url" class="text-sm text-zinc-400 font-bold">URL</label>
          <input id="entity_url" bind:value={newEntity.url} placeholder="https://example.com/list.txt" class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500 h-10" />
        </div>
      {/if}

      
      {#if showInterface}
        <InterfaceSelect 
          value={newEntity.interface ?? null}
          onchange={(val) => newEntity.interface = val} 
        />
      {/if}

      <div class="flex flex-col gap-1">
        <label for="update_interval" class="text-sm text-zinc-400 font-bold">Update interval (sec)</label>
        <input id="update_interval" type="number" bind:value={newEntity.update_interval_seconds} class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
      
      {#if extraFields}
        {@render extraFields({ entity: newEntity, useGeo })}
      {/if}

      <button onclick={addItem} disabled={adding} class="bg-white text-black px-4 py-2 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors uppercase text-xs tracking-widest h-10">
        {adding ? 'Adding...' : addLabel}
      </button>
    </div>
  </div>
  {#if reorderEntities}
    <p class="text-xs text-zinc-500 mb-1">The rules are prioritised by their order (starting from the top).</p>
  {/if}

  <!-- Entities Table -->
  <div class="overflow-x-auto">
    <table class="w-full border-collapse">
      <thead>
        <tr class="text-left border-b border-zinc-800">
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">#</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">URL</th>
          {#if showInterface}
            <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Interface</th>
          {/if}
          
          {#if extraHeaders}
            {@render extraHeaders()}
          {/if}

          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Last Updated</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Interval</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each items as item, i}
          <tr class="border-b border-zinc-900 hover:bg-zinc-900/50">
            <td class="p-2 truncate max-w-xs" title={item.id?.toString()}>
              <div class="flex items-center gap-2">
                {#if reorderEntities}
                  <div class="flex flex-col gap-0.5">
                    <button 
                      onclick={() => moveItem(i, 'up')} 
                      disabled={i === 0}
                      class="text-[10px] hover:text-white disabled:text-zinc-700 transition-colors"
                      title="Move Up"
                    >▲</button>
                    <button 
                      onclick={() => moveItem(i, 'down')} 
                      disabled={i === items.length - 1}
                      class="text-[10px] hover:text-white disabled:text-zinc-700 transition-colors"
                      title="Move Down"
                    >▼</button>
                  </div>
                {/if}
                <span>{item.id}</span>
              </div>
            </td>
            <td class="p-2 truncate max-w-xs" title={item.url}>{item.url}</td>
            {#if showInterface}
              <td class="p-2 text-center text-sm font-mono text-zinc-400">{item.interface || 'Default'}</td>
            {/if}
            
            {#if extraCells}
              {@render extraCells({ entity: item })}
            {/if}

            <td class="p-2 text-sm text-zinc-400">{formatDate(item.last_updated)}</td>
            <td class="p-2 text-sm text-zinc-400">{item.update_interval_seconds}s</td>
            <td class="p-2 text-right space-x-4">
              <button onclick={() => syncItem(item.id ?? 0)} class="text-zinc-400 hover:text-white font-bold text-xs uppercase tracking-widest transition-colors">Sync</button>
              <button onclick={() => deleteItem(item.id ?? 0)} class="text-red-500 hover:text-red-400 font-bold text-xs uppercase tracking-widest transition-colors">Delete</button>
            </td>
          </tr>
        {/each}
        {#if items.length === 0 && !loading}
          <tr>
            <td colspan="7" class="p-12 text-center text-zinc-600 italic tracking-wide">{emptyMessage}</td>
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
</div>
