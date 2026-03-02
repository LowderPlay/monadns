import { auth } from './auth_state.svelte';

export type ResolverProtocol = 'Plain' | 'Tls' | 'Https';

export interface CustomNameserverConfig {
  addr: string;
  protocol: ResolverProtocol;
  tls_dns_name: string | null;
}

export type UpstreamResolverConfig = 
  | { type: 'Quad9Https' } 
  | { type: 'CloudflareHttps' } 
  | { type: 'GoogleHttps' }
  | { type: 'Custom', nameservers: CustomNameserverConfig[] };

export interface Config {
  table_id: number;
  iface: string;
  policy_routing_fwmark: number;
  policy_routing_priority: number;
  tcp_mss_clamp: number | null;
  ipv4_snat: string | null;
  ipv6_snat: string | null;
  ipv4_subnet: string;
  ipv6_subnet: string;
  upstream_resolver: UpstreamResolverConfig;
}

export interface PatchConfig {
  table_id?: number;
  iface?: string;
  policy_routing_fwmark?: number;
  policy_routing_priority?: number;
  tcp_mss_clamp?: number | null;
  ipv4_snat?: string | null;
  ipv6_snat?: string | null;
  ipv4_subnet?: string;
  ipv6_subnet?: string;
  upstream_resolver?: UpstreamResolverConfig;
}

export interface DomainRule {
  domain: string;
  include_subdomains: boolean;
}

export interface DomainList {
  id?: number;
  url: string;
  update_interval_seconds: number;
  include_subdomains: boolean;
  last_updated: string | null;
}

async function request<T>(path: string, options: RequestInit = {}): Promise<T> {
  const headers = new Headers(options.headers);
  if (auth.key) {
    headers.set('X-Api-Key', auth.key);
  }
  if (options.body && !(options.body instanceof FormData)) {
    headers.set('Content-Type', 'application/json');
  }

  const response = await fetch(path, { ...options, headers });

  if (response.status === 401 || response.status === 403) {
    auth.needsLogin = true;
    throw new Error('Unauthorized');
  }

  if (!response.ok) {
    const error = await response.text();
    throw new Error(error || response.statusText);
  }

  if (response.headers.get('Content-Type')?.includes('application/json')) {
    return response.json();
  }
  return response.text() as unknown as T;
}

export const api = {
  getConfig: () => request<Config>('/api/config'),
  patchConfig: (patch: PatchConfig) => request<string>('/api/config', { method: 'PATCH', body: JSON.stringify(patch) }),
  
  getDomains: () => request<DomainRule[]>('/api/domains'),
  addDomain: (rule: DomainRule) => request<string>('/api/domains', { method: 'POST', body: JSON.stringify(rule) }),
  removeDomain: (domain: string) => request<string>(`/api/domains/${domain}`, { method: 'DELETE' }),
  
  getLists: () => request<DomainList[]>('/api/lists'),
  addList: (list: DomainList) => request<string>('/api/lists', { method: 'POST', body: JSON.stringify(list) }),
  removeList: (id: number) => request<string>(`/api/lists/${id}`, { method: 'DELETE' }),
  syncList: (id: number) => request<string>(`/api/lists/${id}/sync`, { method: 'POST' }),
};
