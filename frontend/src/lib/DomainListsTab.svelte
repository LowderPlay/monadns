<script lang="ts">
  import { api, type DomainList } from './api';
  import ListManager from './ListManager.svelte';

  const initialNewEntity: DomainList = {
    url: '',
    update_interval_seconds: 86400,
    include_subdomains: true,
    last_updated: null,
    interface: null,
    priority: 0
  };
</script>

<ListManager
  title="Domain list"
  addLabel="Add list"
  emptyMessage="No domain lists configured."
  fetchData={api.getLists}
  addEntity={api.addList}
  removeEntity={api.removeList}
  syncEntity={api.syncList}
  reorderEntities={api.reorderLists}
  {initialNewEntity}
>
  {#snippet extraFields({ entity })}
    <div class="flex items-center gap-2 h-10">
      <input type="checkbox" id="subdomains" bind:checked={entity.include_subdomains} class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white" />
      <label for="subdomains" class="text-sm text-zinc-400 font-bold">Subdomains?</label>
    </div>
  {/snippet}

  {#snippet extraHeaders()}
    <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Subdomains</th>
  {/snippet}

  {#snippet extraCells({ entity })}
    <td class="p-2 text-center text-sm font-mono">{entity.include_subdomains ? 'YES' : 'NO'}</td>
  {/snippet}
</ListManager>
