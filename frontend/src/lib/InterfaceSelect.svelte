<script lang="ts">
  import { api, type Config } from './api';
  import { onMount } from 'svelte';

  interface Props {
    value: string | null;
    onchange: (value: string | null) => void;
    id?: string;
  }

  let { value, onchange, id = 'interface_select' }: Props = $props();
  let config = $state<Config | null>(null);

  onMount(async () => {
    try {
      config = await api.getConfig();
    } catch (e) {
      console.error('Failed to load config for InterfaceSelect', e);
    }
  });

  function handleChange(e: Event) {
    const val = (e.target as HTMLSelectElement).value;
    onchange(val === 'default' ? null : val);
  }
</script>

<div class="flex flex-col gap-1">
  <label for={id} class="text-sm text-zinc-400 font-bold">Interface</label>
  <select 
    {id} 
    value={value === null ? 'default' : value} 
    onchange={handleChange}
    class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500 w-full"
  >
    <option value="default">Default ({config?.default_interface || '...' })</option>
    {#if config}
      {#each config.interfaces as iface}
        <option value={iface.name}>{iface.name}</option>
      {/each}
    {/if}
  </select>
</div>
