//! Хранение игроков и сессий: MongoDB (база `pop_protocol`) или память-фолбэк.
//!
//! Политика входа (спецификация §9): гость по нику + авто-токен.
//! Токен восстанавливает игрока (в т.ч. для реконнекта); чужой ник без
//! валидного токена занять нельзя — защита от перебора ников.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use futures_util::StreamExt;
use mongodb::bson::{doc, Document};
use mongodb::options::IndexOptions;
use mongodb::{Client, IndexModel};
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRecord {
    pub player_id: u32,
    pub name: String,
    pub token: String,
    pub xp: u32,
}

#[derive(Debug, PartialEq)]
pub enum StoreError {
    /// Mongo недоступна или ответила ошибкой.
    Backend(String),
    InvalidName,
    NameTaken,
    BadToken,
}

/// BSON u32: драйвер кладёт Int32 или Int64 в зависимости от значения.
fn get_u32(doc: &Document, key: &str) -> Option<u32> {
    match doc.get(key)? {
        mongodb::bson::Bson::Int32(v) => Some(*v as u32),
        mongodb::bson::Bson::Int64(v) => Some(*v as u32),
        _ => None,
    }
}

fn now_millis() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0)
}

/// Ник: после обрезки пробелов 1–16 символов, не только цифры-пробелы.
pub fn validate_name(name: &str) -> Result<String, StoreError> {
    let trimmed = name.trim();
    let ok = !trimmed.is_empty()
        && trimmed.chars().count() <= 16
        && trimmed.chars().any(|c| !c.is_whitespace())
        && !trimmed.chars().any(|c| c.is_control());
    if ok { Ok(trimmed.to_string()) } else { Err(StoreError::InvalidName) }
}

fn new_token() -> String {
    uuid::Uuid::new_v4().simple().to_string()
}

/// Единая точка входа:enum-диспетчер вместо dyn-трейта (async fn в dyn не работает).
#[derive(Clone)]
pub enum Store {
    Memory(MemoryStore),
    Mongo(MongoStore),
}

impl Store {
    pub fn backend_name(&self) -> &'static str {
        match self {
            Store::Memory(_) => "memory",
            Store::Mongo(_) => "mongodb",
        }
    }

    /// Найти или создать игрока по нику и токену.
    pub async fn resolve_player(&self, name: &str, token: Option<&str>) -> Result<PlayerRecord, StoreError> {
        match self {
            Store::Memory(s) => s.resolve_player(name, token).await,
            Store::Mongo(s) => s.resolve_player(name, token).await,
        }
    }

    /// Начислить XP игроку (за матч). Ботов (id >= BOT_ID_BASE) пропускаем.
    pub async fn add_xp(&self, player_id: u32, delta: u32) {
        if player_id >= popproto_core::BOT_ID_BASE || delta == 0 {
            return;
        }
        match self {
            Store::Memory(s) => s.add_xp(player_id, delta).await,
            Store::Mongo(s) => s.add_xp(player_id, delta).await,
        }
    }

    /// Глобальный топ по XP (спецификация §9).
    pub async fn leaderboard(&self, limit: usize) -> Vec<LeaderEntry> {
        match self {
            Store::Memory(s) => s.leaderboard(limit).await,
            Store::Mongo(s) => s.leaderboard(limit).await,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeaderEntry {
    pub name: String,
    pub xp: u32,
}

/* ---------- память: фолбэк и юнит-тесты ---------- */

#[derive(Default, Clone)]
pub struct MemoryStore {
    inner: std::sync::Arc<Mutex<MemoryInner>>,
}

#[derive(Default)]
struct MemoryInner {
    players: HashMap<u32, PlayerRecord>,
    tokens: HashMap<String, u32>,
    /// Уникальность ников без токена.
    names: HashMap<String, u32>,
    next_id: u32,
}

impl MemoryStore {
    async fn add_xp(&self, player_id: u32, delta: u32) {
        let mut inner = self.inner.lock().await;
        if let Some(rec) = inner.players.get_mut(&player_id) {
            rec.xp += delta;
        }
    }

    async fn leaderboard(&self, limit: usize) -> Vec<LeaderEntry> {
        let inner = self.inner.lock().await;
        let mut list: Vec<LeaderEntry> = inner
            .players
            .values()
            .map(|r| LeaderEntry { name: r.name.clone(), xp: r.xp })
            .collect();
        list.sort_by(|a, b| b.xp.cmp(&a.xp));
        list.truncate(limit);
        list
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    async fn resolve_player(&self, name: &str, token: Option<&str>) -> Result<PlayerRecord, StoreError> {
        let name = validate_name(name)?;
        let mut inner = self.inner.lock().await;

        if let Some(token) = token {
            let Some(&player_id) = inner.tokens.get(token) else {
                return Err(StoreError::BadToken);
            };
            let mut rec = inner.players.get(&player_id).cloned().expect("token -> player invariant");
            if rec.name != name {
                // Смена ника: освободить старый, занять новый, если свободен.
                if inner.names.contains_key(&name) {
                    return Err(StoreError::NameTaken);
                }
                inner.names.remove(&rec.name);
                inner.names.insert(name.clone(), player_id);
                rec.name = name.clone();
                inner.players.insert(player_id, rec.clone());
            }
            return Ok(rec);
        }

        if inner.names.contains_key(&name) {
            return Err(StoreError::NameTaken);
        }
        let player_id = inner.next_id;
        inner.next_id += 1;
        let token = new_token();
        let rec = PlayerRecord { player_id, name: name.clone(), token: token.clone(), xp: 0 };
        inner.players.insert(player_id, rec.clone());
        inner.tokens.insert(token, player_id);
        inner.names.insert(name, player_id);
        Ok(rec)
    }
}

/* ---------- MongoDB ---------- */

#[derive(Clone)]
pub struct MongoStore {
    players: mongodb::Collection<Document>,
    sessions: mongodb::Collection<Document>,
}

impl MongoStore {
    /// Подключение с проверкой живости (ping) и созданием индексов.
    pub async fn connect(url: &str, db_name: &str) -> Result<Self, mongodb::error::Error> {
        let client = Client::with_uri_str(url).await?;
        client.database(db_name).run_command(doc! { "ping": 1 }).await?;
        let db = client.database(db_name);
        let players = db.collection("players");
        let sessions = db.collection("sessions");

        let name_opts = IndexOptions::builder().unique(true).build();
        players
            .create_index(IndexModel::builder().keys(doc! { "name": 1 }).options(name_opts).build())
            .await?;
        sessions
            .create_index(IndexModel::builder().keys(doc! { "token": 1 }).build())
            .await?;

        Ok(Self { players, sessions })
    }

    async fn random_player_id(&self) -> Result<u32, StoreError> {
        // Случайный u32 с проверкой коллизии — без сервиса счётчиков.
        for _ in 0..8 {
            let id: u32 = (uuid::Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
            if id == 0 {
                continue;
            }
            let exists = self
                .players
                .find_one(doc! { "_id": id as i64 })
                .await
                .map_err(|e| StoreError::Backend(e.to_string()))?;
            if exists.is_none() {
                return Ok(id);
            }
        }
        Err(StoreError::Backend("no free player id".into()))
    }

    async fn insert_player(&self, rec: &PlayerRecord) -> Result<(), StoreError> {
        self.players
            .insert_one(doc! {
                "_id": rec.player_id as i64,
                "name": &rec.name,
                "xp": rec.xp as i64,
                "created_at": now_millis() as i64,
            })
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        self.sessions
            .insert_one(doc! {
                "token": &rec.token,
                "player_id": rec.player_id as i64,
                "created_at": now_millis() as i64,
            })
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))?;
        Ok(())
    }

    async fn player_doc(&self, id: u32) -> Result<Option<Document>, StoreError> {
        self.players
            .find_one(doc! { "_id": id })
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))
    }

    async fn resolve_player(&self, name: &str, token: Option<&str>) -> Result<PlayerRecord, StoreError> {
        let name = validate_name(name)?;

        if let Some(token) = token {
            let session = self
                .sessions
                .find_one(doc! { "token": token })
                .await
                .map_err(|e| StoreError::Backend(e.to_string()))?
                .ok_or(StoreError::BadToken)?;
            let player_id = get_u32(&session, "player_id").ok_or_else(|| StoreError::Backend("session.player_id: bad type".into()))?;
            let player = self
                .player_doc(player_id)
                .await?
                .ok_or(StoreError::BadToken)?;
            let current_name: String = player.get_str("name").map_err(|e| StoreError::Backend(e.to_string()))?.into();
            if current_name != name {
                // Смена ника: имя должно быть свободно.
                let taken = self
                    .player_doc_named(&name)
                    .await?
                    .is_some();
                if taken {
                    return Err(StoreError::NameTaken);
                }
                self.players
                    .update_one(doc! { "_id": player_id }, doc! { "$set": { "name": &name } })
                    .await
                    .map_err(|e| StoreError::Backend(e.to_string()))?;
            }
            let xp = get_u32(&player, "xp").unwrap_or(0);
            return Ok(PlayerRecord { player_id, name, token: token.to_string(), xp });
        }

        // Новый гость: ник должен быть свободен.
        if self.player_doc_named(&name).await?.is_some() {
            return Err(StoreError::NameTaken);
        }
        let player_id = self.random_player_id().await?;
        let token = new_token();
        let rec = PlayerRecord { player_id, name, token, xp: 0 };
        self.insert_player(&rec).await?;
        Ok(rec)
    }

    async fn add_xp(&self, player_id: u32, delta: u32) {
        if let Err(e) = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! { "$inc": { "xp": delta as i64 } },
            )
            .await
        {
            tracing::warn!("add_xp: {e}");
        }
    }

    async fn leaderboard(&self, limit: usize) -> Vec<LeaderEntry> {
        use mongodb::options::FindOptions;
        let mut opts = FindOptions::default();
        opts.sort = Some(mongodb::bson::doc! { "xp": -1 });
        opts.limit = Some(limit as i64);
        let Some(mut cursor) = self.players.find(doc! {}).with_options(opts).await.ok() else {
            return vec![];
        };
        let mut docs = vec![];
        while let Some(Ok(d)) = cursor.next().await {
            docs.push(d);
        }
        docs.iter()
            .map(|d| LeaderEntry {
                name: d.get_str("name").unwrap_or("?").to_string(),
                xp: get_u32(d, "xp").unwrap_or(0),
            })
            .collect()
    }

    async fn player_doc_named(&self, name: &str) -> Result<Option<Document>, StoreError> {
        self.players
            .find_one(doc! { "name": name })
            .await
            .map_err(|e| StoreError::Backend(e.to_string()))
    }
}

/* ---------- выбор бэкенда на старте ---------- */

/// Подключиться к MongoDB; при недоступности — уйти в память с предупреждением.
/// URL берётся из MONGO_URL (по умолчанию локальный mongod).
pub async fn connect_store() -> Store {
    let url = std::env::var("MONGO_URL").unwrap_or_else(|_| "mongodb://localhost:27017".into());
    match tokio::time::timeout(std::time::Duration::from_secs(4), MongoStore::connect(&url, "pop_protocol")).await {
        Ok(Ok(store)) => {
            tracing::info!("MongoDB подключена: {url}, база pop_protocol");
            Store::Mongo(store)
        }
        Ok(Err(e)) => {
            tracing::warn!("MongoDB недоступна ({e}); играем на хранилище в памяти — рекорды будут потеряны при рестарте");
            Store::Memory(MemoryStore::new())
        }
        Err(_) => {
            tracing::warn!("MongoDB не ответила за 4 с ({url}); играем на хранилище в памяти");
            Store::Memory(MemoryStore::new())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_new_player_gets_token() {
        let store = MemoryStore::new();
        let rec = store.resolve_player("Тима", None).await.unwrap();
        assert!(!rec.token.is_empty());
        assert_eq!(rec.xp, 0);
    }

    #[tokio::test]
    async fn memory_token_restores_player() {
        let store = MemoryStore::new();
        let first = store.resolve_player("Тима", None).await.unwrap();
        let again = store.resolve_player("Тима", Some(&first.token)).await.unwrap();
        assert_eq!(first.player_id, again.player_id);
    }

    #[tokio::test]
    async fn memory_name_taken_without_token() {
        let store = MemoryStore::new();
        store.resolve_player("Тима", None).await.unwrap();
        assert_eq!(store.resolve_player("Тима", None).await, Err(StoreError::NameTaken));
    }

    #[tokio::test]
    async fn memory_rename_with_token() {
        let store = MemoryStore::new();
        let first = store.resolve_player("Тима", None).await.unwrap();
        let renamed = store.resolve_player("Пума", Some(&first.token)).await.unwrap();
        assert_eq!(renamed.player_id, first.player_id);
        assert_eq!(renamed.name, "Пума");
        // Старое имя освободилось.
        store.resolve_player("Тима", None).await.unwrap();
    }

    #[tokio::test]
    async fn memory_invalid_names() {
        let store = MemoryStore::new();
        assert_eq!(store.resolve_player("   ", None).await, Err(StoreError::InvalidName));
        assert_eq!(store.resolve_player("", None).await, Err(StoreError::InvalidName));
        let long = "ш".repeat(17);
        assert_eq!(store.resolve_player(&long, None).await, Err(StoreError::InvalidName));
    }

    #[tokio::test]
    async fn memory_bad_token() {
        let store = MemoryStore::new();
        assert_eq!(
            store.resolve_player("Тима", Some("нет-такого")).await,
            Err(StoreError::BadToken)
        );
    }

    #[tokio::test]
    async fn memory_xp_and_leaderboard() {
        let store = MemoryStore::new();
        let a = store.resolve_player("Аня", None).await.unwrap();
        let b = store.resolve_player("Боря", None).await.unwrap();
        let bot_id = popproto_core::BOT_ID_BASE; // боты не копят XP
        store.add_xp(a.player_id, 120).await;
        store.add_xp(a.player_id, 30).await;
        store.add_xp(b.player_id, 50).await;
        store.add_xp(bot_id, 999).await;
        let top = store.leaderboard(10).await;
        assert_eq!(top.len(), 2, "боты вне лидерборда");
        assert_eq!(top[0].name, "Аня");
        assert_eq!(top[0].xp, 150);
        // повторное обновление записи: xp сохраняется
        let again = store.resolve_player("Аня", Some(&a.token)).await.unwrap();
        assert_eq!(again.xp, 150);
    }

    /// Интеграционный тест с настоящей MongoDB: запускается только при заданном
    /// MONGO_TEST_URL (например mongodb://localhost:27017).
    #[tokio::test]
    async fn mongo_resolve_roundtrip() {
        let Ok(url) = std::env::var("MONGO_TEST_URL") else {
            eprintln!("MONGO_TEST_URL не задан — mongo-тест пропущен");
            return;
        };
        let Ok(store) = MongoStore::connect(&url, "pop_protocol_test").await else {
            eprintln!("MongoDB недоступна — mongo-тест пропущен");
            return;
        };
        let name = format!("test_{}", uuid::Uuid::new_v4().simple());
        let rec = store.resolve_player(&name, None).await.unwrap();
        let again = store.resolve_player(&name, Some(&rec.token)).await.unwrap();
        assert_eq!(rec.player_id, again.player_id);
        assert_eq!(store.resolve_player(&name, None).await, Err(StoreError::NameTaken));
    }
}
