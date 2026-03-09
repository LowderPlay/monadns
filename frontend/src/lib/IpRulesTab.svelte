<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type IpRule, type Config } from './api';
  import { toast } from './toast_state.svelte';

  let ips = $state<IpRule[]>([]);
  let config = $state<Config | null>(null);
  let error = $state<string | null>(null);
  let loading = $state(true);

  // Form for adding new IP rule
  let newSubnet = $state('');
  let newInterface = $state<string | null>(null);
  let adding = $state(false);

  async function loadData() {
    loading = true;
    try {
      const [i, c] = await Promise.all([api.getIps(), api.getConfig()]);
      ips = i;
      config = c;
      if (!newInterface && config.interfaces.length > 0) {
        newInterface = 'default';
      }
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to load data: ' + e.message);
    } finally {
      loading = false;
    }
  }

  onMount(loadData);

  async function addIp() {
    if (!newSubnet) return;
    adding = true;
    try {
      await api.addIp({
        subnet: newSubnet,
        interface: newInterface === 'default' ? null : newInterface
      });
      newSubnet = '';
      toast.success('IP rule added');
      await loadData();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to add IP rule: ' + e.message);
    } finally {
      adding = false;
    }
  }

  async function deleteIp(subnet: string) {
    if (!confirm(`Remove ${subnet}?`)) return;
    try {
      await api.removeIp(subnet);
      toast.success('IP rule removed');
      await loadData();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to remove IP rule: ' + e.message);
    }
  }
</script>

<div class="space-y-6">
  <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">IP Rules</h2>

  {#if error}
    <div class="bg-red-900/20 text-red-400 p-4 border border-red-800">
      {error}
      <button onclick={() => error = null} class="ml-2 underline text-sm">Dismiss</button>
    </div>
  {/if}

  <!-- Add New IP Rule Form -->
  <div class="bg-zinc-900 p-4 border border-zinc-800 space-y-4">
    <h3 class="text-lg font-bold">Add new IP rule</h3>
    <div class="grid grid-cols-1 md:grid-cols-3 gap-4 items-end">
      <div class="flex flex-col gap-1">
        <label for="subnet_input" class="text-sm text-zinc-400 font-bold">Subnet (CIDR)</label>
        <input id="subnet_input" bind:value={newSubnet} placeholder="1.1.1.0/24" class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="interface_select" class="text-sm text-zinc-400 font-bold">Interface</label>
        <select id="interface_select" bind:value={newInterface} class="bg-zinc-950 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500">
          <option value="default">Default ({config?.default_interface || '...' })</option>
          {#if config}
            {#each config.interfaces as iface}
              <option value={iface.name}>{iface.name}</option>
            {/each}
          {/if}
        </select>
      </div>
      <button onclick={addIp} disabled={adding} class="bg-white text-black px-4 py-2 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors uppercase text-xs tracking-widest h-10">
        {adding ? 'Adding...' : 'Add IP Rule'}
      </button>
    </div>
  </div>

  <!-- IPs Table -->
  <div class="overflow-x-auto">
    <table class="w-full border-collapse text-left">
      <thead>
        <tr class="border-b border-zinc-800">
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider">Subnet</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-center">Interface</th>
          <th class="p-2 text-zinc-400 font-bold uppercase text-xs tracking-wider text-right">Actions</th>
        </tr>
      </thead>
      <tbody>
        {#each ips as ip}
          <tr class="border-b border-zinc-900 hover:bg-zinc-900/50">
            <td class="p-2 tracking-tight font-mono">{ip.subnet}</td>
            <td class="p-2 text-center text-sm font-mono text-zinc-400">{ip.interface || 'Default'}</td>
            <td class="p-2 text-right">
              <button onclick={() => deleteIp(ip.subnet)} class="text-red-500 hover:text-red-400 font-bold text-xs uppercase tracking-widest transition-colors">Delete</button>
            </td>
          </tr>
        {/each}
        {#if ips.length === 0 && !loading}
          <tr>
            <td colspan="3" class="p-12 text-center text-zinc-600 italic tracking-wide">No custom IP rules configured.</td>
          </tr>
        {/if}
      </tbody>
    </table>
  </div>
</div>
