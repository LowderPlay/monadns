# <img src="frontend/public/icon.svg" alt="MonaDNS Logo" align="center" height="48"> MonaDNS

MonaDNS is a transparent DNS-based traffic steering tool and resolver. It intercepts DNS queries for configured domains, responds with "Fake IPs", and automatically manages Linux networking rules (`nftables` and `iproute2`) to steer traffic destined to these fake IPs into specific network interfaces and routing tables. 

This functionality provides a lightweight, transparent proxying mechanism akin to the fake-ip features found in tools like Clash or Surge, designed to run directly on Linux routers or gateways.

## Features

- **Fake IP DNS Resolution**: Intercepts DNS requests for specified domains and returns dynamically allocated IPv4 and IPv6 addresses from a configured subnet.
- **Transparent Traffic Steering**: Automatically maintains `nftables` chains and `ip rules` to mark and route traffic destined to the allocated Fake IPs through a designated interface and routing table.
- **Automated NAT/Masquerade**: Optionally applies SNAT or Masquerade to the steered traffic.
- **Upstream DNS Support**: Resolves non-intercepted domains via standard upstream resolvers including Quad9, Cloudflare, Google, or custom servers via UDP, DNS-over-TLS (DoT), or DNS-over-HTTPS (DoH).
- **Domain Lists Management**: Supports adding individual domains or syncing domain lists from external sources.
- **Integrated Web Interface**: A modern web UI built with Svelte 5 and TailwindCSS for managing configuration, domains, and lists.
- **REST API**: Fully documented OpenAPI (Swagger) endpoints for programmatic management.
- **Prometheus Metrics**: Built-in Prometheus exporter for monitoring DNS query metrics.

## Architecture

The project is structured into two main components:

### Backend (`resolver/`)
A high-performance Rust application built with `hickory-dns` (for DNS handling), `axum` (for the HTTP API), and `nftables` / `rtnetlink` (for Linux network management).
- Serves as the primary DNS server.
- Embeds and serves the pre-built Svelte frontend.
- Stores state and rules in an SQLite database.
- Modifies Linux networking state (requires appropriate capabilities, e.g., `CAP_NET_ADMIN` or `root`).

### Frontend (`frontend/`)
A Single Page Application (SPA) built with Svelte 5 and Vite. It interacts with the backend REST API to allow users to manage the DNS rules, domain subscriptions, and core network configurations.

## Requirements

- **Linux OS**: MonaDNS heavily relies on Linux-specific networking APIs (`nftables` and `rtnetlink`).
- **Root Privileges / Capabilities**: Running the backend requires root access or `CAP_NET_ADMIN` to manipulate network interfaces, routing tables, and firewall rules.
- **Add a default route**: The steered packets are routed to IP table `100` (by default, can be changed in UI). Make sure that you add a rule to route packets to a specific interface:
  ```bash
  sudo ip route add default dev wg0 table 100 # replace wg0 with the interface
  ```
  If you use wg-quick, you can set `Table = 100` to add the route automatically.
```bash
sudo sysctl -w net.ipv4.conf.all.rp_filter=0 # Disable reverse path filtering
sudo sysctl -w net.ipv4.ip_forward=1 # Enable forwarding (if you want to 
```

## Environment Variables

The backend can be configured via several environment variables:

| Variable               | Description                                                   | Default                    |
|:-----------------------|:--------------------------------------------------------------|:---------------------------|
| `MONADNS_CONFIG_PATH`  | Path to the TOML configuration file                           | `/opt/monadns/config.toml` |
| `MONADNS_DB_PATH`      | Path to the SQLite database                                   | `/opt/monadns/db.sqlite`   |
| `MONADNS_DNS_BIND`     | Address and port to bind the DNS server                       | `[::]:5553`                |
| `MONADNS_HTTP_BIND`    | Address and port to bind the HTTP API / UI                    | `[::]:8080`                |
| `MONADNS_METRICS_BIND` | Optional address to bind the Prometheus exporter              | *(Disabled)*               |
| `MONADNS_API_PASSWORD` | Optional password for configuration (uses `X-Api-Key` header) | *(Disabled, no password)*  |

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
