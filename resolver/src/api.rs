use std::sync::{Arc, LazyLock};
use axum::{
    extract::{Path, State, Request},
    Json,
    routing::{get, post, delete},
    Router,
    middleware::{self, Next},
    response::{IntoResponse, Response},
    http::StatusCode,
};
use axum_embed::ServeEmbed;
use log::error;
use utoipa::OpenApi;
use utoipa_axum::router::OpenApiRouter;
use utoipa_swagger_ui::SwaggerUi;
use frontend::FrontendDist;
use crate::app::App;
use crate::config::{Config, PatchConfig, UpstreamResolverConfig};
use crate::domain_controller::sqlite::{DomainRule, DomainList};

#[derive(OpenApi)]
#[openapi(
    paths(get_config, patch_config, list_domain_rules, add_domain_rule, remove_domain_rule, list_domain_lists, add_domain_list, remove_domain_list, sync_domain_list),
    components(schemas(Config, PatchConfig, UpstreamResolverConfig, DomainRule, DomainList)),
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
        .route("/domains", get(list_domain_rules).post(add_domain_rule))
        .route("/domains/{domain}", delete(remove_domain_rule))
        .route("/lists", get(list_domain_lists).post(add_domain_list))
        .route("/lists/{id}", delete(remove_domain_list))
        .route("/lists/{id}/sync", post(sync_domain_list))
        .with_state(app)
        .split_for_parts();

    let serve_assets = ServeEmbed::<FrontendDist>::new();

    let api_routes = api_routes.layer(middleware::from_fn(auth_middleware));

    Router::new()
        .nest("/api", api_routes)
        .merge(SwaggerUi::new("/swagger").url("/api-docs/openapi.json", openapi))
        .fallback_service(serve_assets)
}

/// Get current configuration
#[utoipa::path(
    get,
    path = "/config",
    responses(
        (status = 200, description = "Current configuration", body = Config)
    )
)]
async fn get_config(
    State(app): State<Arc<App>>,
) -> Json<Config> {
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
async fn list_domain_rules(
    State(app): State<Arc<App>>,
) -> Result<Json<Vec<DomainRule>>, String> {
    app.domain_controller().list_rules().await
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
    app.domain_controller().add_rule(&rule.domain, rule.include_subdomains).await
        .map(|_| Json("Domain rule added".to_string()))
        .map_err(|e| e.to_string())
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
    app.domain_controller().remove_rule(&domain).await
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
async fn list_domain_lists(
    State(app): State<Arc<App>>,
) -> Result<Json<Vec<DomainList>>, String> {
    app.domain_controller().list_domain_lists().await
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
    let list_id = app.domain_controller().add_domain_list(list).await
        .map_err(|e| e.to_string())?;

    tokio::spawn(async move {
        // Update after added
        if let Err(e) = app.domain_controller().sync_list_by_id(list_id).await {
            error!("Failed to initial sync for list {}: {}", list_id, e);
        }
    });

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
    app.domain_controller().remove_domain_list(id).await
        .map(|_| Json("Domain list removed".to_string()))
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
    tokio::spawn(async move {
        if let Err(e) = app.domain_controller().sync_list_by_id(id).await {
            error!("Failed to sync list {}: {}", id, e);
        }
    });

    Ok(Json("Domain list sync started".to_string()))
}
