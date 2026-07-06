<script lang="ts">
import { onMount } from "svelte";
import File from "../assets/File.svelte";
import Trash from "../assets/Trash.svelte";
import {
	api,
	type Config,
	type FailoverMode,
	type InterfaceHealthStatus,
	type ResolverProtocol,
	type UpstreamResolverConfig,
} from "./api";
import { toast } from "./toast_state.svelte";

let config = $state<Config | null>(null);
let saving = $state(false);
let interfaceHealth = $state<InterfaceHealthStatus[]>([]);
let healthError = $state<string | null>(null);
let expandedInterfaces = $state<Record<string, boolean>>({});

async function loadInterfaceHealth() {
	try {
		interfaceHealth = await api.getInterfaceHealth();
		healthError = null;
	} catch (e: any) {
		healthError = e.message;
	}
}

onMount(() => {
	void (async () => {
		try {
			config = await api.getConfig();
			await loadInterfaceHealth();
		} catch (e: any) {
			toast.error("Failed to load config: " + e.message);
		}
	})();

	const interval = window.setInterval(loadInterfaceHealth, 5000);
	return () => window.clearInterval(interval);
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
		iface.failover_interfaces = iface.failover_interfaces.filter(Boolean);
		if (iface.health_check_enabled && iface.health_check_hosts.length === 0) {
			toast.error(
				`At least one health check host is required for ${iface.name}`,
			);
			return;
		}
	}
	config.failover_interfaces = config.failover_interfaces.filter(Boolean);

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
				failover_mode: "global",
				failover_interfaces: [],
			},
		];
		config.failover_interfaces = [...config.failover_interfaces, "new0"];
	}
}

function removeInterface(index: number) {
	if (config && config.interfaces.length > 1) {
		const ifaceName = config.interfaces[index].name;
		config.interfaces = config.interfaces.filter((_, i) => i !== index);
		const { [ifaceName]: _, ...nextExpandedInterfaces } = expandedInterfaces;
		expandedInterfaces = nextExpandedInterfaces;
		if (config.default_interface === ifaceName) {
			config.default_interface = config.interfaces[0].name;
		}
		config.failover_interfaces = config.failover_interfaces.filter(
			(name) => name !== ifaceName,
		);
		for (const iface of config.interfaces) {
			iface.failover_interfaces = iface.failover_interfaces.filter(
				(name) => name !== ifaceName,
			);
		}
	} else {
		toast.error("At least one interface is required");
	}
}

function isInterfaceExpanded(interfaceName: string) {
	return Boolean(expandedInterfaces[interfaceName]);
}

function toggleInterfaceExpanded(interfaceName: string) {
	expandedInterfaces = {
		...expandedInterfaces,
		[interfaceName]: !expandedInterfaces[interfaceName],
	};
}

function addGlobalFailoverInterface() {
	if (config) {
		config.failover_interfaces = [...config.failover_interfaces, ""];
	}
}

function removeGlobalFailoverInterface(index: number) {
	if (config) {
		config.failover_interfaces = config.failover_interfaces.filter(
			(_, i) => i !== index,
		);
	}
}

function addInterfaceFailoverTarget(interfaceIndex: number) {
	if (config) {
		config.interfaces[interfaceIndex].failover_interfaces = [
			...config.interfaces[interfaceIndex].failover_interfaces,
			"",
		];
	}
}

function removeInterfaceFailoverTarget(
	interfaceIndex: number,
	targetIndex: number,
) {
	if (config) {
		config.interfaces[interfaceIndex].failover_interfaces = config.interfaces[
			interfaceIndex
		].failover_interfaces.filter((_, index) => index !== targetIndex);
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

function healthStatusesForInterface(interfaceName: string) {
	return interfaceHealth.filter((status) => status.interface === interfaceName);
}

function formatLatency(status: InterfaceHealthStatus) {
	return status.latency_ms === null ? "—" : `${status.latency_ms.toFixed(1)}ms`;
}

function formatLoss(status: InterfaceHealthStatus) {
	return `${status.packet_loss_percent.toFixed(1)}%`;
}

function formatUpdated(status: InterfaceHealthStatus) {
	if (!status.updated_at_ms) return "never";
	return new Date(status.updated_at_ms).toLocaleTimeString();
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

    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2 mt-8">Export</h2>
    <div class="grid grid-cols-1 md:grid-cols-2 gap-x-8 gap-y-6">
      <div class="flex items-center gap-4 h-fit">
        <input
                id="export_enabled"
                type="checkbox"
                bind:checked={config.export_enabled}
                class="monadns-checkbox"
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

    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2 mt-8">High Availability</h2>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-6 mt-4">
      <div class="flex flex-col gap-1">
        <label for="health-interval" class="text-sm font-bold text-zinc-300">Health Check Interval (seconds)</label>
        <p class="text-xs text-zinc-500 mb-1">Time between checks for every interface and host.</p>
        <input id="health-interval" type="number" min="1" bind:value={config.health_check_interval_seconds} class="bg-zinc-900 border border-zinc-700 p-2 text-sm focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="health-timeout" class="text-sm font-bold text-zinc-300">Health Check Timeout (seconds)</label>
        <p class="text-xs text-zinc-500 mb-1">Maximum duration allowed for one ping batch.</p>
        <input id="health-timeout" type="number" min="1" bind:value={config.health_check_timeout_seconds} class="bg-zinc-900 border border-zinc-700 p-2 text-sm focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="health-ping-count" class="text-sm font-bold text-zinc-300">Ping Count</label>
        <p class="text-xs text-zinc-500 mb-1">ICMP echo requests sent to each host per check.</p>
        <input id="health-ping-count" type="number" min="1" bind:value={config.health_check_ping_count} class="bg-zinc-900 border border-zinc-700 p-2 text-sm focus:outline-none focus:border-zinc-500" />
      </div>
      <div class="flex flex-col gap-1">
        <label for="failover-recovery-delay" class="text-sm font-bold text-zinc-300">Failback Delay (seconds)</label>
        <p class="text-xs text-zinc-500 mb-1">Recovered interfaces must stay healthy this long before traffic switches back.</p>
        <input id="failover-recovery-delay" type="number" min="0" bind:value={config.failover_recovery_delay_seconds} class="bg-zinc-900 border border-zinc-700 p-2 text-sm focus:outline-none focus:border-zinc-500" />
      </div>
    </div>

    <div class="border border-zinc-900 bg-zinc-950/40 p-4 space-y-3 max-w-2xl">
      <div class="flex items-center justify-between gap-4">
        <div>
          <h3 class="text-sm font-bold text-zinc-300 uppercase tracking-widest">Global Failover Order</h3>
          <p class="text-xs text-zinc-500">When an interface uses global failover and becomes unhealthy, the first healthy interface in this preference list is used.</p>
        </div>
        <button
          type="button"
          onclick={addGlobalFailoverInterface}
          class="text-xs font-bold border border-zinc-700 px-3 py-1 hover:bg-zinc-800"
        >
          Add
        </button>
      </div>
      {#if config.failover_interfaces.length === 0}
        <p class="text-xs text-zinc-600">Empty list falls back to the configured interface order.</p>
      {/if}
      {#each config.failover_interfaces as _, failoverIndex}
        <div class="flex items-center gap-2">
          <span class="w-6 text-xs font-mono text-zinc-600">{failoverIndex + 1}</span>
          <select
            aria-label="Global failover interface"
            bind:value={config.failover_interfaces[failoverIndex]}
            class="w-full bg-zinc-900 border border-zinc-700 p-2 text-sm focus:outline-none focus:border-zinc-500"
          >
            <option value="">Select interface</option>
            {#each config.interfaces as iface}
              <option value={iface.name}>{iface.name}</option>
            {/each}
          </select>
          <button
            type="button"
            onclick={() => removeGlobalFailoverInterface(failoverIndex)}
            class="text-red-500 hover:text-red-400 p-2"
          >
            <Trash />
          </button>
        </div>
      {/each}
    </div>

    <h2 class="text-xl font-bold border-b border-zinc-800 pb-2 mt-8">Interfaces</h2>
    
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
          {@const healthStatuses = healthStatusesForInterface(iface.name)}
          {@const expanded = isInterfaceExpanded(iface.name)}
          <div class="border border-zinc-800 bg-zinc-950/30 px-6 {expanded ? 'py-6 space-y-4' : 'py-4'}">
            <div class="flex items-center justify-between gap-4">
              <button
                type="button"
                aria-expanded={expanded}
                onclick={() => toggleInterfaceExpanded(iface.name)}
                class="-m-2 flex min-w-0 flex-1 flex-wrap items-center gap-2 p-2 text-left"
              >
                <span class="text-zinc-500 text-xs font-mono transition-transform {expanded ? 'rotate-90' : ''}">›</span>
                <h3 class="font-bold text-white uppercase tracking-widest text-xs">
                  Interface: {iface.name} {#if iface.name === config.default_interface}(Default){/if}
                </h3>

                {#if healthError}
                  <span class="rounded-full border border-red-900/60 bg-red-950/30 px-2 py-0.5 text-[10px] font-bold uppercase tracking-widest text-red-400" title={healthError}>
                    Health unavailable
                  </span>
                {:else if !iface.health_check_enabled}
                  <span class="rounded-full border border-zinc-800 bg-zinc-900 px-2 py-0.5 text-[10px] font-bold uppercase tracking-widest text-zinc-500">
                    Health off
                  </span>
                {:else if healthStatuses.length === 0}
                  <span class="rounded-full border border-zinc-800 bg-zinc-900 px-2 py-0.5 text-[10px] font-bold uppercase tracking-widest text-zinc-500">
                    Waiting
                  </span>
                {:else if healthStatuses.every((status) => status.healthy)}
                  <span class="rounded-full border border-emerald-900/70 bg-emerald-950/30 px-2 py-0.5 text-[10px] font-bold uppercase tracking-widest text-emerald-400">
                    Healthy
                  </span>
                {:else}
                  <span class="rounded-full border border-red-900/70 bg-red-950/30 px-2 py-0.5 text-[10px] font-bold uppercase tracking-widest text-red-400">
                    Unhealthy
                  </span>
                {/if}

                {#each healthStatuses as status}
                  <span
                    class="inline-flex max-w-full items-center gap-1 rounded-full border px-2 py-0.5 text-[10px] font-mono {status.healthy ? 'border-emerald-900/70 bg-emerald-950/20 text-emerald-300' : 'border-red-900/70 bg-red-950/20 text-red-300'}"
                    title="{status.host}: ping {formatLatency(status)}, loss {formatLoss(status)}, updated {formatUpdated(status)}{status.error ? `, error: ${status.error}` : ''}"
                  >
                    <span class="max-w-36 truncate">{status.host}</span>
                    <span class="text-zinc-500">·</span>
                    <span>{formatLatency(status)}</span>
                    <span class="text-zinc-500">·</span>
                    <span>{formatLoss(status)}</span>
                  </span>
                {/each}
              </button>

              <button onclick={() => removeInterface(i)} class="text-red-500 hover:text-red-400 p-2 transition-colors">
                <Trash />
              </button>
            </div>

            {#if expanded}
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
                    class="monadns-checkbox"
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

              <div class="lg:col-span-3 grid grid-cols-1 lg:grid-cols-3 gap-6 border-t border-zinc-900 pt-4">
                <div class="flex items-center gap-3 h-fit">
                  <input
                    id="iface-health-enabled-{i}"
                    type="checkbox"
                    bind:checked={iface.health_check_enabled}
                    class="monadns-checkbox"
                  />
                  <div>
                    <label for="iface-health-enabled-{i}" class="text-xs font-bold text-zinc-400 uppercase">Health Check</label>
                    <p class="text-xs text-zinc-500">Periodically ping through this interface.</p>
                  </div>
                </div>

                <div class="flex flex-col gap-2 lg:col-span-2">
                  <div class="flex items-center justify-between gap-4">
                    <div>
                      <span class="text-xs font-bold text-zinc-400 uppercase">Health Check Hosts</span>
                      <p class="text-xs text-zinc-500">IPv4/IPv6 addresses or hostnames to ping.</p>
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
                  <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
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
                </div>
              </div>

              <div class="lg:col-span-3 grid grid-cols-1 md:grid-cols-2 gap-6">
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

              <div class="lg:col-span-3 grid grid-cols-1 lg:grid-cols-3 gap-6 border-t border-zinc-900 pt-4">
                <div class="flex flex-col gap-2">
                  <div>
                    <label for="iface-failover-mode-{i}" class="text-xs font-bold text-zinc-400 uppercase">Failover</label>
                    <p class="text-xs text-zinc-500 mb-1">Controls what happens when this interface is unhealthy.</p>
                  </div>
                  <select
                    id="iface-failover-mode-{i}"
                    value={iface.failover_mode}
                    onchange={(e) => {
                      iface.failover_mode = e.currentTarget.value as FailoverMode;
                    }}
                    class="bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600"
                  >
                    <option value="global">Use global failover order</option>
                    <option value="disabled">No failover</option>
                    <option value="custom">Custom failover order</option>
                  </select>
                </div>

                <div class="flex flex-col gap-2 lg:col-span-2">
                  {#if iface.failover_mode === "custom"}
                    <div class="flex items-center justify-between gap-4">
                      <div>
                        <span class="text-xs font-bold text-zinc-400 uppercase">Custom Failover Order</span>
                        <p class="text-xs text-zinc-500">First healthy interface in this custom order is used.</p>
                      </div>
                      <button
                        type="button"
                        onclick={() => addInterfaceFailoverTarget(i)}
                        class="text-xs font-bold border border-zinc-700 px-3 py-1 hover:bg-zinc-800"
                      >
                        Add
                      </button>
                    </div>
                    {#if iface.failover_interfaces.length === 0}
                      <p class="text-xs text-zinc-600">No custom fallback targets configured.</p>
                    {/if}
                    <div class="grid grid-cols-1 md:grid-cols-2 gap-2">
                      {#each iface.failover_interfaces as _, targetIndex}
                        <div class="flex items-center gap-2">
                          <span class="w-6 text-xs font-mono text-zinc-600">{targetIndex + 1}</span>
                          <select
                            aria-label="Custom failover interface"
                            bind:value={iface.failover_interfaces[targetIndex]}
                            class="w-full bg-zinc-900 border border-zinc-800 p-2 text-sm focus:outline-none focus:border-zinc-600"
                          >
                            <option value="">Select interface</option>
                            {#each config.interfaces as target}
                              <option value={target.name}>{target.name}</option>
                            {/each}
                          </select>
                          <button
                            type="button"
                            onclick={() => removeInterfaceFailoverTarget(i, targetIndex)}
                            class="text-red-500 hover:text-red-400 p-2"
                          >
                            <Trash />
                          </button>
                        </div>
                      {/each}
                    </div>
                  {:else if iface.failover_mode === "global"}
                    <div class="h-full flex items-center text-xs text-zinc-500">
                      Uses the global failover preference list above.
                    </div>
                  {:else}
                    <div class="h-full flex items-center text-xs text-zinc-500">
                      Traffic stays on this interface even when it is unhealthy.
                    </div>
                  {/if}
                </div>
              </div>
            </div>
            {/if}
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

<style>
	.monadns-checkbox {
		appearance: none;
		-webkit-appearance: none;
		background-color: rgb(9 9 11);
		border: 1px solid rgb(63 63 70);
		border-radius: 0.25rem;
		cursor: pointer;
		flex: 0 0 auto;
		height: 1.5rem;
		width: 1.5rem;
		touch-action: manipulation;
	}

	.monadns-checkbox:checked {
		background-color: white;
		background-image: url("data:image/svg+xml,%3csvg viewBox='0 0 16 16' fill='none' xmlns='http://www.w3.org/2000/svg'%3e%3cpath d='M3.5 8.5L6.5 11.5L12.5 4.5' stroke='black' stroke-width='2.25' stroke-linecap='round' stroke-linejoin='round'/%3e%3c/svg%3e");
		background-position: center;
		background-repeat: no-repeat;
		background-size: 1rem 1rem;
		border-color: white;
	}

	.monadns-checkbox:focus-visible {
		outline: 2px solid rgb(212 212 216);
		outline-offset: 2px;
	}

	.monadns-checkbox:disabled {
		cursor: not-allowed;
		opacity: 0.4;
	}
</style>
