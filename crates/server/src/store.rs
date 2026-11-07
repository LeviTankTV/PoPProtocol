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
use popproto_core::MapInfo;
use tokio::sync::Mutex;

#[derive(Debug, Clone, PartialEq)]
pub struct PlayerRecord {
    pub player_id: u32,
    pub name: String,
    pub token: String,
    pub xp: u32,
    /// Рейтинг ELO (старт 1000), сезон = календарный месяц.
    pub elo: i32,
    /// Надетый скин (каталог в клиенте; 0 = классика).
    pub skin: u8,
    /// Купленные скины.
    pub skins_owned: Vec<u8>,
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

fn get_i32(doc: &Document, key: &str) -> Option<i32> {
    match doc.get(key)? {
        mongodb::bson::Bson::Int32(v) => Some(*v),
        mongodb::bson::Bson::Int64(v) => Some(*v as i32),
        _ => None,
    }
}

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Ник: после обрезки пробелов 1–16 символов, не только цифры-пробелы.
pub fn validate_name(name: &str) -> Result<String, StoreError> {
    let trimmed = name.trim();
    let ok = !trimmed.is_empty()
        && trimmed.chars().count() <= 16
        && trimmed.chars().any(|c| !c.is_whitespace())
        && !trimmed.chars().any(|c| c.is_control());
    if ok {
        Ok(trimmed.to_string())
    } else {
        Err(StoreError::InvalidName)
    }
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

    /// Опубликовать карту редактора; возвращает id.
    pub async fn publish_map(&self, name: &str, author: &str, rows: Vec<String>) -> Option<u32> {
        let id: u32 = (uuid::Uuid::new_v4().as_u128() & 0xFFFF_FFFF) as u32;
        match self {
            Store::Memory(m) => m.publish_map(id, name, author, rows).await,
            Store::Mongo(m) => m.publish_map(id, name, author, rows).await,
        }
    }

    /// Список последних карт.
    pub async fn list_maps(&self, limit: usize) -> Vec<MapInfo> {
        match self {
            Store::Memory(m) => m.list_maps(limit).await,
            Store::Mongo(m) => m.list_maps(limit).await,
        }
    }

    /// Отметка сыгранной партии на карте (резерв: id карты в настройках комнаты).
    #[allow(dead_code)]
    pub async fn map_plays_inc(&self, map_id: u32) {
        match self {
            Store::Memory(m) => m.map_plays_inc(map_id).await,
            Store::Mongo(m) => m.map_plays_inc(map_id).await,
        }
    }

    /// Найти или создать игрока по нику и токену.
    pub async fn resolve_player(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<PlayerRecord, StoreError> {
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

    /// Результат матча: обновить ELO и записать в историю.
    pub async fn match_result(
        &self,
        player_id: u32,
        place: u32,
        total: u32,
        score: u32,
        elo_delta: i32,
        season: &str,
    ) {
        if player_id >= popproto_core::BOT_ID_BASE {
            return;
        }
        match self {
            Store::Memory(s) => {
                s.match_result(player_id, place, total, score, elo_delta, season)
                    .await
            }
            Store::Mongo(s) => {
                s.match_result(player_id, place, total, score, elo_delta, season)
                    .await
            }
        }
    }

    /// Топ сезона по ELO.
    pub async fn elo_top(&self, limit: usize) -> Vec<LeaderEntry> {
        match self {
            Store::Memory(s) => s.elo_top(limit).await,
            Store::Mongo(s) => s.elo_top(limit).await,
        }
    }

    /// Профиль игрока по id.
    pub async fn profile(&self, player_id: u32) -> Option<Profile> {
        match self {
            Store::Memory(s) => s.profile(player_id).await,
            Store::Mongo(s) => s.profile(player_id).await,
        }
    }

    /// Прогресс квестов дня.
    #[allow(dead_code)]
    pub async fn quests_view(&self, player_id: u32) -> (Vec<QuestView>, u32) {
        match self {
            Store::Memory(m) => m.quests_view(player_id).await,
            Store::Mongo(m) => m.quests_view(player_id).await,
        }
    }

    /// Обновить прогресс квестов после матча.
    pub async fn quest_bump(&self, player_id: u32, bricks: u32, win: bool, score: u32) {
        if player_id >= popproto_core::BOT_ID_BASE {
            return;
        }
        match self {
            Store::Memory(m) => m.quest_bump(player_id, bricks, win, score).await,
            Store::Mongo(m) => m.quest_bump(player_id, bricks, win, score).await,
        }
    }

    /// Забрать награду квеста.
    pub async fn quest_claim(&self, player_id: u32, idx: usize) -> Result<u32, String> {
        match self {
            Store::Memory(m) => m.quest_claim(player_id, idx).await,
            Store::Mongo(m) => m.quest_claim(player_id, idx).await,
        }
    }

    /// Создать клан (создатель становится участником).
    pub async fn clan_create(
        &self,
        owner_id: u32,
        owner_name: &str,
        tag: &str,
        name: &str,
    ) -> Result<(), String> {
        match self {
            Store::Memory(m) => m.clan_create(owner_id, owner_name, tag, name).await,
            Store::Mongo(m) => m.clan_create(owner_id, owner_name, tag, name).await,
        }
    }

    /// Вступить в клан по тегу.
    pub async fn clan_join(&self, player_id: u32, tag: &str) -> Result<(), String> {
        match self {
            Store::Memory(m) => m.clan_join(player_id, tag).await,
            Store::Mongo(m) => m.clan_join(player_id, tag).await,
        }
    }

    /// Покинуть клан.
    pub async fn clan_leave(&self, player_id: u32) -> Result<(), String> {
        match self {
            Store::Memory(m) => m.clan_leave(player_id).await,
            Store::Mongo(m) => m.clan_leave(player_id).await,
        }
    }

    /// Клан игрока (None — без клана).
    #[allow(dead_code)]
    pub async fn clan_of(&self, player_id: u32) -> Option<ClanInfo> {
        match self {
            Store::Memory(m) => m.clan_of(player_id).await,
            Store::Mongo(m) => m.clan_of(player_id).await,
        }
    }

    /// Топ кланов по суммарному XP.
    pub async fn clan_top(&self, limit: usize) -> Vec<ClanInfo> {
        match self {
            Store::Memory(m) => m.clan_top(limit).await,
            Store::Mongo(m) => m.clan_top(limit).await,
        }
    }

    /// Купить скин за XP. Ok(true) — куплен, Ok(false) — уже есть, Err — не хватает XP.
    pub async fn buy_skin(&self, player_id: u32, skin: u8, price: u32) -> Result<bool, String> {
        match self {
            Store::Memory(m) => m.buy_skin(player_id, skin, price).await,
            Store::Mongo(m) => m.buy_skin(player_id, skin, price).await,
        }
    }

    /// Надеть скин (только из купленных; 0 всегда доступен).
    pub async fn equip_skin(&self, player_id: u32, skin: u8) {
        match self {
            Store::Memory(m) => m.equip_skin(player_id, skin).await,
            Store::Mongo(m) => m.equip_skin(player_id, skin).await,
        }
    }

    /// Профиль по нику (для HTTP /api/profile?name=).
    pub async fn profile_by_name(&self, name: &str) -> Option<Profile> {
        let player_id = self.resolve_id_by_name(name).await?;
        self.profile(player_id).await
    }

    /// id игрока по нику.
    async fn resolve_id_by_name(&self, name: &str) -> Option<u32> {
        match self {
            Store::Memory(m) => {
                let inner = m.inner.lock().await;
                inner.names.get(name).copied()
            }
            Store::Mongo(m) => m.player_id_by_name(name).await,
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
    pub elo: i32,
}

/// Клан для списка и меню.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ClanInfo {
    pub tag: String,
    pub name: String,
    pub owner: String,
    pub members: u32,
    pub xp: u64,
}

/// Профиль игрока для HTTP /api/profile.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Profile {
    pub name: String,
    pub xp: u32,
    pub elo: i32,
    pub matches: Vec<MatchRecord>,
    /// Ежедневные квесты и стрик.
    pub quests: Vec<QuestView>,
    pub streak: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct QuestView {
    pub kind: u8,
    pub goal: u32,
    pub prog: u32,
    pub claimed: bool,
    pub reward: u32,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct MatchRecord {
    pub place: u32,
    pub total: u32,
    pub score: u32,
    pub elo_delta: i32,
    pub season: String,
    pub ts: i64,
}

/* ---------- память: фолбэк и юнит-тесты ---------- */

#[derive(Default, Clone)]
pub struct MemoryStore {
    inner: std::sync::Arc<Mutex<MemoryInner>>,
}

/// Состояние квестов дня: (дата, прогресс[3], заявки[3], стрик, последний день с матчем).
type DailyState = (String, [u32; 3], [bool; 3], u32, String);

#[derive(Default)]
struct MemoryInner {
    players: HashMap<u32, PlayerRecord>,
    tokens: HashMap<String, u32>,
    /// Уникальность ников без токена.
    names: HashMap<String, u32>,
    /// Карты редактора.
    maps: Vec<MapInfo>,
    /// История матчей по игрокам.
    matches: Vec<(u32, MatchRecord)>,
    /// Квесты дня: (дата, прогресс[3], заявки[3], стрик, последний день с матчем).
    daily: HashMap<u32, DailyState>,
    /// Кланы по тегу.
    clans: HashMap<String, ClanInfo>,
    /// Тег клана игрока.
    player_clan: HashMap<u32, String>,
    /// XP игроков (для суммарного XP клана).
    xp_by_player: HashMap<u32, u32>,
    next_id: u32,
}

impl MemoryStore {
    async fn publish_map(
        &self,
        id: u32,
        name: &str,
        author: &str,
        rows: Vec<String>,
    ) -> Option<u32> {
        let mut inner = self.inner.lock().await;
        inner.maps.push(MapInfo {
            id,
            name: name.into(),
            author: author.into(),
            rows,
            plays: 0,
        });
        Some(id)
    }

    async fn list_maps(&self, limit: usize) -> Vec<MapInfo> {
        let inner = self.inner.lock().await;
        inner.maps.iter().rev().take(limit).cloned().collect()
    }

    async fn map_plays_inc(&self, map_id: u32) {
        let mut inner = self.inner.lock().await;
        if let Some(m) = inner.maps.iter_mut().find(|m| m.id == map_id) {
            m.plays += 1;
        }
    }

    async fn add_xp(&self, player_id: u32, delta: u32) {
        let mut inner = self.inner.lock().await;
        if let Some(rec) = inner.players.get_mut(&player_id) {
            rec.xp += delta;
        }
    }

    fn quests_locked(inner: &mut MemoryInner, player_id: u32) -> (Vec<QuestView>, u32) {
        let today = crate::lobby::season_now_pub();
        let entry = inner.daily.entry(player_id).or_insert((
            today.clone(),
            [0; 3],
            [false; 3],
            0,
            String::new(),
        ));
        if entry.0 != today {
            *entry = (today.clone(), [0; 3], [false; 3], entry.3, entry.4.clone());
        }
        let goals = Self::quest_goals(player_id, &today);
        let views = goals
            .iter()
            .enumerate()
            .map(|(idx, (kind, goal, reward))| QuestView {
                kind: *kind,
                goal: *goal,
                prog: entry.1[idx].min(*goal),
                claimed: entry.2[idx],
                reward: *reward,
            })
            .collect();
        (views, entry.3)
    }

    /// Цели квестов дня по seed игрока и дате.
    fn quest_goals(seed: u32, date: &str) -> [(u8, u32, u32); 3] {
        let seed32 = seed ^ u32::from_le_bytes(date.as_bytes()[..4].try_into().unwrap_or([0; 4]));
        let h = |n: u32| seed32.wrapping_mul(2654435761).wrapping_add(n) >> 8;
        [
            (0u8, 100 + h(1) % 120, 150), // кирпичи
            (1, 1 + h(2) % 3, 250),       // победы
            (2, 800 + h(3) % 1200, 300),  // очки за день
        ]
    }

    #[allow(dead_code)]
    async fn quests_view(&self, player_id: u32) -> (Vec<QuestView>, u32) {
        let mut inner = self.inner.lock().await;
        Self::quests_locked(&mut inner, player_id)
    }

    async fn quest_bump(&self, player_id: u32, bricks: u32, win: bool, score: u32) {
        let mut inner = self.inner.lock().await;
        let today = crate::lobby::season_now_pub();
        let entry = inner.daily.entry(player_id).or_insert((
            today.clone(),
            [0; 3],
            [false; 3],
            0,
            String::new(),
        ));
        if entry.0 != today {
            *entry = (today.clone(), [0; 3], [false; 3], entry.3, entry.4.clone());
        }
        if entry.4 != today {
            entry.3 += 1; // стрик: любой матч в новый день
            entry.4 = today.clone();
        }
        entry.1[0] = entry.1[0].saturating_add(bricks);
        if win {
            entry.1[1] += 1;
        }
        entry.1[2] = entry.1[2].saturating_add(score);
    }

    async fn quest_claim(&self, player_id: u32, idx: usize) -> Result<u32, String> {
        let mut inner = self.inner.lock().await;
        let today = crate::lobby::season_now_pub();
        let entry = inner.daily.get_mut(&player_id).ok_or("нет квестов")?;
        if entry.0 != today {
            return Err("квесты обновились".into());
        }
        let goals = Self::quest_goals(player_id, &today);
        let (_kind, goal, reward) = goals[idx];
        if entry.2[idx] {
            return Err("уже получено".into());
        }
        if entry.1[idx] < goal {
            return Err("квест не выполнен".into());
        }
        entry.2[idx] = true;
        if let Some(rec) = inner.players.get_mut(&player_id) {
            rec.xp += reward;
        }
        Ok(reward)
    }
    async fn clan_create(
        &self,
        owner_id: u32,
        owner_name: &str,
        tag: &str,
        name: &str,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        if inner.clans.contains_key(tag) {
            return Err("такой тег уже занят".into());
        }
        inner.clans.insert(
            tag.into(),
            ClanInfo {
                tag: tag.into(),
                name: name.into(),
                owner: owner_name.into(),
                members: 1,
                xp: 0,
            },
        );
        inner.player_clan.insert(owner_id, tag.into());
        Ok(())
    }

    async fn clan_join(&self, player_id: u32, tag: &str) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let clan = inner.clans.get_mut(tag).ok_or("клан не найден")?;
        clan.members += 1;
        inner.player_clan.insert(player_id, tag.into());
        Ok(())
    }

    async fn clan_leave(&self, player_id: u32) -> Result<(), String> {
        let mut inner = self.inner.lock().await;
        let Some(tag) = inner.player_clan.remove(&player_id) else {
            return Err("вы без клана".into());
        };
        if let Some(clan) = inner.clans.get_mut(&tag) {
            clan.members = clan.members.saturating_sub(1);
        }
        Ok(())
    }

    #[allow(dead_code)]
    async fn clan_of(&self, player_id: u32) -> Option<ClanInfo> {
        let inner = self.inner.lock().await;
        let tag = inner.player_clan.get(&player_id)?;
        let clan = inner.clans.get(tag)?.clone();
        let xp = inner
            .player_clan
            .iter()
            .filter(|(_, t)| t.as_str() == clan.tag)
            .filter_map(|(pid, _)| inner.xp_by_player.get(pid).map(|x| *x as u64))
            .sum();
        Some(ClanInfo { xp, ..clan })
    }

    #[allow(dead_code)]
    async fn clan_top(&self, limit: usize) -> Vec<ClanInfo> {
        let inner = self.inner.lock().await;
        let mut list: Vec<ClanInfo> = inner
            .clans
            .values()
            .map(|c| {
                let xp = inner
                    .player_clan
                    .iter()
                    .filter(|(_, t)| t.as_str() == c.tag)
                    .filter_map(|(pid, _)| inner.xp_by_player.get(pid).map(|x| *x as u64))
                    .sum();
                ClanInfo { xp, ..c.clone() }
            })
            .collect();
        list.sort_by_key(|c| std::cmp::Reverse(c.xp));
        list.truncate(limit);
        list
    }

    async fn buy_skin(&self, player_id: u32, skin: u8, price: u32) -> Result<bool, String> {
        let mut inner = self.inner.lock().await;
        let rec = inner.players.get_mut(&player_id).ok_or("нет игрока")?;
        if rec.skins_owned.contains(&skin) {
            return Ok(false);
        }
        if rec.xp < price {
            return Err("не хватает XP".into());
        }
        rec.xp -= price;
        rec.skins_owned.push(skin);
        Ok(true)
    }

    async fn equip_skin(&self, player_id: u32, skin: u8) {
        let mut inner = self.inner.lock().await;
        if let Some(rec) = inner.players.get_mut(&player_id) {
            if skin == 0 || rec.skins_owned.contains(&skin) {
                rec.skin = skin;
            }
        }
    }

    async fn match_result(
        &self,
        player_id: u32,
        place: u32,
        total: u32,
        score: u32,
        elo_delta: i32,
        season: &str,
    ) {
        let mut inner = self.inner.lock().await;
        if let Some(rec) = inner.players.get_mut(&player_id) {
            rec.elo += elo_delta;
        }
        inner.matches.push((
            player_id,
            MatchRecord {
                place,
                total,
                score,
                elo_delta,
                season: season.into(),
                ts: SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .ok()
                    .map(|d| d.as_secs() as i64)
                    .unwrap_or(0),
            },
        ));
    }

    async fn elo_top(&self, limit: usize) -> Vec<LeaderEntry> {
        let inner = self.inner.lock().await;
        let mut list: Vec<LeaderEntry> = inner
            .players
            .values()
            .map(|r| LeaderEntry {
                name: r.name.clone(),
                xp: r.xp,
                elo: r.elo,
            })
            .collect();
        list.sort_by_key(|r| std::cmp::Reverse(r.elo));
        list.truncate(limit);
        list
    }

    async fn profile(&self, player_id: u32) -> Option<Profile> {
        let mut inner = self.inner.lock().await;
        let (quests, streak) = Self::quests_locked(&mut inner, player_id);
        let rec = inner.players.get(&player_id)?;
        let mut matches: Vec<MatchRecord> = inner
            .matches
            .iter()
            .filter(|(pid, _)| *pid == player_id)
            .map(|(_, m)| m.clone())
            .collect();
        matches.sort_by_key(|m| std::cmp::Reverse(m.ts));
        matches.truncate(20);
        Some(Profile {
            name: rec.name.clone(),
            xp: rec.xp,
            elo: rec.elo,
            matches,
            quests,
            streak,
        })
    }

    async fn leaderboard(&self, limit: usize) -> Vec<LeaderEntry> {
        let inner = self.inner.lock().await;
        let mut list: Vec<LeaderEntry> = inner
            .players
            .values()
            .map(|r| LeaderEntry {
                name: r.name.clone(),
                xp: r.xp,
                elo: r.elo,
            })
            .collect();
        list.sort_by_key(|r| std::cmp::Reverse(r.xp));
        list.truncate(limit);
        list
    }
}

impl MemoryStore {
    pub fn new() -> Self {
        Self::default()
    }

    async fn resolve_player(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<PlayerRecord, StoreError> {
        let name = validate_name(name)?;
        let mut inner = self.inner.lock().await;

        if let Some(token) = token {
            let Some(&player_id) = inner.tokens.get(token) else {
                return Err(StoreError::BadToken);
            };
            let mut rec = inner
                .players
                .get(&player_id)
                .cloned()
                .expect("token -> player invariant");
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
        let rec = PlayerRecord {
            player_id,
            name: name.clone(),
            token: token.clone(),
            xp: 0,
            elo: 1000,
            skin: 0,
            skins_owned: vec![0],
        };
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
    maps: mongodb::Collection<Document>,
    matches: mongodb::Collection<Document>,
    clans: mongodb::Collection<Document>,
}

impl MongoStore {
    /// Подключение с проверкой живости (ping) и созданием индексов.
    pub async fn connect(url: &str, db_name: &str) -> Result<Self, mongodb::error::Error> {
        let client = Client::with_uri_str(url).await?;
        client
            .database(db_name)
            .run_command(doc! { "ping": 1 })
            .await?;
        let db = client.database(db_name);
        let players = db.collection("players");
        let sessions = db.collection("sessions");
        let maps = db.collection("maps");
        let matches = db.collection("matches");
        let clans = db.collection("clans");

        let name_opts = IndexOptions::builder().unique(true).build();
        players
            .create_index(
                IndexModel::builder()
                    .keys(doc! { "name": 1 })
                    .options(name_opts)
                    .build(),
            )
            .await?;
        sessions
            .create_index(IndexModel::builder().keys(doc! { "token": 1 }).build())
            .await?;

        Ok(Self {
            players,
            sessions,
            maps,
            matches,
            clans,
        })
    }
    fn quest_goals(seed: u32, date: &str) -> [(u8, u32, u32); 3] {
        let seed32 = seed ^ u32::from_le_bytes(date.as_bytes()[..4].try_into().unwrap_or([0; 4]));
        let h = |n: u32| seed32.wrapping_mul(2654435761).wrapping_add(n) >> 8;
        [
            (0u8, 100 + h(1) % 120, 150),
            (1, 1 + h(2) % 3, 250),
            (2, 800 + h(3) % 1200, 300),
        ]
    }

    async fn quests_view(&self, player_id: u32) -> (Vec<QuestView>, u32) {
        let today = crate::lobby::season_now_pub();
        let d = match self
            .players
            .find_one(doc! { "_id": player_id as i64 })
            .await
        {
            Ok(Some(d)) => d,
            _ => return (vec![], 0),
        };
        let daily = d.get_document("daily").ok();
        let _date = daily.and_then(|x| x.get_str("date").ok()).unwrap_or("");
        let prog = daily
            .and_then(|x| x.get_array("prog").ok())
            .map(|a| {
                let mut v = [0u32; 3];
                for (k, item) in a.iter().enumerate().take(3) {
                    v[k] = item.as_i32().unwrap_or(0) as u32;
                }
                v
            })
            .unwrap_or([0; 3]);
        let claimed = daily
            .and_then(|x| x.get_array("claimed").ok())
            .map(|a| {
                let mut v = [false; 3];
                for (k, item) in a.iter().enumerate().take(3) {
                    v[k] = item.as_bool().unwrap_or(false);
                }
                v
            })
            .unwrap_or([false; 3]);
        let streak = daily.and_then(|x| x.get_i32("streak").ok()).unwrap_or(0) as u32;
        let goals = Self::quest_goals(player_id, &today);
        let views = goals
            .iter()
            .enumerate()
            .map(|(idx, (kind, goal, reward))| QuestView {
                kind: *kind,
                goal: *goal,
                prog: prog[idx].min(*goal),
                claimed: claimed[idx],
                reward: *reward,
            })
            .collect();
        (views, streak)
    }

    async fn quest_bump(&self, player_id: u32, bricks: u32, win: bool, score: u32) {
        let today = crate::lobby::season_now_pub();
        let _ = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! {
                    "$inc": {
                        "daily.prog.0": bricks as i32,
                        "daily.prog.2": score as i32,
                    },
                    "$set": { "daily.date": &today },
                    "$max": { "daily.last": &today },
                },
            )
            .await;
        if win {
            let _ = self
                .players
                .update_one(
                    doc! { "_id": player_id as i64 },
                    doc! { "$inc": { "daily.prog.1": 1 } },
                )
                .await;
        }
        // стрик: если дата последнего матча не сегодня — +1
        let _ = self
            .players
            .update_one(
                doc! { "_id": player_id as i64, "daily.last": { "$ne": &today } },
                doc! { "$inc": { "daily.streak": 1 }, "$set": { "daily.last": &today } },
            )
            .await;
    }

    async fn quest_claim(&self, player_id: u32, idx: usize) -> Result<u32, String> {
        let today = crate::lobby::season_now_pub();
        let d = self
            .players
            .find_one(doc! { "_id": player_id as i64 })
            .await
            .map_err(|e| e.to_string())?
            .ok_or("нет игрока")?;
        let daily = d.get_document("daily").ok();
        let date_ok = daily.and_then(|x| x.get_str("date").ok()) == Some(today.as_str());
        if !date_ok {
            return Err("квесты обновились".into());
        }
        let claimed = daily
            .and_then(|x| x.get_array("claimed").ok())
            .map(|a| {
                a.iter()
                    .enumerate()
                    .all(|(k, v)| k == idx || v.as_bool().unwrap_or(false))
            })
            .unwrap_or(true);
        let prog = daily
            .and_then(|x| x.get_array("prog").ok())
            .and_then(|a| a.get(idx).and_then(|v| v.as_i32()))
            .unwrap_or(0) as u32;
        let goals = Self::quest_goals(player_id, &today);
        let (_kind, goal, reward) = goals[idx];
        if claimed
            && daily
                .and_then(|x| x.get_array("claimed").ok())
                .and_then(|a| a.get(idx))
                .and_then(|v| v.as_bool())
                .unwrap_or(false)
        {
            return Err("уже получено".into());
        }
        if prog < goal {
            return Err("квест не выполнен".into());
        }
        let field = format!("daily.claimed.{idx}");
        let res = self
            .players
            .update_one(
                doc! { "_id": player_id as i64, &field: false },
                doc! { "$set": { &field: true }, "$inc": { "xp": reward as i32 } },
            )
            .await
            .map_err(|e| e.to_string())?;
        if res.modified_count == 0 {
            return Err("уже получено".into());
        }
        Ok(reward)
    }
    async fn clan_create(
        &self,
        owner_id: u32,
        owner_name: &str,
        tag: &str,
        name: &str,
    ) -> Result<(), String> {
        let exists = self
            .clans
            .find_one(doc! { "_id": tag })
            .await
            .map_err(|e| e.to_string())?;
        if exists.is_some() {
            return Err("такой тег уже занят".into());
        }
        self.clans
            .insert_one(doc! {
                "_id": tag,
                "name": name,
                "owner": owner_name,
                "members": [owner_id as i64],
            })
            .await
            .map_err(|e| e.to_string())?;
        self.set_player_clan(owner_id, tag).await;
        Ok(())
    }

    async fn clan_join(&self, player_id: u32, tag: &str) -> Result<(), String> {
        let res = self
            .clans
            .update_one(
                doc! { "_id": tag },
                doc! { "$addToSet": { "members": player_id as i64 } },
            )
            .await
            .map_err(|e| e.to_string())?;
        if res.matched_count == 0 {
            return Err("клан не найден".into());
        }
        self.set_player_clan(player_id, tag).await;
        Ok(())
    }

    async fn clan_leave(&self, player_id: u32) -> Result<(), String> {
        let tag = self
            .players
            .find_one(doc! { "_id": player_id as i64 })
            .await
            .map_err(|e| e.to_string())?
            .and_then(|d| d.get_str("clan").ok().map(|s| s.to_string()));
        let Some(tag) = tag else {
            return Err("вы без клана".into());
        };
        let _ = self
            .clans
            .update_one(
                doc! { "_id": &tag },
                doc! { "$pull": { "members": player_id as i64 } },
            )
            .await;
        let _ = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! { "$unset": { "clan": "" } },
            )
            .await;
        Ok(())
    }

    async fn set_player_clan(&self, player_id: u32, tag: &str) {
        let _ = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! { "$set": { "clan": tag } },
            )
            .await;
    }

    fn clan_from_doc(d: &Document, xp: u64) -> ClanInfo {
        ClanInfo {
            tag: d.get_str("_id").unwrap_or("?").into(),
            name: d.get_str("name").unwrap_or("?").into(),
            owner: d.get_str("owner").unwrap_or("?").into(),
            members: d.get_array("members").map(|a| a.len() as u32).unwrap_or(0),
            xp,
        }
    }

    #[allow(dead_code)]
    async fn clan_of(&self, player_id: u32) -> Option<ClanInfo> {
        let d = self
            .players
            .find_one(doc! { "_id": player_id as i64 })
            .await
            .ok()??;
        let tag = d.get_str("clan").ok()?;
        let c = self.clans.find_one(doc! { "_id": tag }).await.ok()??;
        // суммарный XP: агрегируем по members
        let members = c.get_array("members").ok()?;
        let mut xp = 0u64;
        for m in members {
            if let Some(pid) = m.as_i64() {
                if let Some(pd) = self
                    .players
                    .find_one(doc! { "_id": pid })
                    .await
                    .ok()
                    .flatten()
                {
                    xp += get_u32(&pd, "xp").unwrap_or(0) as u64;
                }
            }
        }
        Some(Self::clan_from_doc(&c, xp))
    }

    async fn clan_top(&self, limit: usize) -> Vec<ClanInfo> {
        let mut out = vec![];
        let Ok(mut cur) = self.clans.find(mongodb::bson::Document::new()).await else {
            return out;
        };
        let mut raw = vec![];
        while let Some(Ok(d)) = cur.next().await {
            raw.push(d);
        }
        // XP по кланам: один проход по игрокам
        let mut xp_by_tag: std::collections::HashMap<String, u64> =
            std::collections::HashMap::new();
        let Ok(mut pc) = self.players.find(mongodb::bson::Document::new()).await else {
            return out;
        };
        while let Some(Ok(pd)) = pc.next().await {
            if let Ok(tag) = pd.get_str("clan") {
                *xp_by_tag.entry(tag.into()).or_insert(0) += get_u32(&pd, "xp").unwrap_or(0) as u64;
            }
        }
        for c in raw {
            let tag = c.get_str("_id").unwrap_or("?").to_string();
            let xp = xp_by_tag.get(&tag).copied().unwrap_or(0);
            out.push(Self::clan_from_doc(&c, xp));
        }
        out.sort_by_key(|c| std::cmp::Reverse(c.xp));
        out.truncate(limit);
        out
    }

    async fn buy_skin(&self, player_id: u32, skin: u8, price: u32) -> Result<bool, String> {
        let owned = self
            .players
            .find_one(doc! { "_id": player_id as i64, "skins_owned": skin as i32 })
            .await
            .map_err(|e| e.to_string())?;
        if owned.is_some() {
            return Ok(false);
        }
        let res = self
            .players
            .update_one(
                doc! { "_id": player_id as i64, "xp": { "$gte": price as i32 } },
                doc! { "$inc": { "xp": -(price as i32) }, "$addToSet": { "skins_owned": skin as i32 } },
            )
            .await
            .map_err(|e| e.to_string())?;
        if res.modified_count == 0 {
            return Err("не хватает XP".into());
        }
        Ok(true)
    }

    async fn equip_skin(&self, player_id: u32, skin: u8) {
        let _ = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! { "$set": { "skin": skin as i32 } },
            )
            .await;
    }

    async fn match_result(
        &self,
        player_id: u32,
        place: u32,
        total: u32,
        score: u32,
        elo_delta: i32,
        season: &str,
    ) {
        if let Err(e) = self
            .players
            .update_one(
                doc! { "_id": player_id as i64 },
                doc! { "$inc": { "elo": elo_delta as i64 } },
            )
            .await
        {
            tracing::warn!("elo update: {e}");
        }
        let _ = self
            .matches
            .insert_one(doc! {
                "player_id": player_id as i64,
                "place": place as i32,
                "total": total as i32,
                "score": score as i32,
                "elo_delta": elo_delta,
                "season": season,
                "ts": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64).unwrap_or(0),
            })
            .await;
    }

    async fn elo_top(&self, limit: usize) -> Vec<LeaderEntry> {
        let mut out = vec![];
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "elo": -1 })
            .limit(limit as i64)
            .build();
        let Ok(mut cur) = self
            .players
            .find(mongodb::bson::Document::new())
            .with_options(opts)
            .await
        else {
            return out;
        };
        while let Some(Ok(d)) = cur.next().await {
            out.push(LeaderEntry {
                name: d.get_str("name").unwrap_or("?").into(),
                xp: get_u32(&d, "xp").unwrap_or(0),
                elo: get_i32(&d, "elo").unwrap_or(1000),
            });
        }
        out
    }

    async fn profile(&self, player_id: u32) -> Option<Profile> {
        let d = self
            .players
            .find_one(doc! { "_id": player_id as i64 })
            .await
            .ok()??;
        let mut matches = vec![];
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "ts": -1 })
            .limit(20)
            .build();
        if let Ok(mut cur) = self
            .matches
            .find(doc! { "player_id": player_id as i64 })
            .with_options(opts)
            .await
        {
            while let Some(Ok(m)) = cur.next().await {
                matches.push(MatchRecord {
                    place: get_u32(&m, "place").unwrap_or(0),
                    total: get_u32(&m, "total").unwrap_or(0),
                    score: get_u32(&m, "score").unwrap_or(0),
                    elo_delta: get_i32(&m, "elo_delta").unwrap_or(0),
                    season: m.get_str("season").unwrap_or("?").into(),
                    ts: m.get_i64("ts").unwrap_or(0),
                });
            }
        }
        let (quests, streak) = self.quests_view(player_id).await;
        Some(Profile {
            name: d.get_str("name").unwrap_or("?").into(),
            xp: get_u32(&d, "xp").unwrap_or(0),
            elo: get_i32(&d, "elo").unwrap_or(1000),
            matches,
            quests,
            streak,
        })
    }

    async fn player_id_by_name(&self, name: &str) -> Option<u32> {
        let d = self.players.find_one(doc! { "name": name }).await.ok()??;
        Some(get_u32(&d, "_id").unwrap_or(0))
    }

    async fn publish_map(
        &self,
        id: u32,
        name: &str,
        author: &str,
        rows: Vec<String>,
    ) -> Option<u32> {
        let doc = doc! {
            "_id": id as i64,
            "name": name,
            "author": author,
            "rows": rows,
            "plays": 0i32,
            "created": SystemTime::now().duration_since(UNIX_EPOCH).ok().map(|d| d.as_secs() as i64).unwrap_or(0),
        };
        self.maps.insert_one(doc).await.ok()?;
        Some(id)
    }

    async fn list_maps(&self, limit: usize) -> Vec<MapInfo> {
        let mut out = vec![];
        let filter = mongodb::bson::Document::new();
        let opts = mongodb::options::FindOptions::builder()
            .sort(doc! { "created": -1 })
            .limit(limit as i64)
            .build();
        let mut cur = match self.maps.find(filter).with_options(opts).await {
            Ok(c) => c,
            Err(_) => return out,
        };
        while let Some(Ok(d)) = cur.next().await {
            let empty = Vec::new();
            let rows: Vec<String> = d
                .get_array("rows")
                .unwrap_or(&empty)
                .iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect();
            out.push(MapInfo {
                id: d.get_i64("_id").unwrap_or_default() as u32,
                name: d.get_str("name").unwrap_or("?").into(),
                author: d.get_str("author").unwrap_or("?").into(),
                rows,
                plays: d.get_i32("plays").unwrap_or_default() as u32,
            });
        }
        out
    }

    #[allow(dead_code)]
    async fn map_plays_inc(&self, map_id: u32) {
        let _ = self
            .maps
            .update_one(
                doc! { "_id": map_id as i64 },
                doc! { "$inc": { "plays": 1 } },
            )
            .await;
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

    async fn resolve_player(
        &self,
        name: &str,
        token: Option<&str>,
    ) -> Result<PlayerRecord, StoreError> {
        let name = validate_name(name)?;

        if let Some(token) = token {
            let session = self
                .sessions
                .find_one(doc! { "token": token })
                .await
                .map_err(|e| StoreError::Backend(e.to_string()))?
                .ok_or(StoreError::BadToken)?;
            let player_id = get_u32(&session, "player_id")
                .ok_or_else(|| StoreError::Backend("session.player_id: bad type".into()))?;
            let player = self
                .player_doc(player_id)
                .await?
                .ok_or(StoreError::BadToken)?;
            let current_name: String = player
                .get_str("name")
                .map_err(|e| StoreError::Backend(e.to_string()))?
                .into();
            if current_name != name {
                // Смена ника: имя должно быть свободно.
                let taken = self.player_doc_named(&name).await?.is_some();
                if taken {
                    return Err(StoreError::NameTaken);
                }
                self.players
                    .update_one(
                        doc! { "_id": player_id },
                        doc! { "$set": { "name": &name } },
                    )
                    .await
                    .map_err(|e| StoreError::Backend(e.to_string()))?;
            }
            let xp = get_u32(&player, "xp").unwrap_or(0);
            return Ok(PlayerRecord {
                player_id,
                name,
                token: token.to_string(),
                xp,
                elo: get_i32(&player, "elo").unwrap_or(1000),
                skin: get_u32(&player, "skin").unwrap_or(0) as u8,
                skins_owned: player
                    .get_array("skins_owned")
                    .map(|a| {
                        a.iter()
                            .filter_map(|v| v.as_i32().map(|x| x as u8))
                            .collect()
                    })
                    .unwrap_or_else(|_| vec![0]),
            });
        }

        // Новый гость: ник должен быть свободен.
        if self.player_doc_named(&name).await?.is_some() {
            return Err(StoreError::NameTaken);
        }
        let player_id = self.random_player_id().await?;
        let token = new_token();
        let rec = PlayerRecord {
            player_id,
            name,
            token,
            xp: 0,
            elo: 1000,
            skin: 0,
            skins_owned: vec![0],
        };
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
                elo: get_i32(d, "elo").unwrap_or(1000),
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
    match tokio::time::timeout(
        std::time::Duration::from_secs(4),
        MongoStore::connect(&url, "pop_protocol"),
    )
    .await
    {
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
    #[tokio::test]
    async fn match_result_updates_elo_and_history() {
        let store = Store::Memory(MemoryStore::new());
        let rec = store.resolve_player("Иванс", None).await.unwrap();
        store
            .match_result(rec.player_id, 1, 2, 500, 32, "2026-11")
            .await;
        store
            .match_result(rec.player_id, 2, 2, 100, -32, "2026-11")
            .await;
        let prof = store.profile(rec.player_id).await.unwrap();
        assert_eq!(prof.elo, 1000);
        assert_eq!(prof.matches.len(), 2);
        let top = store.elo_top(10).await;
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].elo, 1000);
    }

    #[tokio::test]
    async fn skins_buy_equip_flow() {
        let store = Store::Memory(MemoryStore::new());
        let rec = store.resolve_player("СкинТест", None).await.unwrap();
        // XP не хватает
        assert!(store.buy_skin(rec.player_id, 1, 500).await.is_err());
        // даём XP и покупаем
        store.add_xp(rec.player_id, 600).await;
        assert!(store.buy_skin(rec.player_id, 1, 500).await.unwrap());
        // повторная покупка — false
        assert!(!store.buy_skin(rec.player_id, 1, 500).await.unwrap());
        store.equip_skin(rec.player_id, 1).await;
        let prof = store.profile(rec.player_id).await.unwrap();
        assert_eq!(prof.xp, 100); // 600 - 500
        store.equip_skin(rec.player_id, 1).await;
    }

    #[test]
    fn season_format() {
        let s = crate::lobby::season_now_pub();
        assert_eq!(s.len(), 7);
        assert_eq!(s.as_bytes()[4], b'-');
    }

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
        let again = store
            .resolve_player("Тима", Some(&first.token))
            .await
            .unwrap();
        assert_eq!(first.player_id, again.player_id);
    }

    #[tokio::test]
    async fn memory_name_taken_without_token() {
        let store = MemoryStore::new();
        store.resolve_player("Тима", None).await.unwrap();
        assert_eq!(
            store.resolve_player("Тима", None).await,
            Err(StoreError::NameTaken)
        );
    }

    #[tokio::test]
    async fn memory_rename_with_token() {
        let store = MemoryStore::new();
        let first = store.resolve_player("Тима", None).await.unwrap();
        let renamed = store
            .resolve_player("Пума", Some(&first.token))
            .await
            .unwrap();
        assert_eq!(renamed.player_id, first.player_id);
        assert_eq!(renamed.name, "Пума");
        // Старое имя освободилось.
        store.resolve_player("Тима", None).await.unwrap();
    }

    #[tokio::test]
    async fn memory_invalid_names() {
        let store = MemoryStore::new();
        assert_eq!(
            store.resolve_player("   ", None).await,
            Err(StoreError::InvalidName)
        );
        assert_eq!(
            store.resolve_player("", None).await,
            Err(StoreError::InvalidName)
        );
        let long = "ш".repeat(17);
        assert_eq!(
            store.resolve_player(&long, None).await,
            Err(StoreError::InvalidName)
        );
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
        assert_eq!(
            store.resolve_player(&name, None).await,
            Err(StoreError::NameTaken)
        );
    }
}
