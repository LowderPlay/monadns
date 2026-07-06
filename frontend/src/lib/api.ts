import { auth } from "./auth_state.svelte";

export type ResolverProtocol = "Plain" | "Tls" | "Https";
export type FailoverMode = "global" | "disabled" | "custom";

export interface CustomNameserverConfig {
	addr: string;
	protocol: ResolverProtocol;
	tls_dns_name: string | null;
}

export type UpstreamResolverConfig =
	| { type: "Quad9Https" }
	| { type: "CloudflareHttps" }
	| { type: "GoogleHttps" }
	| { type: "Custom"; nameservers: CustomNameserverConfig[] };

export interface InterfaceConfig {
	name: string;
	fwmark: number;
	table_id: number;
	tcp_mss_clamp: number | null;
	ipv4_snat: string | null;
	ipv6_snat: string | null;
	health_check_enabled: boolean;
	health_check_hosts: string[];
	health_check_latency_threshold_ms: number;
	health_check_packet_loss_threshold_percent: number;
	failover_mode: FailoverMode;
	failover_interfaces: string[];
}

export interface Config {
	interfaces: InterfaceConfig[];
	default_interface: string;
	ipv4_subnet: string;
	ipv6_subnet: string;
	upstream_resolver: UpstreamResolverConfig;
	export_enabled: boolean;
	health_check_interval_seconds: number;
	health_check_timeout_seconds: number;
	health_check_ping_count: number;
	failover_recovery_delay_seconds: number;
	failover_interfaces: string[];
}

export interface PatchConfig {
	interfaces?: InterfaceConfig[];
	default_interface?: string;
	ipv4_subnet?: string;
	ipv6_subnet?: string;
	upstream_resolver?: UpstreamResolverConfig;
	export_enabled?: boolean;
	health_check_interval_seconds?: number;
	health_check_timeout_seconds?: number;
	health_check_ping_count?: number;
	failover_recovery_delay_seconds?: number;
	failover_interfaces?: string[];
}

export interface DomainRule {
	id?: number;
	domain: string;
	include_subdomains: boolean;
	interface: string | null;
}

export interface DomainList {
	id?: number;
	url: string;
	update_interval_seconds: number;
	include_subdomains: boolean;
	last_updated: string | null;
	interface: string | null;
	priority: number;
}

export interface IpRule {
	id?: number;
	subnet: string;
	interface: string | null;
}

export interface IpList {
	id?: number;
	url: string;
	update_interval_seconds: number;
	last_updated: string | null;
	interface: string | null;
	priority: number;
}

export interface GeoSource {
	id?: number;
	url: string;
	type: "geosite" | "geoip";
	update_interval_seconds: number;
	last_updated: string | null;
}

export interface AvailableGeoOptions {
	geosite: string[];
	geoip: string[];
}

export interface InterfaceHealthStatus {
	interface: string;
	host: string;
	enabled: boolean;
	healthy: boolean;
	latency_ms: number | null;
	packet_loss_percent: number;
	healthy_since_ms: number | null;
	last_unhealthy_at_ms: number | null;
	updated_at_ms: number | null;
	error: string | null;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
	const headers = new Headers(options.headers);
	if (auth.key) {
		headers.set("X-Api-Key", auth.key);
	}
	if (options.body && typeof options.body === "string") {
		headers.set("Content-Type", "application/json");
	}

	const response = await fetch(path, { ...options, headers });

	if (response.status === 401 || response.status === 403) {
		auth.needsLogin = true;
		throw new Error("Unauthorized");
	}

	if (!response.ok) {
		const error = await response.text();
		throw new Error(error || response.statusText);
	}

	const contentType = response.headers.get("Content-Type");
	if (contentType && contentType.includes("application/json")) {
		return response.json();
	}
	return response.text() as unknown as T;
}

export const api = {
	getConfig: () => request<Config>("/api/config"),
	patchConfig: (patch: PatchConfig) =>
		request<string>("/api/config", {
			method: "PATCH",
			body: JSON.stringify(patch),
		}),
	getInterfaceHealth: () =>
		request<InterfaceHealthStatus[]>("/api/health/interfaces"),

	getDomains: () => request<DomainRule[]>("/api/domains"),
	addDomain: (rule: DomainRule) =>
		request<string>("/api/domains", {
			method: "POST",
			body: JSON.stringify(rule),
		}),
	removeDomain: (domain: string) =>
		request<string>(`/api/domains/${domain}`, { method: "DELETE" }),

	getLists: () => request<DomainList[]>("/api/lists"),
	addList: (list: DomainList) =>
		request<string>("/api/lists", {
			method: "POST",
			body: JSON.stringify(list),
		}),
	removeList: (id: number) =>
		request<string>(`/api/lists/${id}`, { method: "DELETE" }),
	syncList: (id: number) =>
		request<string>(`/api/lists/${id}/sync`, { method: "POST" }),
	reorderLists: (ids: number[]) =>
		request<string>("/api/lists/reorder", {
			method: "POST",
			body: JSON.stringify(ids),
		}),

	getIps: () => request<IpRule[]>("/api/ips"),
	addIp: (rule: IpRule) =>
		request<string>("/api/ips", { method: "POST", body: JSON.stringify(rule) }),
	removeIp: (subnet: string) =>
		request<string>(`/api/ips/${encodeURIComponent(subnet)}`, {
			method: "DELETE",
		}),

	getIpLists: () => request<IpList[]>("/api/ip-lists"),
	addIpList: (list: IpList) =>
		request<string>("/api/ip-lists", {
			method: "POST",
			body: JSON.stringify(list),
		}),
	removeIpList: (id: number) =>
		request<string>(`/api/ip-lists/${id}`, { method: "DELETE" }),
	syncIpList: (id: number) =>
		request<string>(`/api/ip-lists/${id}/sync`, { method: "POST" }),
	reorderIpLists: (ids: number[]) =>
		request<string>("/api/ip-lists/reorder", {
			method: "POST",
			body: JSON.stringify(ids),
		}),

	getGeoSources: () => request<GeoSource[]>("/api/geo-sources"),
	addGeoSource: (source: GeoSource) =>
		request<string>("/api/geo-sources", {
			method: "POST",
			body: JSON.stringify(source),
		}),
	removeGeoSource: (id: number) =>
		request<string>(`/api/geo-sources/${id}`, { method: "DELETE" }),
	syncGeoSource: (id: number) =>
		request<string>(`/api/geo-sources/${id}/sync`, { method: "POST" }),
	getGeoOptions: () => request<AvailableGeoOptions>("/api/geo-options"),
};
