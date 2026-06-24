<script lang="ts">
import { onMount } from "svelte";
import File from "../assets/File.svelte";
import Trash from "../assets/Trash.svelte";
import {
	api,
	type Config,
	type ResolverProtocol,
	type UpstreamResolverConfig,
} from "./api";
import { toast } from "./toast_state.svelte";

let config = $state<Config | null>(null);
let saving = $state(false);

onMount(async () => {
	try {
		config = await api.getConfig();
	} catch (e: any) {
		toast.error("Failed to load config: " + e.message);
	}
});

async function save() {
	if (!config) return;

	for (const iface of config.interfaces) {
		if (iface.ipv4_snat?.trim() === "") {
			iface.ipv4_snat = null;
		}
		if (iface.ipv6_snat?.trim() === "") {
			iface.ipv6_snat = null;
		}
		iface.health_check_hosts = iface.health_check_hosts
			.map((host) => host.trim())
			.filter(Boolean);
		if (iface.health_check_enabled && iface.health_check_hosts.length === 0) {
			toast.error(
				`At least one health check host is required for ${iface.name}`,
			);
			return;
		}
	}

	if (config.upstream_resolver.type === "Custom") {
		if (config.upstream_resolver.nameservers.length === 0) {
			toast.error(
				"At least one nameserver is required for Custom configuration",
			);
			return;
		}
		for (const ns of config.upstream_resolver.nameservers) {
			if (!ns.addr) {
				toast.error("Nameserver address cannot be empty");
				return;
			}
		}
	}

	saving = true;
	try {
		await api.patchConfig(config);
		toast.success("Configuration saved successfully");
	} catch (e: any) {
		toast.error(`Failed to save config: ${e.message}`);
	} finally {
		saving = false;
	}
}

const resolverPresets: Record<UpstreamResolverConfig["type"], string> = {
	Quad9Https: "Quad9 DoH",
	CloudflareHttps: "Cloudflare DoH",
	GoogleHttps: "Google DoH",
	Custom: "Custom",
};
const protocols: ResolverProtocol[] = ["Plain", "Tls", "Https"];

function addNameserver() {
	if (config && config.upstream_resolver.type === "Custom") {
		config.upstream_resolver.nameservers = [
			...config.upstream_resolver.nameservers,
			{ addr: "", protocol: "Plain", tls_dns_name: null },
		];
	}
}

function removeNameserver(index: number) {
	if (config && config.upstream_resolver.type === "Custom") {
		config.upstream_resolver.nameservers =
			config.upstream_resolver.nameservers.filter((_, i) => i !== index);
	}
}

function addInterface() {
	if (config) {
		const nextFwmark =
			Math.max(0, ...config.interfaces.map((i) => i.fwmark)) + 1;
		const nextTable =
			Math.max(0, ...config.interfaces.map((i) => i.table_id)) + 1;
		config.interfaces = [
			...config.interfaces,
			{
				name: "new0",
				fwmark: nextFwmark,
				table_id: nextTable,
				tcp_mss_clamp: 1280,
				ipv4_snat: null,
				ipv6_snat: null,
				health_check_enabled: true,
				health_check_hosts: ["1.1.1.1", "2606:4700:4700::1111"],
				health_check_latency_threshold_ms: 500,
				health_check_packet_loss_threshold_percent: 50,
			},
		];
	}
}

function removeInterface(index: number) {
	if (config && config.interfaces.length > 1) {
		const ifaceName = config.interfaces[index].name;
		config.interfaces = config.interfaces.filter((_, i) => i !== index);
		if (config.default_interface === ifaceName) {
			config.default_interface = config.interfaces[0].name;
		}
	} else {
		toast.error("At least one interface is required");
	}
}

function addHealthCheckHost(interfaceIndex: number) {
	if (config) {
		config.interfaces[interfaceIndex].health_check_hosts = [
			...config.interfaces[interfaceIndex].health_check_hosts,
			"",
		];
	}
}

function removeHealthCheckHost(interfaceIndex: number, hostIndex: number) {
	if (config) {
		config.interfaces[interfaceIndex].health_check_hosts = config.interfaces[
			interfaceIndex
		].health_check_hosts.filter((_, index) => index !== hostIndex);
	}
}

function handleResolverChange(e: Event) {
	const type = (e.target as HTMLSelectElement)
		.value as UpstreamResolverConfig["type"];
	if (config) {
		if (type === "Custom") {
			config.upstream_resolver = {
				type: "Custom",
				nameservers: [{ addr: "", protocol: "Plain", tls_dns_name: null }],
			};
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

    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2 mt-8">Export</h2>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-6">
      <div class="flex items-center gap-4 h-fit">
        <input
                id="export_enabled"
                type="checkbox"
                bind:checked={config.export_enabled}
                class="w-5 h-5 border-zinc-700 bg-zinc-950 accent-white cursor-pointer"
        />
        <div class="space-y-1">
          <label for="export_enabled" class="text-sm font-bold text-zinc-300">Public Rule Export</label>
          <p class="text-xs text-zinc-500">Enable unauthenticated access to your active domain and IP rules as .lst files. (Make sure to save the config)</p>
        </div>
      </div>
      {#if config.export_enabled}
        <div class="flex flex-col gap-1">
          <label for="ipv6_subnet" class="text-sm font-bold text-zinc-300">Download Lists</label>
          <p class="text-xs text-zinc-500 mb-1">Below are permanent links to actual lists used in MonaDNS. You can use them for conditional routing</p>
          <div class="pt-2 flex flex-col sm:flex-row gap-4">
            <a href="/api/export/domains.lst" target="_blank" class="text-xs font-bold text-zinc-400 hover:text-white flex items-center gap-2 transition-colors">
              <File/>
              domains.lst
            </a>
            <a href="/api/export/ips.lst" target="_blank" class="text-xs font-bold text-zinc-400 hover:text-white flex items-center gap-2 transition-colors ">
              <File/>
              ips.lst
            </a>
          </div>
        </div>
      {/if}
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

    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2 mt-8">Interfaces</h2>

    <div class="grid grid-cols-1 md:grid-cols-3 gap-6 mt-4">
      <div class="flex flex-col gap-1">
        <label for="health-interval" class="text-xs font-bold text-zinc-400 uppercase">Health Check Interval (seconds)</label>
        <p class="text-xs text-zinc-500 mb-1">Time between checks for every interface and host.</p>
        <input id="health-interval" type="number" min="1" bind:value={config.health_check_interval_seconds} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="health-timeout" class="text-xs font-bold text-zinc-400 uppercase">Health Check Timeout (seconds)</label>
        <p class="text-xs text-zinc-500 mb-1">Maximum duration allowed for one ping batch.</p>
        <input id="health-timeout" type="number" min="1" bind:value={config.health_check_timeout_seconds} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="health-ping-count" class="text-xs font-bold text-zinc-400 uppercase">Ping Count</label>
        <p class="text-xs text-zinc-500 mb-1">ICMP echo requests sent to each host per check.</p>
        <input id="health-ping-count" type="number" min="1" bind:value={config.health_check_ping_count} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
      </div>
    </div>
    
    <div class="flex flex-col gap-4 mt-4">
      <div class="flex flex-col gap-1 max-w-md">
        <label for="default_iface" class="text-sm font-bold text-zinc-300">Default Interface</label>
        <p class="text-xs text-zinc-500 mb-1">Interface used when no specific interface is assigned to a rule.</p>
        <select id="default_iface" bind:value={config.default_interface} class="bg-zinc-900 border border-zinc-700 p-2 focus:outline-none focus:border-zinc-500">
          {#each config.interfaces as iface}
            <option value={iface.name}>{iface.name}</option>
          {/each}
        </select>
      </div>

      <div class="space-y-6 mt-4">
        {#each config.interfaces as iface, i}
          <div class="border border-zinc-800 p-6 space-y-4 bg-zinc-950/30">
            <div class="flex items-center justify-between">
              <h3 class="font-bold text-white uppercase tracking-widest text-xs">
                Interface: {iface.name} {#if iface.name === config.default_interface}(Default){/if}
              </h3>
              <button onclick={() => removeInterface(i)} class="text-red-500 hover:text-red-400 p-2 transition-colors">
                <Trash />
              </button>
            </div>

            <div class="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-6">
              <!-- Interface Name -->
              <div class="flex flex-col gap-1">
                <label for="iface-name-{i}" class="text-xs font-bold text-zinc-400 uppercase">Name</label>
                <p class="text-xs text-zinc-500 mb-1">The system interface which the outgoing traffic is routed to (e.g., wg0, eth0).</p>
                <input id="iface-name-{i}" bind:value={iface.name} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
              </div>

              <!-- Fwmark -->
              <div class="flex flex-col gap-1">
                <label for="iface-mark-{i}" class="text-xs font-bold text-zinc-400 uppercase">Fwmark</label>
                <p class="text-xs text-zinc-500 mb-1">Packets fwmark to steer traffic with.</p>
                <input id="iface-mark-{i}" type="number" bind:value={iface.fwmark} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
              </div>

              <!-- Table ID -->
              <div class="flex flex-col gap-1">
                <label for="iface-table-{i}" class="text-xs font-bold text-zinc-400 uppercase">Routing Table ID</label>
                <p class="text-xs text-zinc-500 mb-1">Linux routing table ID where steered packets are routed (by default no routes are added, so default will be used).</p>
                <input id="iface-table-{i}" type="number" bind:value={iface.table_id} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
              </div>

              <!-- TCP MSS Clamp -->
              <div class="flex flex-col gap-1">
                <label for="iface-mss-{i}" class="text-xs font-bold text-zinc-400 uppercase">TCP MSS Clamp</label>
                <p class="text-xs text-zinc-500 mb-1">Clamps TCP Maximum Segment Size to prevent MTU issues.</p>
                <div class="flex items-center gap-2">
                  <input
                    type="checkbox"
                    checked={iface.tcp_mss_clamp !== null}
                    onchange={(e) => {
                      if (e.currentTarget.checked) {
                        iface.tcp_mss_clamp = 1360;
                      } else {
                        iface.tcp_mss_clamp = null;
                      }
                    }}
                    class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white cursor-pointer"
                  />
                  <input id="iface-mss-{i}" type="number"
                         disabled={iface.tcp_mss_clamp === null}
                         placeholder="Disabled"
                         bind:value={iface.tcp_mss_clamp}
                         class="w-full bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" />
                </div>
              </div>

              <!-- IPv4 SNAT -->
              <div class="flex flex-col gap-1">
                <label for="iface-snat4-{i}" class="text-xs font-bold text-zinc-400 uppercase">IPv4 SNAT</label>
                <p class="text-xs text-zinc-500 mb-1">Optional Source NAT address for outgoing IPv4 traffic. Masquerading will be used if not set.</p>
                <input id="iface-snat4-{i}" bind:value={iface.ipv4_snat} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" placeholder="Masquerade" />
              </div>

              <!-- IPv6 SNAT -->
              <div class="flex flex-col gap-1">
                <label for="iface-snat6-{i}" class="text-xs font-bold text-zinc-400 uppercase">IPv6 SNAT</label>
                <p class="text-xs text-zinc-500 mb-1">Optional Source NAT address for outgoing IPv6 traffic. Masquerading will be used if not set.</p>
                <input id="iface-snat6-{i}" bind:value={iface.ipv6_snat} class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600" placeholder="Masquerade" />
              </div>

              <!-- Health Check -->
              <div class="flex items-center gap-3 h-fit">
                <input
                  id="iface-health-enabled-{i}"
                  type="checkbox"
                  bind:checked={iface.health_check_enabled}
                  class="w-4 h-4 border-zinc-700 bg-zinc-950 accent-white cursor-pointer"
                />
                <div>
                  <label for="iface-health-enabled-{i}" class="text-xs font-bold text-zinc-400 uppercase">Health Check</label>
                  <p class="text-xs text-zinc-500">Periodically ping a host through this interface.</p>
                </div>
              </div>

              <div class="flex flex-col gap-2 lg:col-span-2">
                <div class="flex items-center justify-between">
                  <div>
                    <span class="text-xs font-bold text-zinc-400 uppercase">Health Check Hosts</span>
                    <p class="text-xs text-zinc-500">IPv4/IPv6 addresses or hostnames to ping through this interface.</p>
                  </div>
                  <button
                    type="button"
                    disabled={!iface.health_check_enabled}
                    onclick={() => addHealthCheckHost(i)}
                    class="text-xs font-bold border border-zinc-700 px-3 py-1 hover:bg-zinc-800 disabled:opacity-40"
                  >
                    Add Host
                  </button>
                </div>
                {#each iface.health_check_hosts as _, hostIndex}
                  <div class="flex gap-2">
                    <input
                      aria-label="Health check host"
                      disabled={!iface.health_check_enabled}
                      bind:value={iface.health_check_hosts[hostIndex]}
                      class="w-full bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600 disabled:opacity-40"
                      placeholder={hostIndex === 0 ? "1.1.1.1" : "2606:4700:4700::1111"}
                    />
                    <button
                      type="button"
                      disabled={!iface.health_check_enabled}
                      onclick={() => removeHealthCheckHost(i, hostIndex)}
                      class="text-red-500 hover:text-red-400 p-2 disabled:opacity-40"
                    >
                      <Trash />
                    </button>
                  </div>
                {/each}
              </div>

              <div class="flex flex-col gap-1">
                <label for="iface-health-latency-{i}" class="text-xs font-bold text-zinc-400 uppercase">Maximum Latency (ms)</label>
                <p class="text-xs text-zinc-500 mb-1">Average latency above this value marks the interface unhealthy.</p>
                <input
                  id="iface-health-latency-{i}"
                  type="number"
                  min="0"
                  step="1"
                  disabled={!iface.health_check_enabled}
                  bind:value={iface.health_check_latency_threshold_ms}
                  class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600 disabled:opacity-40"
                />
              </div>

              <div class="flex flex-col gap-1">
                <label for="iface-health-loss-{i}" class="text-xs font-bold text-zinc-400 uppercase">Maximum Packet Loss (%)</label>
                <p class="text-xs text-zinc-500 mb-1">Packet loss above this percentage marks the interface unhealthy.</p>
                <input
                  id="iface-health-loss-{i}"
                  type="number"
                  min="0"
                  max="100"
                  step="1"
                  disabled={!iface.health_check_enabled}
                  bind:value={iface.health_check_packet_loss_threshold_percent}
                  class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600 disabled:opacity-40"
                />
              </div>
            </div>
          </div>
        {/each}

        <button onclick={addInterface} class="w-full border border-dashed border-zinc-700 p-4 text-zinc-500 hover:text-zinc-300 hover:border-zinc-500 hover:bg-zinc-900/30 transition-all uppercase tracking-widest text-xs font-bold">
          + Add Outbound Interface
        </button>
      </div>
    </div>

    <div class="pt-10">
      <button onclick={save} disabled={saving} class="bg-white text-black px-10 py-4 font-bold hover:bg-zinc-200 disabled:bg-zinc-600 transition-colors uppercase tracking-widest text-sm">
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
