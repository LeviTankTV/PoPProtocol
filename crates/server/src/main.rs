//! PoP - Protocol: авторитетный игровой сервер (axum).
//! Шаги 1–6: клиент в бинарнике, /health, /stats, хранилище игроков,
//! лобби и комнаты по WS. Матчевый цикл — шаги 7–8.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use axum::{Json, Router};
use rust_embed::RustEmbed;

mod boss;
mod game;
mod lobby;
mod store;
mod ws;
use lobby::Lobby;
use store::Store;

/// Весь клиент вшивается в бинарник: один файл сервера = вся игра.
#[derive(RustEmbed)]
#[folder = "../../client"]
struct Assets;

fn asset(path: &str) -> Option<Vec<u8>> {
    Assets::get(path).map(|f| f.data.into_owned())
}

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) ws_clients: Arc<AtomicUsize>,
    pub(crate) store: Arc<Store>,
    pub(crate) lobby: Arc<Lobby>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "popproto_server=info".into()),
        )
        .init();

    let store = Arc::new(store::connect_store().await);
    let lobby = Arc::new(Lobby::new(store.clone()));
    let state = AppState {
        ws_clients: Arc::new(AtomicUsize::new(0)),
        store,
        lobby,
    };
    Lobby::spawn_sweeper(state.lobby.as_ref().clone());
    let port: u16 = std::env::var("PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(8080);

    let app = Router::new()
        .route("/", get(index))
        .route("/{file}", get(asset_route))
        .route("/health", get(health))
        .route("/stats", get(stats))
        .route("/api/rooms", get(rooms_rest))
        .route("/api/leaderboard", get(leaderboard_rest))
        .route("/api/maps", get(maps_rest))
        .route("/api/season", get(season_rest))
        .route("/api/profile", get(profile_rest))
        .route("/ws", get(ws::upgrade))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(("0.0.0.0", port))
        .await
        .expect("bind 0.0.0.0");
    tracing::info!("PoP - Protocol server listening on http://0.0.0.0:{port}");
    axum::serve(listener, app).await.expect("server error");
}

async fn index() -> Response {
    let html = asset("index.html").expect("index.html embedded");
    ([("content-type", "text/html; charset=utf-8")], html).into_response()
}

/// Статика клиента из встраиваемых ассетов (proto.js, app.js, ...).
async fn asset_route(axum::extract::Path(file): axum::extract::Path<String>) -> Response {
    let Some(bytes) = asset(&file) else {
        return (axum::http::StatusCode::NOT_FOUND, "нет такого файла").into_response();
    };
    let mime = if file.ends_with(".js") {
        "application/javascript; charset=utf-8"
    } else if file.ends_with(".html") {
        "text/html; charset=utf-8"
    } else {
        "application/octet-stream"
    };
    ([("content-type", mime)], bytes).into_response()
}

async fn health() -> &'static str {
    "ok"
}

async fn stats(State(state): State<AppState>) -> Response {
    let rooms = state.lobby.room_list().await.len();
    Json(serde_json::json!({
        "rooms": rooms,
        "players_online": state.ws_clients.load(Ordering::Relaxed),
        "db": state.store.backend_name(),
        "version": env!("CARGO_PKG_VERSION"),
    }))
    .into_response()
}

/// Снимок списка комнат для веб-клиента (тот же список, что по WS).
async fn rooms_rest(State(state): State<AppState>) -> Response {
    Json(state.lobby.room_list().await).into_response()
}

/// Опубликованные карты редактора.
async fn maps_rest(State(state): State<AppState>) -> Response {
    let maps = state.lobby.maps_list(30).await;
    Json(serde_json::json!(maps
        .iter()
        .map(|m| serde_json::json!({
            "id": m.id, "name": m.name, "author": m.author,
            "rows": m.rows, "plays": m.plays,
        }))
        .collect::<Vec<_>>()))
    .into_response()
}

/// Текущий сезон + топ по ELO.
async fn season_rest(State(state): State<AppState>) -> Response {
    let season = crate::lobby::season_now_pub();
    let top = state.store.elo_top(10).await;
    Json(serde_json::json!({
        "season": season,
        "top": top.iter().map(|e| serde_json::json!({ "name": e.name, "elo": e.elo })).collect::<Vec<_>>(),
    }))
    .into_response()
}

/// Профиль игрока: ?name=Ник.
async fn profile_rest(
    State(state): State<AppState>,
    axum::extract::Query(q): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> Response {
    let Some(name) = q.get("name") else {
        return (axum::http::StatusCode::BAD_REQUEST, "name required").into_response();
    };
    match state.store.profile_by_name(name).await {
        Some(p) => Json(p).into_response(),
        None => (axum::http::StatusCode::NOT_FOUND, "no such player").into_response(),
    }
}

/// Глобальный топ по XP.
async fn leaderboard_rest(State(state): State<AppState>) -> Response {
    let top = state.store.leaderboard(10).await;
    Json(serde_json::json!(top
        .iter()
        .map(|e| serde_json::json!({ "name": e.name, "xp": e.xp }))
        .collect::<Vec<_>>()))
    .into_response()
}
