# <img src="frontend/public/icon.svg" alt="MonaDNS Logo" align="center" height="48"> MonaDNS

MonaDNS is a transparent DNS-based traffic steering tool and resolver. It intercepts DNS queries for configured domains, responds with "Fake IPs", and automatically manages Linux networking rules (`nftables` and `iproute2`) to steer traffic destined to these fake IPs into specific network interfaces and routing tables. 

This functionality provides a lightweight, transparent proxying mechanism akin to the fake-ip features found in tools like Clash or Surge, designed to run directly on Linux routers or gateways.

## Features

- **Fake IP DNS Resolution**: Intercepts DNS requests for specified domains and returns dynamically allocated IPv4 and IPv6 addresses from a configured subnet.
- **Multi-Interface Traffic Steering**: Automatically maintains `nftables` chains and `ip rules` to mark and route traffic destined to the allocated Fake IPs through multiple designated interfaces and routing tables.
- **IP-based Steering**: Beyond DNS-based steering, MonaDNS can also steer traffic based on destination IP subnets.
- **Automated NAT/Masquerade**: Optionally applies SNAT or Masquerade to the steered traffic per interface.
- **TCP MSS Clamping**: Built-in support for MSS clamping to prevent fragmentation issues on tunnel interfaces (e.g., WireGuard).
- **Upstream DNS Support**: Resolves non-intercepted domains via standard upstream resolvers including Quad9, Cloudflare, Google, or custom servers via UDP, DNS-over-TLS (DoT), or DNS-over-HTTPS (DoH).
- **Domain & IP Lists Management**: Supports adding individual domains/subnets or syncing lists from external sources.
- **GeoSite & GeoIP Support**: Integrated support for V2Ray-compatible `geosite.dat` and `geoip.dat` files for bulk domain and IP management.
- **Virtual Geo Protocols**: Reference geo categories directly in your lists using `geosite://<category>` or `geoip://<category>` (e.g., `geosite://google`, `geoip://cn`).
- **Integrated Web Interface**: A modern web UI built with Svelte 5 and TailwindCSS for managing configuration, domains, IPs, and lists.
- **REST API**: Fully documented OpenAPI (Swagger) endpoints for programmatic management.
- **Prometheus Metrics**: Built-in Prometheus exporter for monitoring DNS query metrics and traffic statistics.

## Use Cases

- **Direct Router Installation**: Install MonaDNS directly on a Linux-based router (e.g., OpenWrt) to provide transparent steering for all connected clients.
- **Sidecar Gateway**: Run MonaDNS on a separate device (like a Raspberry Pi) in your network. Set the Fake IP subnets (`198.18.0.0/15` and `fd32:bfcc:fba0:1337::/64` by default) as static routes on your main router pointing to the MonaDNS device. Clients using MonaDNS as their DNS server will have their traffic automatically routed through it for configured domains.
- **VPN Server Smart Routing**: Deploy MonaDNS on a VPN server (WireGuard, OpenVPN). By pushing MonaDNS as the DNS server to VPN clients, you can steer specific client traffic through different exit nodes or local interfaces based on the requested domain or destination IP.

## Architecture

The project is structured into two main components:

### Backend (`resolver/`)
A high-performance Rust application built with `hickory-dns` (for DNS handling), `axum` (for the HTTP API), and `nftables` / `rtnetlink` (for Linux network management).
- Serves as the primary DNS server.
- Manages multiple egress interfaces using `fwmark` and custom routing tables.
- Embeds and serves the pre-built Svelte frontend.
- Stores state and rules in an SQLite database.
- Modifies Linux networking state (requires appropriate capabilities, e.g., `CAP_NET_ADMIN` or `root`).

### Frontend (`frontend/`)
A Single Page Application (SPA) built with Svelte 5 and Vite. It interacts with the backend REST API to allow users to manage the DNS rules, IP rules, subscriptions, and core network configurations.

## Requirements

- **Linux OS**: MonaDNS heavily relies on Linux-specific networking APIs (`nftables` and `rtnetlink`).
- **Root Privileges / Capabilities**: Running the backend requires root access or `CAP_NET_ADMIN` to manipulate network interfaces, routing tables, and firewall rules.
- **Routing Tables**: For each interface you want to steer traffic through, you must ensure a default route exists in the corresponding routing table.
  For example, if you use interface `wg0` with table `100`:
  ```bash
  sudo ip route add default dev wg0 table 100
  ```
  If you use `wg-quick`, you can set `Table = 100` in your Wireguard configuration to add the route automatically.

- **System Settings**:
  ```bash
  sudo sysctl -w net.ipv4.conf.all.rp_filter=0 # Disable reverse path filtering
  sudo sysctl -w net.ipv4.ip_forward=1 # Enable forwarding
  ```

## Configuration

MonaDNS can be configured via environment variables and a TOML configuration file.

### Environment Variables

| Variable               | Description                                                   | Default                    |
|:-----------------------|:--------------------------------------------------------------|:---------------------------|
| `MONADNS_CONFIG_PATH`  | Path to the TOML configuration file                           | `/opt/monadns/config.toml` |
| `MONADNS_DB_PATH`      | Path to the SQLite database                                   | `/opt/monadns/db.sqlite`   |
| `MONADNS_DNS_BIND`     | Address and port to bind the DNS server                       | `[::]:5553`                |
| `MONADNS_HTTP_BIND`    | Address and port to bind the HTTP API / UI                    | `[::]:8080`                |
| `MONADNS_METRICS_BIND` | Optional address to bind the Prometheus exporter              | *(Disabled)*               |
| `MONADNS_API_PASSWORD` | Optional password for configuration (uses `X-Api-Key` header) | *(Disabled, no password)*  |

### Configuration File (`config.toml`)

The configuration file defines the network interfaces, subnets for Fake IPs, and upstream DNS resolvers.
This configuration is fully editable via the Web UI.

```toml
default_interface = "wg0"
ipv4_subnet = "198.18.0.0/15"
ipv6_subnet = "fd32:bfcc:fba0:1337::/64"
export_enabled = false

[[interfaces]]
name = "wg0"
fwmark = 1
table_id = 100
tcp_mss_clamp = 1280
ipv4_snat = "10.10.10.4"

[[interfaces]]
name = "eth0"
fwmark = 2
table_id = 101

[upstream_resolver]
type = "Quad9Https"
```

#### Upstream Resolver Types
- `Quad9Https`, `CloudflareHttps`, `GoogleHttps`
- `Custom`:
  ```toml
  [upstream_resolver]
  type = "Custom"
  nameservers = [
    { addr = "1.1.1.1", protocol = "Plain" },
    { addr = "8.8.8.8", protocol = "Tls", tls_dns_name = "dns.google" }
  ]
  ```

## GeoSite & GeoIP Management

MonaDNS supports the widely used `geosite.dat` and `geoip.dat` binary formats for efficient management of large domain and IP lists.

1.  **Add Geo Sources:** In the "Geo Sources" tab, add URLs to your `.dat` files (e.g., from [v2fly/domain-list-community](https://github.com/v2fly/domain-list-community/releases/latest/download/dlc.dat) or [v2fly/geoip](https://github.com/v2fly/geoip/releases/latest/download/geoip.dat)).
2.  **Use Categories:** Once synced, you can use categories from these files in your Domain or IP lists using the following formats:
    *   `geosite://google` - All domains in the Google category.
    *   `geosite://cn` - All domains associated with China.
    *   `geoip://ir` - All IP subnets for Iran.
3.  **Efficiency:** MonaDNS decodes these files once and stores them in its database. Matching against these lists is highly optimized using a domain specificity fast-path.

> [!NOTE]
> Regex rules are not supported yet.

## API & Documentation

When running, the application exposes a Swagger UI for exploring and testing the REST API. You can access it by navigating to:
`http://<MONADNS_HTTP_BIND>/swagger`

## Docker Support

MonaDNS can be run as a Docker container. Since it needs to manage the host's networking stack (`nftables`, routing tables), it requires elevated privileges and usually runs in host network mode.

### Docker Compose

The easiest way to run MonaDNS is using [Docker Compose](docker-compose.yaml).

### Building the Image

To build the Docker image locally:

```bash
docker build -t monadns .
```

The Dockerfile uses a multi-stage build to compile both the Svelte frontend and the Rust backend, resulting in a lean final image based on Debian.

## Development

### Frontend
```bash
cd frontend
pnpm install
pnpm dev
```

### Backend
```bash
cd resolver
cargo run
```
*(Note: Running the backend typically requires root privileges due to its interaction with `nftables`)*

## Building

> [!NOTE]
> Make sure to build the frontend first, so it gets embedded into the resolver binary.

```bash
cd frontend
pnpm build
```

```bash
cd resolver
cargo build
```
