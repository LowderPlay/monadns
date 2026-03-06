<script lang="ts">
  import { onMount } from 'svelte';
  import { api, type Config, type UpstreamResolverConfig, type ResolverProtocol } from '../lib/api';
  import { toast } from './toast_state.svelte';
  import Trash from "../assets/Trash.svelte";

  let config = $state<Config | null>(null);
  let error = $state<string | null>(null);
  let saving = $state(false);

  onMount(async () => {
    try {
      config = await api.getConfig();
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to load config: ' + e.message);
    }
  });

  async function save() {
    if (!config) return;

    if(config.ipv4_snat?.trim() === "") {
      config.ipv4_snat = null;
    }

    if(config.ipv6_snat?.trim() === "") {
      config.ipv6_snat = null;
    }

    if (config.upstream_resolver.type === 'Custom') {
      if (config.upstream_resolver.nameservers.length === 0) {
        toast.error('At least one nameserver is required for Custom configuration');
        return;
      }
      for (const ns of config.upstream_resolver.nameservers) {
        if (!ns.addr) {
          toast.error('Nameserver address cannot be empty');
          return;
        }
      }
    }

    saving = true;
    try {
      await api.patchConfig(config);
      toast.success('Configuration saved successfully');
    } catch (e: any) {
      error = e.message;
      toast.error('Failed to save config: ' + e.message);
    } finally {
      saving = false;
    }
  }

  const resolverPresets: Record<UpstreamResolverConfig['type'], string> = {
    'Quad9Https': 'Quad9 DoH',
    'CloudflareHttps': 'Cloudflare DoH',
    'GoogleHttps': 'Google DoH',
    'Custom': 'Custom'
  };
  const protocols: ResolverProtocol[] = ['Plain', 'Tls', 'Https'];

  function addNameserver() {
    if (config && config.upstream_resolver.type === 'Custom') {
      config.upstream_resolver.nameservers = [
        ...config.upstream_resolver.nameservers,
        { addr: '', protocol: 'Plain', tls_dns_name: null }
      ];
    }
  }

  function removeNameserver(index: number) {
    if (config && config.upstream_resolver.type === 'Custom') {
      config.upstream_resolver.nameservers = config.upstream_resolver.nameservers.filter((_, i) => i !== index);
    }
  }

  function handleResolverChange(e: Event) {
    const type = (e.target as HTMLSelectElement).value as UpstreamResolverConfig['type'];
    if (config) {
      if (type === 'Custom') {
        config.upstream_resolver = { type: 'Custom', nameservers: [{ addr: '', protocol: 'Plain', tls_dns_name: null }] };
      } else {
        config.upstream_resolver = { type } as UpstreamResolverConfig;
      }
    }
  }
</script>

<div class="space-y-6">

  {#if config}
    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">DNS</h2>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-6">

      <!-- Upstream Resolver -->
      <div class="flex flex-col gap-1">
        <label for="upstream" class="text-sm font-bold text-zinc-300">Upstream DNS</label>
        <p class="text-xs text-zinc-500 mb-1">DNS provider used for resolving queries.</p>
        <select id="upstream" value={config.upstream_resolver.type} onchange={handleResolverChange} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500">
          {#each Object.entries(resolverPresets) as [r, name]}
            <option value={r}>{name}</option>
          {/each}
        </select>
      </div>

      <!-- IPv4 Subnet -->
      <div class="flex flex-col gap-1">
        <label for="ipv4_subnet" class="text-sm font-bold text-zinc-300">Fake IPv4 Subnet</label>
        <p class="text-xs text-zinc-500 mb-1">Subnet used for mapping intercepted domains to fake IPv4s.</p>
        <input id="ipv4_subnet" bind:value={config.ipv4_subnet} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>

      <!-- IPv6 Subnet -->
      <div class="flex flex-col gap-1">
        <label for="ipv6_subnet" class="text-sm font-bold text-zinc-300">Fake IPv6 Subnet</label>
        <p class="text-xs text-zinc-500 mb-1">Subnet used for mapping intercepted domains to fake IPv6s.</p>
        <input id="ipv6_subnet" bind:value={config.ipv6_subnet} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>
    </div>
    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2">Routing</h2>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-6">
      <!-- Interface -->
      <div class="flex flex-col gap-1">
        <label for="iface" class="text-sm font-bold text-zinc-300">Network Interface</label>
        <p class="text-xs text-zinc-500 mb-1">The system interface which the outgoing traffic is routed to (e.g., wg0, eth0).</p>
        <input id="iface" bind:value={config.iface} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>

      <!-- Table ID -->
      <div class="flex flex-col gap-1">
        <label for="table_id" class="text-sm font-bold text-zinc-300">Routing Table ID</label>
        <p class="text-xs text-zinc-500 mb-1">Linux routing table ID where steered packets are routed (by default no routes are added, so default will be used).</p>
        <input id="table_id" type="number" bind:value={config.table_id} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
      </div>

      <!-- TCP MSS Clamp -->
      <div class="flex flex-col gap-1">
        <label for="tcp_mss_clamp_toggle" class="text-sm font-bold text-zinc-300 cursor-pointer">TCP MSS Clamp</label>
        <p class="text-xs text-zinc-500 mb-1">Clamps TCP Maximum Segment Size to prevent MTU issues.</p>
        <div class="flex items-center gap-2">
          <input
                  type="checkbox"
                  id="tcp_mss_clamp_toggle"
                  checked={config.tcp_mss_clamp !== null}
                  onchange={(e) => {
              if (config) {
                if (e.currentTarget.checked) {
                  config.tcp_mss_clamp = 1360;
                } else {
                  config.tcp_mss_clamp = null;
                }
              }
            }}
                  class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white cursor-pointer"
          />
          <input id="tcp_mss_clamp" type="number"
                 disabled={config.tcp_mss_clamp === null}
                 placeholder="Disabled"
                 bind:value={config.tcp_mss_clamp}
                 class="w-full bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" />
          <!--{#if config.tcp_mss_clamp !== null}-->
          <!--  -->
          <!--{:else}-->
          <!--  <p class="p-2">Disabled</p>-->
          <!--{/if}-->
        </div>

      </div>

      <!-- IPv4 SNAT -->
      <div class="flex flex-col gap-1">
        <label for="ipv4_snat" class="text-sm font-bold text-zinc-300">IPv4 SNAT</label>
        <p class="text-xs text-zinc-500 mb-1">Optional Source NAT address for outgoing IPv4 traffic. Masquerading will be used if not set.</p>
        <input id="ipv4_snat" bind:value={config.ipv4_snat} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" placeholder="None" />
      </div>

      <!-- IPv6 SNAT -->
      <div class="flex flex-col gap-1">
        <label for="ipv6_snat" class="text-sm font-bold text-zinc-300">IPv6 SNAT</label>
        <p class="text-xs text-zinc-500 mb-1">Optional Source NAT address for outgoing IPv6 traffic. Masquerading will be used if not set.</p>
        <input id="ipv6_snat" bind:value={config.ipv6_snat} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500" placeholder="None" />
      </div>
    </div>

    <!-- Custom Nameservers Section -->
    {#if config.upstream_resolver.type === 'Custom'}
      <div class="mt-8 border border-zinc-800 p-6 space-y-4">
        <div class="flex items-center justify-between">
          <h3 class="font-bold text-zinc-300 uppercase tracking-widest text-xs">Custom nameservers</h3>
          <button onclick={addNameserver} class="text-xs font-bold border border-zinc-700 px-3 py-1 hover:bg-zinc-800 transition-colors uppercase tracking-widest">Add Nameserver</button>
        </div>
        
        <div class="space-y-4">
          {#each config.upstream_resolver.nameservers as ns, i}
            <div class="grid grid-cols-1 md:grid-cols-12 gap-4 items-end border-b border-zinc-900 pb-4">
              <div class="md:col-span-3 flex flex-col gap-1">
                <label for="ns-proto-{i}" class="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Protocol</label>
                <select id="ns-proto-{i}" bind:value={ns.protocol} class="bg-zinc-950 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600">
                  {#each protocols as p}
                    <option value={p}>{p}</option>
                  {/each}
                </select>
              </div>
              <div class="{ns.protocol === 'Plain' ? 'md:col-span-8' : 'md:col-span-5'} flex flex-col gap-1">
                <label for="ns-addr-{i}" class="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">Address (IP[:Port])</label>
                <input id="ns-addr-{i}" bind:value={ns.addr} placeholder="1.1.1.1" class="bg-zinc-950 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
              </div>
              {#if (ns.protocol !== 'Plain')}
                <div class="md:col-span-3 flex flex-col gap-1">
                  <label for="ns-tls-{i}" class="text-[10px] font-bold text-zinc-500 uppercase tracking-wider">TLS Hostname</label>
                  <input id="ns-tls-{i}" bind:value={ns.tls_dns_name} placeholder="cloudflare-dns.com" class="bg-zinc-950 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600 disabled:opacity-30" />
                </div>
              {/if}

              <div class="md:col-span-1 flex justify-end">
                <button onclick={() => removeNameserver(i)} class="text-red-500 hover:text-red-400 p-2 transition-colors">
                  <Trash />
                </button>
              </div>
            </div>
          {/each}
        </div>
      </div>
    {/if}

    <div class="pt-6">
      <button onclick={save} disabled={saving} class="bg-white text-black px-8 py-3 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors uppercase tracking-widest text-sm">
        {saving ? 'Saving...' : 'Save Configuration'}
      </button>
    </div>
  {:else}
    <div class="animate-pulse space-y-8">
      <div class="grid grid-cols-2 gap-8">
        {#each Array(8) as _}
          <div class="space-y-2">
            <div class="h-3 bg-zinc-900 w-1/4"></div>
            <div class="h-10 bg-zinc-900"></div>
          </div>
        {/each}
      </div>
    </div>
  {/if}
</div>
