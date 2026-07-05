use crate::app::App;
use crate::config::{Config, PatchConfig, UpstreamResolverConfig};
use crate::domain_controller::PolicyId;
use crate::domain_controller::sqlite::{DomainList, DomainRule, GeoSource, IpList, IpRule};
use crate::health_check::InterfaceHealthStatus;
use axum::{
    Json, Router,
    extract::{Path, Request, State},
    http::StatusCode,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::{delete, get, post},
};
use axum_embed::ServeEmbed;
use frontend::FrontendDist;
use log::error;
use serde::Serialize;
use std::sync::{Arc, LazyLock};
use utoipa::{OpenApi, ToSchema};
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;

#[derive(Serialize, ToSchema)]
pub struct AvailableGeoOptions {
    pub geosite: Vec<String>,
    pub geoip: Vec<String>,
}

async fn set_policy_mark_for_interface(
    app: &Arc<App>,
    policy_id: PolicyId,
    interface: Option<&str>,
) -> Result<(), String> {
    let fwmark = app.current_config().resolve_interface(interface).fwmark;
    let route_controller = app.handler().state.load().route_controller.clone();
    route_controller
        .set_policy_mark(policy_id, fwmark)
        .await
        .map_err(|e| e.to_string())
}

#[derive(OpenApi)]
#[openapi(
    paths(
        get_config, patch_config, get_interface_health,
        list_domain_rules, add_domain_rule, remove_domain_rule, 
        list_domain_lists, add_domain_list, remove_domain_list, sync_domain_list, reorder_domain_lists,
        list_ip_rules, add_ip_rule, remove_ip_rule,
        list_ip_lists, add_ip_list, remove_ip_list, sync_ip_list, reorder_ip_lists,
        list_geo_sources, add_geo_source, remove_geo_source, sync_geo_source, get_geo_options,
        export_domains, export_ips
    ),
    components(schemas(Config, PatchConfig, UpstreamResolverConfig, DomainRule, DomainList, IpRule, IpList, GeoSource, AvailableGeoOptions, InterfaceHealthStatus)),
    modifiers(&SecurityAddon),
    security(
        ("api_key" = [])
    ),
    servers(
        (url = "/api")
    )
)]
pub struct ApiDoc;

struct SecurityAddon;

impl utoipa::Modify for SecurityAddon {
    fn modify(&self, openapi: &mut utoipa::openapi::OpenApi) {
        if let Some(components) = openapi.components.as_mut() {
            components.add_security_scheme(
                "api_key",
                utoipa::openapi::security::SecurityScheme::ApiKey(
                    utoipa::openapi::security::ApiKey::Header(
                        utoipa::openapi::security::ApiKeyValue::new("X-Api-Key"),
                    ),
                ),
            );
        }
    }
}

static API_PASSWORD: LazyLock<Option<String>> = LazyLock::new(|| {
    let password = std::env::var("MONADNS_API_PASSWORD").ok();
    if password.is_none() {
        log::warn!("MONADNS_API_PASSWORD not set, API is open!");
    }

    password
});

async fn auth_middleware(req: Request, next: Next) -> Response {
    if let Some(expected) = &*API_PASSWORD {
        let auth_header = req.headers().get("X-Api-Key").and_then(|h| h.to_str().ok());
        if auth_header != Some(&expected) {
            return (StatusCode::UNAUTHORIZED, "Invalid API Key").into_response();
        }
    }

    next.run(req).await
}

pub fn create_router(app: Arc<App>) -> Router {
    let (api_routes, openapi) = OpenApiRouter::with_openapi(ApiDoc::openapi())
        .route("/config", get(get_config).patch(patch_config))
        .route("/health/interfaces", get(get_interface_health))
        .route("/domains", get(list_domain_rules).post(add_domain_rule))
        .route("/domains/{domain}", delete(remove_domain_rule))
        .route("/lists", get(list_domain_lists).post(add_domain_list))
        .route("/lists/reorder", post(reorder_domain_lists))
        .route("/lists/{id}", delete(remove_domain_list))
        .route("/lists/{id}/sync", post(sync_domain_list))
        .route("/ips", get(list_ip_rules).post(add_ip_rule))
        .route("/ips/{*subnet}", delete(remove_ip_rule))
        .route("/ip-lists", get(list_ip_lists).post(add_ip_list))
        .route("/ip-lists/reorder", post(reorder_ip_lists))
        .route("/ip-lists/{id}", delete(remove_ip_list))
        .route("/ip-lists/{id}/sync", post(sync_ip_list))
        .route("/geo-sources", get(list_geo_sources).post(add_geo_source))
        .route("/geo-sources/{id}", delete(remove_geo_source))
        .route("/geo-sources/{id}/sync", post(sync_geo_source))
        .route("/geo-options", get(get_geo_options))
        .with_state(app.clone())
        .split_for_parts();

    let export_routes = Router::new()
        .route("/domains.lst", get(export_domains))
        .route("/ips.lst", get(export_ips))
        .with_state(app);

    let serve_assets = ServeEmbed::<FrontendDist>::new();

    let api_routes = api_routes.layer(middleware::from_fn(auth_middleware));

    Router::new()
        .nest("/api", api_routes)
        .nest("/api/export", export_routes)
        .merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", openapi))
        .fallback_service(serve_assets)
}

/// Get current interface health check status
#[utoipa::path(
    get,
    path = "/health/interfaces",
    responses(
        (status = 200, description = "Interface health check status", body = [InterfaceHealthStatus])
    )
)]
async fn get_interface_health(State(app): State<Arc<App>>) -> Json<Vec<InterfaceHealthStatus>> {
    Json(app.interface_health_statuses())
}

/// Get current configuration
#[utoipa::path(
    get,
    path = "/config",
    responses(
        (status = 200, description = "Current configuration", body = Config)
    )
)]
async fn get_config(State(app): State<Arc<App>>) -> Json<Config> {
    Json((*app.current_config()).clone())
}

/// Patch configuration
#[utoipa::path(
    patch,
    path = "/config",
    request_body = PatchConfig,
    responses(
        (status = 200, description = "Configuration updated", body = String),
        (status = 500, description = "Failed to update configuration", body = String)
    )
)]
async fn patch_config(
    State(app): State<Arc<App>>,
    Json(patch): Json<PatchConfig>,
) -> Result<Json<String>, String> {
    match app.patch_config(patch).await {
        Ok(_) => Ok(Json("Config updated".to_string())),
        Err(e) => Err(format!("Failed to update config: {}", e)),
    }
}

/// List all domain rules
#[utoipa::path(
    get,
    path = "/domains",
    responses(
        (status = 200, description = "List of domain rules", body = [DomainRule])
    )
)]
async fn list_domain_rules(State(app): State<Arc<App>>) -> Result<Json<Vec<DomainRule>>, String> {
    app.controller()
        .list_rules()
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Add or update a domain rule
#[utoipa::path(
    post,
    path = "/domains",
    request_body = DomainRule,
    responses(
        (status = 200, description = "Domain rule added or updated", body = String),
        (status = 500, description = "Failed to add domain rule", body = String)
    )
)]
async fn add_domain_rule(
    State(app): State<Arc<App>>,
    Json(rule): Json<DomainRule>,
) -> Result<Json<String>, String> {
    let rule_id = app
        .controller()
        .add_rule(&rule.domain, rule.include_subdomains, rule.interface.clone())
        .await
        .map_err(|e| e.to_string())?;
    set_policy_mark_for_interface(
        &app,
        PolicyId::DomainRule(rule_id),
        rule.interface.as_deref(),
    )
    .await?;
    Ok(Json("Domain rule added".to_string()))
}

/// Remove a domain rule
#[utoipa::path(
    delete,
    path = "/domains/{domain}",
    params(
        ("domain" = String, Path, description = "Domain to remove")
    ),
    responses(
        (status = 200, description = "Domain rule removed", body = String),
        (status = 500, description = "Failed to remove domain rule", body = String)
    )
)]
async fn remove_domain_rule(
    State(app): State<Arc<App>>,
    Path(domain): Path<String>,
) -> Result<Json<String>, String> {
    app.controller()
        .remove_rule(&domain)
        .await
        .map(|_| Json("Domain rule removed".to_string()))
        .map_err(|e| e.to_string())
}

/// List all domain lists
#[utoipa::path(
    get,
    path = "/lists",
    responses(
        (status = 200, description = "List of domain lists", body = [DomainList])
    )
)]
async fn list_domain_lists(State(app): State<Arc<App>>) -> Result<Json<Vec<DomainList>>, String> {
    app.controller()
        .list_domain_lists()
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Add a domain list
#[utoipa::path(
    post,
    path = "/lists",
    request_body = DomainList,
    responses(
        (status = 200, description = "Domain list added", body = String),
        (status = 500, description = "Failed to add domain list", body = String)
    )
)]
async fn add_domain_list(
    State(app): State<Arc<App>>,
    Json(list): Json<DomainList>,
) -> Result<Json<String>, String> {
    let interface = list.interface.clone();
    let updating = list.id.is_some();
    let list_id = app
        .controller()
        .add_domain_list(list)
        .await
        .map_err(|e| e.to_string())?;
    set_policy_mark_for_interface(&app, PolicyId::DomainList(list_id), interface.as_deref())
        .await?;

    if !updating {
        let controller = app.controller();
        tokio::spawn(async move {
            // Update after added
            if let Err(e) = controller.sync_list_by_id(list_id).await {
                error!("Failed to initial sync for list {}: {}", list_id, e);
            }
        });
    }

    Ok(Json(format!("Domain list added with id {}", list_id)))
}

/// Remove a domain list
#[utoipa::path(
    delete,
    path = "/lists/{id}",
    params(
        ("id" = i64, Path, description = "ID of the domain list to remove")
    ),
    responses(
        (status = 200, description = "Domain list removed", body = String),
        (status = 500, description = "Failed to remove domain list", body = String)
    )
)]
async fn remove_domain_list(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    app.controller()
        .remove_domain_list(id)
        .await
        .map(|_| Json("Domain list removed".to_string()))
        .map_err(|e| e.to_string())
}

/// Reorder domain lists
#[utoipa::path(
    post,
    path = "/lists/reorder",
    request_body = [i64],
    responses(
        (status = 200, description = "Domain lists reordered", body = String),
        (status = 500, description = "Failed to reorder domain lists", body = String)
    )
)]
async fn reorder_domain_lists(
    State(app): State<Arc<App>>,
    Json(ids): Json<Vec<i64>>,
) -> Result<Json<String>, String> {
    app.controller()
        .reorder_domain_lists(ids)
        .await
        .map(|_| Json("Domain lists reordered".to_string()))
        .map_err(|e| e.to_string())
}

/// Sync a domain list
#[utoipa::path(
    post,
    path = "/lists/{id}/sync",
    params(
        ("id" = i64, Path, description = "ID of the domain list to sync")
    ),
    responses(
        (status = 200, description = "Domain list synced", body = String),
        (status = 500, description = "Failed to sync domain list", body = String)
    )
)]
pub async fn sync_domain_list(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    let controller = app.controller();
    tokio::spawn(async move {
        if let Err(e) = controller.sync_list_by_id(id).await {
            error!("Failed to sync list {}: {}", id, e);
        }
    });

    Ok(Json("Domain list sync started".to_string()))
}

/// List all IP rules
#[utoipa::path(
    get,
    path = "/ips",
    responses(
        (status = 200, description = "List of IP rules", body = [IpRule])
    )
)]
async fn list_ip_rules(State(app): State<Arc<App>>) -> Result<Json<Vec<IpRule>>, String> {
    app.controller()
        .list_ip_rules()
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Add or update an IP rule
#[utoipa::path(
    post,
    path = "/ips",
    request_body = IpRule,
    responses(
        (status = 200, description = "IP rule added or updated", body = String),
        (status = 500, description = "Failed to add IP rule", body = String)
    )
)]
async fn add_ip_rule(
    State(app): State<Arc<App>>,
    Json(rule): Json<IpRule>,
) -> Result<Json<String>, String> {
    let rule_id = app
        .controller()
        .add_ip_rule(&rule.subnet, rule.interface.clone())
        .await
        .map_err(|e| e.to_string())?;
    set_policy_mark_for_interface(&app, PolicyId::IpRule(rule_id), rule.interface.as_deref())
        .await?;
    Ok(Json("IP rule added".to_string()))
}

/// Remove an IP rule
#[utoipa::path(
    delete,
    path = "/ips/{subnet}",
    params(
        ("subnet" = String, Path, description = "Subnet to remove")
    ),
    responses(
        (status = 200, description = "IP rule removed", body = String),
        (status = 500, description = "Failed to remove IP rule", body = String)
    )
)]
async fn remove_ip_rule(
    State(app): State<Arc<App>>,
    Path(subnet): Path<String>,
) -> Result<Json<String>, String> {
    app.controller()
        .remove_ip_rule(&subnet)
        .await
        .map(|_| Json("IP rule removed".to_string()))
        .map_err(|e| e.to_string())
}

/// List all IP lists
#[utoipa::path(
    get,
    path = "/ip-lists",
    responses(
        (status = 200, description = "List of IP lists", body = [IpList])
    )
)]
async fn list_ip_lists(State(app): State<Arc<App>>) -> Result<Json<Vec<IpList>>, String> {
    app.controller()
        .list_ip_lists()
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Add an IP list
#[utoipa::path(
    post,
    path = "/ip-lists",
    request_body = IpList,
    responses(
        (status = 200, description = "IP list added", body = String),
        (status = 500, description = "Failed to add IP list", body = String)
    )
)]
async fn add_ip_list(
    State(app): State<Arc<App>>,
    Json(list): Json<IpList>,
) -> Result<Json<String>, String> {
    let interface = list.interface.clone();
    let updating = list.id.is_some();
    let list_id = app
        .controller()
        .add_ip_list(list)
        .await
        .map_err(|e| e.to_string())?;
    set_policy_mark_for_interface(&app, PolicyId::IpList(list_id), interface.as_deref()).await?;

    if !updating {
        let controller = app.controller();
        tokio::spawn(async move {
            // Update after added
            if let Err(e) = controller.sync_ip_list_by_id(list_id).await {
                error!("Failed to initial sync for IP list {}: {}", list_id, e);
            }
        });
    }

    Ok(Json(format!("IP list added with id {}", list_id)))
}

/// Remove an IP list
#[utoipa::path(
    delete,
    path = "/ip-lists/{id}",
    params(
        ("id" = i64, Path, description = "ID of the IP list to remove")
    ),
    responses(
        (status = 200, description = "IP list removed", body = String),
        (status = 500, description = "Failed to remove IP list", body = String)
    )
)]
async fn remove_ip_list(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    app.controller()
        .remove_ip_list(id)
        .await
        .map(|_| Json("IP list removed".to_string()))
        .map_err(|e| e.to_string())
}

/// Reorder IP lists
#[utoipa::path(
    post,
    path = "/ip-lists/reorder",
    request_body = [i64],
    responses(
        (status = 200, description = "IP lists reordered", body = String),
        (status = 500, description = "Failed to reorder IP lists", body = String)
    )
)]
async fn reorder_ip_lists(
    State(app): State<Arc<App>>,
    Json(ids): Json<Vec<i64>>,
) -> Result<Json<String>, String> {
    app.controller()
        .reorder_ip_lists(ids)
        .await
        .map(|_| Json("IP lists reordered".to_string()))
        .map_err(|e| e.to_string())
}

/// Sync an IP list
#[utoipa::path(
    post,
    path = "/ip-lists/{id}/sync",
    params(
        ("id" = i64, Path, description = "ID of the IP list to sync")
    ),
    responses(
        (status = 200, description = "IP list sync started", body = String),
        (status = 500, description = "Failed to sync IP list", body = String)
    )
)]
pub async fn sync_ip_list(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    let controller = app.controller();
    tokio::spawn(async move {
        if let Err(e) = controller.sync_ip_list_by_id(id).await {
            error!("Failed to sync IP list {}: {}", id, e);
        }
    });

    Ok(Json("IP list sync started".to_string()))
}

/// List all geo sources
#[utoipa::path(
    get,
    path = "/geo-sources",
    responses(
        (status = 200, description = "List of geo sources", body = [GeoSource])
    )
)]
async fn list_geo_sources(State(app): State<Arc<App>>) -> Result<Json<Vec<GeoSource>>, String> {
    app.controller()
        .list_geo_sources()
        .await
        .map(Json)
        .map_err(|e| e.to_string())
}

/// Add a geo source
#[utoipa::path(
    post,
    path = "/geo-sources",
    request_body = GeoSource,
    responses(
        (status = 200, description = "Geo source added", body = String),
        (status = 500, description = "Failed to add geo source", body = String)
    )
)]
async fn add_geo_source(
    State(app): State<Arc<App>>,
    Json(source): Json<GeoSource>,
) -> Result<Json<String>, String> {
    let source_id = app
        .controller()
        .add_geo_source(source)
        .await
        .map_err(|e| e.to_string())?;

    let controller = app.controller();
    tokio::spawn(async move {
        if let Err(e) = controller.sync_geo_source_by_id(source_id).await {
            error!("Failed to initial sync for geo source {}: {}", source_id, e);
        }
    });

    Ok(Json(format!("Geo source added with id {}", source_id)))
}

/// Remove a geo source
#[utoipa::path(
    delete,
    path = "/geo-sources/{id}",
    params(
        ("id" = i64, Path, description = "ID of the geo source to remove")
    ),
    responses(
        (status = 200, description = "Geo source removed", body = String),
        (status = 500, description = "Failed to remove geo source", body = String)
    )
)]
async fn remove_geo_source(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    app.controller()
        .remove_geo_source(id)
        .await
        .map(|_| Json("Geo source removed".to_string()))
        .map_err(|e| e.to_string())
}

/// Sync a geo source
#[utoipa::path(
    post,
    path = "/geo-sources/{id}/sync",
    params(
        ("id" = i64, Path, description = "ID of the geo source to sync")
    ),
    responses(
        (status = 200, description = "Geo source sync started", body = String),
        (status = 500, description = "Failed to sync geo source", body = String)
    )
)]
pub async fn sync_geo_source(
    State(app): State<Arc<App>>,
    Path(id): Path<i64>,
) -> Result<Json<String>, String> {
    let controller = app.controller();
    tokio::spawn(async move {
        if let Err(e) = controller.sync_geo_source_by_id(id).await {
            error!("Failed to sync geo source {}: {}", id, e);
        }
    });

    Ok(Json("Geo source sync started".to_string()))
}

/// Get available geosite and geoip categories
#[utoipa::path(
    get,
    path = "/geo-options",
    responses(
        (status = 200, description = "Available geo categories", body = AvailableGeoOptions)
    )
)]
async fn get_geo_options(State(app): State<Arc<App>>) -> Result<Json<AvailableGeoOptions>, String> {
    let geosite = app
        .controller()
        .list_geosite_categories()
        .await
        .map_err(|e| e.to_string())?;
    let geoip = app
        .controller()
        .list_geoip_categories()
        .await
        .map_err(|e| e.to_string())?;

    Ok(Json(AvailableGeoOptions { geosite, geoip }))
}

/// Export all domains as a .lst file
#[utoipa::path(
    get,
    path = "/export/domains.lst",
    responses(
        (status = 200, description = "All enabled domains", body = String),
        (status = 403, description = "Export disabled", body = String)
    )
)]
async fn export_domains(State(app): State<Arc<App>>) -> impl IntoResponse {
    let config = app.current_config();
    if !config.export_enabled {
        return (StatusCode::FORBIDDEN, "Export disabled").into_response();
    }

    match app.controller().get_all_domains().await {
        Ok(domains) => {
            let body = domains.join("\n");
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                body,
            )
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

/// Export all IP subnets as a .lst file
#[utoipa::path(
    get,
    path = "/export/ips.lst",
    responses(
        (status = 200, description = "All enabled subnets", body = String),
        (status = 403, description = "Export disabled", body = String)
    )
)]
async fn export_ips(State(app): State<Arc<App>>) -> impl IntoResponse {
    let config = app.current_config();
    if !config.export_enabled {
        return (StatusCode::FORBIDDEN, "Export disabled").into_response();
    }

    match app.controller().get_all_subnets().await {
        Ok(subnets) => {
            let body = subnets
                .into_iter()
                .map(|(subnet, _, _, _)| subnet)
                .collect::<Vec<_>>()
                .join("\n");
            (
                [(
                    axum::http::header::CONTENT_TYPE,
                    "text/plain; charset=utf-8",
                )],
                body,
            )
                .into_response()
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
