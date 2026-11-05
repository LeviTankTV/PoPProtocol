//! Лобби и комнаты PoP - Protocol (шаги 5–6).
//!
//! Чистая игровая логика без WS: каждый мутирующий метод сам рассылает
//! `ServerMsg` в канал комнаты. Тестируется без сети через `broadcast::Receiver`.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};

use popproto_core::{
    BotLevel, ErrCode, GameMode, PlayerInfo, RoomPhase, RoomSettings, RoomState, RoomSummary,
    ServerMsg, BOT_ID_BASE, MAX_PLAYERS, MAX_SPECTATORS, RECONNECT_WINDOW_SECS,
};
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::game::{self, GameCmd};
use crate::store::{PlayerRecord, Store};

pub struct Room {
    pub id: u32,
    /// 6-значный код входа.
    pub code: String,
    pub name: String,
    pub password: Option<String>,
    pub mode: GameMode,
    pub settings: RoomSettings,
    pub host_id: u32,
    pub phase: RoomPhase,
    pub players: Vec<PlayerInfo>,
    pub spectators: Vec<PlayerInfo>,
    pub tx: broadcast::Sender<ServerMsg>,
    /// Игроки с оборванным соединением: id -> дедлайн возврата (90 сек).
    pub reconnects: HashMap<u32, Instant>,
    next_bot_id: u32,
}

impl Room {
    pub fn state(&self) -> RoomState {
        RoomState {
            room_id: self.id,
            code: self.code.clone(),
            name: self.name.clone(),
            mode: self.mode,
            settings: self.settings.clone(),
            host_id: self.host_id,
            phase: self.phase,
            players: self.players.clone(),
            spectators: self.spectators.clone(),
        }
    }

    pub fn summary(&self) -> RoomSummary {
        RoomSummary {
            room_id: self.id,
            code: self.code.clone(),
            name: self.name.clone(),
            mode: self.mode,
            players: self.players.len() as u8,
            max_players: self.settings.max_players,
            has_password: self.password.is_some(),
            in_progress: self.phase == RoomPhase::Playing,
        }
    }

    fn broadcast(&self, msg: ServerMsg) {
        // Нет слушателей — не ошибка: комната с ботами/без зрителей.
        let _ = self.tx.send(msg);
    }

    fn broadcast_state(&self) {
        self.broadcast(ServerMsg::RoomState(Box::new(self.state())));
    }

    #[allow(dead_code)] // зарезервировано: профиль игрока в REST
    fn info_of(&self, player_id: u32) -> Option<PlayerInfo> {
        self.players
            .iter()
            .chain(&self.spectators)
            .find(|p| p.id == player_id)
            .cloned()
    }

    fn remove_member(&mut self, player_id: u32) -> bool {
        let before = self.players.len() + self.spectators.len();
        self.players.retain(|p| p.id != player_id);
        self.spectators.retain(|p| p.id != player_id);
        self.players.len() + self.spectators.len() < before
    }
}

fn info_from_record(rec: &PlayerRecord, color: u8) -> PlayerInfo {
    PlayerInfo {
        id: rec.player_id,
        name: rec.name.clone(),
        ready: false,
        is_bot: false,
        bot_level: None,
        score: 0,
        lives: 3,
        color,
        xp: rec.xp,
        online: true,
    }
}

fn gen_code() -> String {
    // 6 цифр, первая ненулевая; коды короткоживущие, коллизии проверяет лобби.
    let n: u32 = (uuid::Uuid::new_v4().as_u128() % 900_000) as u32 + 100_000;
    n.to_string()
}

#[derive(Debug, PartialEq)]
pub enum JoinKind {
    Player,
    Spectator,
}

#[derive(Debug, PartialEq)]
pub struct Joined {
    pub room_id: u32,
    pub kind: JoinKind,
}

#[derive(Debug, PartialEq)]
#[allow(dead_code)] // вариант ошибок бэкенда хранилища
pub enum LobbyError {
    Code(ErrCode),
    Backend(String),
}

impl From<ErrCode> for LobbyError {
    fn from(code: ErrCode) -> Self {
        LobbyError::Code(code)
    }
}

#[derive(Default)]
struct Inner {
    rooms: HashMap<u32, Room>,
    /// Активные матчи: команда-канал игрового цикла по room_id.
    games: HashMap<u32, mpsc::UnboundedSender<GameCmd>>,
    next_room_id: u32,
}

#[derive(Clone)]
pub struct Lobby {
    inner: Arc<Mutex<Inner>>,
    /// Хранилище для начисления XP по итогам матчей.
    store: Arc<Store>,
}

impl Lobby {
    pub fn new(store: Arc<Store>) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner::default())),
            store,
        }
    }

    /// Подписка на события комнаты (RoomState, PlayerJoined, ...).
    pub async fn subscribe(&self, room_id: u32) -> Option<broadcast::Receiver<ServerMsg>> {
        self.inner
            .lock()
            .await
            .rooms
            .get(&room_id)
            .map(|r| r.tx.subscribe())
    }

    #[allow(dead_code)] // зарезервировано: REST-проверки
    pub async fn room_exists(&self, room_id: u32) -> bool {
        self.inner.lock().await.rooms.contains_key(&room_id)
    }

    pub async fn room_list(&self) -> Vec<RoomSummary> {
        let inner = self.inner.lock().await;
        let mut list: Vec<RoomSummary> = inner.rooms.values().map(|r| r.summary()).collect();
        list.sort_by_key(|s| s.room_id);
        list
    }

    pub async fn create_room(
        &self,
        creator: &PlayerRecord,
        name: String,
        password: Option<String>,
        mode: GameMode,
        mut settings: RoomSettings,
    ) -> Result<u32, LobbyError> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 32 {
            return Err(ErrCode::InvalidInput.into());
        }
        settings.max_players = settings.max_players.clamp(1, MAX_PLAYERS as u8);

        let mut inner = self.inner.lock().await;
        let id = inner.next_room_id;
        inner.next_room_id += 1;

        let mut code = gen_code();
        while inner.rooms.values().any(|r| r.code == code) {
            code = gen_code();
        }

        let (tx, _rx) = broadcast::channel(128);
        let host = info_from_record(creator, 0);
        let room = Room {
            id,
            code,
            name: name.to_string(),
            password: password.filter(|p| !p.is_empty()),
            mode,
            settings,
            host_id: creator.player_id,
            phase: RoomPhase::Lobby,
            players: vec![host],
            spectators: vec![],
            tx,
            reconnects: HashMap::new(),
            next_bot_id: 0,
        };
        inner.rooms.insert(id, room);
        tracing::info!("создана комната #{id} «{name}» ({mode:?})");
        Ok(id)
    }

    /// Текущее состояние комнаты; None если комнаты нет.
    pub async fn room_state(&self, room_id: u32) -> Option<RoomState> {
        self.inner
            .lock()
            .await
            .rooms
            .get(&room_id)
            .map(|r| r.state())
    }

    /// Отправить сообщение в канал комнаты без изменения состояния (чат, эмоции).
    pub async fn broadcast_raw(&self, room_id: u32, msg: ServerMsg) -> Result<(), LobbyError> {
        let inner = self.inner.lock().await;
        let room = inner.rooms.get(&room_id).ok_or(ErrCode::RoomNotFound)?;
        room.broadcast(msg);
        Ok(())
    }

    /// Передать команду матчу (PaddleX/Action). false — матча нет.
    pub async fn game_cmd(&self, room_id: u32, cmd: GameCmd) -> bool {
        let inner = self.inner.lock().await;
        inner
            .games
            .get(&room_id)
            .map(|tx| tx.send(cmd).is_ok())
            .unwrap_or(false)
    }

    /// Обрыв соединения: игрок остаётся в слоте на окно реконнекта.
    pub async fn player_disconnected(&self, player_id: u32, room_id: u32) -> bool {
        let mut inner = self.inner.lock().await;
        let Some(room) = inner.rooms.get_mut(&room_id) else {
            return false;
        };
        let seat = room
            .players
            .iter_mut()
            .find(|p| p.id == player_id)
            .or_else(|| room.spectators.iter_mut().find(|p| p.id == player_id));
        match seat {
            Some(seat) if seat.online => {
                seat.online = false;
                room.reconnects.insert(
                    player_id,
                    Instant::now() + Duration::from_secs(RECONNECT_WINDOW_SECS),
                );
                room.broadcast_state();
                tracing::info!(
                    "игрок {player_id} отвалился, окно реконнекта {} с",
                    RECONNECT_WINDOW_SECS
                );
                true
            }
            _ => true,
        }
    }

    /// Убрать игроков, не вернувшихся за окно реконнекта. Вызывается фоновым свипером.
    pub async fn sweep_reconnects(&self) {
        let expired: Vec<(u32, u32)> = {
            let inner = self.inner.lock().await;
            inner
                .rooms
                .iter()
                .flat_map(|(id, room)| {
                    room.reconnects
                        .iter()
                        .filter(|(_, deadline)| **deadline <= Instant::now())
                        .map(move |(pid, _)| (*id, *pid))
                })
                .collect()
        };
        for (room_id, player_id) in expired {
            self.leave_room(player_id, room_id).await;
        }
    }

    /// Фоновая уборка окон реконнекта.
    pub fn spawn_sweeper(lobby: Lobby) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(3));
            loop {
                interval.tick().await;
                lobby.sweep_reconnects().await;
            }
        });
    }

    /// Хост добавляет бота в свободный слот (только в фазе лобби).
    pub async fn add_bot(
        &self,
        host_id: u32,
        room_id: u32,
        level: BotLevel,
    ) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner.rooms.get_mut(&room_id).ok_or(ErrCode::RoomNotFound)?;
        if room.host_id != host_id {
            return Err(ErrCode::NotHost.into());
        }
        if room.phase != RoomPhase::Lobby {
            return Err(ErrCode::NotAllowed.into());
        }
        if room.players.len() >= room.settings.max_players as usize {
            return Err(ErrCode::RoomFull.into());
        }
        let id = BOT_ID_BASE + room.next_bot_id;
        room.next_bot_id += 1;
        let name = match level {
            BotLevel::Lazy => "Бот·Лентяй",
            BotLevel::Normal => "Бот",
            BotLevel::Pro => "Бот·Профи",
        };
        let color_slot = room.players.len() as u8;
        let info = PlayerInfo {
            id,
            name: name.into(),
            ready: true,
            is_bot: true,
            bot_level: Some(level),
            score: 0,
            lives: 3,
            color: color_slot,
            xp: 0,
            online: true,
        };
        room.broadcast(ServerMsg::PlayerJoined(Box::new(info.clone())));
        room.players.push(info);
        room.broadcast_state();
        Ok(())
    }

    /// Хост убирает бота.
    pub async fn remove_bot(
        &self,
        host_id: u32,
        room_id: u32,
        target: u32,
    ) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner.rooms.get_mut(&room_id).ok_or(ErrCode::RoomNotFound)?;
        if room.host_id != host_id {
            return Err(ErrCode::NotHost.into());
        }
        if room.phase != RoomPhase::Lobby {
            return Err(ErrCode::NotAllowed.into());
        }
        let is_bot = room.players.iter().any(|p| p.id == target && p.is_bot);
        if target == host_id || !is_bot || !room.remove_member(target) {
            return Err(ErrCode::InvalidInput.into());
        }
        room.broadcast(ServerMsg::PlayerLeft { player_id: target });
        room.broadcast_state();
        Ok(())
    }

    /// Матч завершён: снять фазу Playing, сбросить готовность, начислить XP.
    pub async fn finish_match(&self, room_id: u32, scores: Vec<(u32, u32)>) {
        {
            let mut inner = self.inner.lock().await;
            inner.games.remove(&room_id);
            let Some(room) = inner.rooms.get_mut(&room_id) else {
                return;
            };
            room.phase = RoomPhase::Lobby;
            for p in room.players.iter_mut() {
                p.ready = false;
                // XP в комнате обновится у вернувшихся при следующем матче
                if let Some((_, score)) = scores.iter().find(|(pid, _)| *pid == p.id) {
                    p.xp += score / 10;
                }
            }
            tracing::info!("комната #{room_id}: матч завершён");
            room.broadcast_state();
        }
        // XP в хранилище: очки/10, минимум 1 за матч с очками
        for (player_id, score) in &scores {
            let delta = if *score >= 10 { score / 10 } else { 1 };
            self.store.add_xp(*player_id, delta).await;
        }
    }

    pub async fn join_room(
        &self,
        player: &PlayerRecord,
        code: &str,
        password: Option<&str>,
    ) -> Result<Joined, LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner
            .rooms
            .values_mut()
            .find(|r| r.code == code)
            .ok_or(ErrCode::RoomNotFound)?;

        // Реконнект: свой слот с оборванным соединением возвращается без пароля.
        let seat = room
            .players
            .iter_mut()
            .find(|p| p.id == player.player_id)
            .or_else(|| {
                room.spectators
                    .iter_mut()
                    .find(|p| p.id == player.player_id)
            });
        if let Some(seat) = seat {
            if seat.online {
                return Err(ErrCode::NotAllowed.into()); // уже в комнате со живым соединением
            }
            seat.online = true;
            room.reconnects.remove(&player.player_id);
            let room_id = room.id;
            room.broadcast_state();
            tracing::info!("игрок {} вернулся в комнату #{room_id}", player.name);
            let kind = if room.players.iter().any(|p| p.id == player.player_id) {
                JoinKind::Player
            } else {
                JoinKind::Spectator
            };
            return Ok(Joined { room_id, kind });
        }

        match (&room.password, password) {
            (Some(expected), Some(got)) if expected == got => {}
            (None, _) => {}
            _ => return Err(ErrCode::WrongPassword.into()),
        }
        if room.players.iter().any(|p| p.id == player.player_id) {
            return Err(ErrCode::NotAllowed.into()); // уже игрок
        }
        // Зритель просится в игроки: переводим, если есть свободный слот.
        if let Some(spec) = room.spectators.iter().find(|p| p.id == player.player_id) {
            if room.players.len() >= room.settings.max_players as usize
                || room.phase == RoomPhase::Playing
            {
                return Err(ErrCode::RoomFull.into()); // места нет — остаёшься зрителем
            }
            let mut info = spec.clone();
            info.color = (0..8u8)
                .find(|c| !room.players.iter().any(|p| p.color == *c))
                .unwrap_or(room.players.len() as u8);
            info.ready = false;
            room.spectators.retain(|p| p.id != player.player_id);
            room.players.push(info);
            let room_id = room.id;
            tracing::info!("зритель {} стал игроком в комнате #{room_id}", player.name);
            room.broadcast_state();
            return Ok(Joined {
                room_id,
                kind: JoinKind::Player,
            });
        }

        let kind = if room.phase == RoomPhase::Playing
            || room.players.len() >= room.settings.max_players as usize
        {
            JoinKind::Spectator
        } else {
            JoinKind::Player
        };
        if kind == JoinKind::Spectator && room.spectators.len() >= MAX_SPECTATORS {
            return Err(ErrCode::RoomFull.into());
        }

        let color_slot = room.players.len() as u8;
        let info = info_from_record(player, color_slot);
        let joined_msg = ServerMsg::PlayerJoined(Box::new(info.clone()));
        match kind {
            JoinKind::Player => room.players.push(info),
            JoinKind::Spectator => room.spectators.push(info),
        }
        let room_id = room.id;
        room.broadcast(joined_msg);
        room.broadcast_state();
        Ok(Joined { room_id, kind })
    }

    /// Смена цвета игроком: цвет должен быть свободен среди участников комнаты.
    pub async fn set_color(
        &self,
        player_id: u32,
        room_id: u32,
        color: u8,
    ) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let Some(room) = inner.rooms.get_mut(&room_id) else {
            return Err(LobbyError::Code(ErrCode::NotAllowed));
        };
        let taken = room
            .players
            .iter()
            .any(|p| p.id != player_id && p.color == color)
            || room
                .spectators
                .iter()
                .any(|p| p.id != player_id && p.color == color);
        if taken {
            return Err(LobbyError::Code(ErrCode::NotAllowed));
        }
        let changed = room
            .players
            .iter_mut()
            .find(|p| p.id == player_id)
            .map(|p| {
                p.color = color;
                true
            })
            .or_else(|| {
                room.spectators
                    .iter_mut()
                    .find(|p| p.id == player_id)
                    .map(|p| {
                        p.color = color;
                        true
                    })
            });
        if changed.is_none() {
            return Err(LobbyError::Code(ErrCode::NotAllowed));
        }
        tracing::info!("комната #{room_id}: игрок {player_id} сменил цвет на {color}");
        room.broadcast_state();
        Ok(())
    }

    pub async fn leave_room(&self, player_id: u32, room_id: u32) -> bool {
        let mut inner = self.inner.lock().await;
        // Матч узнаёт об уходе игрока (уберёт ракетку и мячи).
        if let Some(tx) = inner.games.get(&room_id) {
            let _ = tx.send(GameCmd::PlayerLeft { player_id });
        }
        let Some(room) = inner.rooms.get_mut(&room_id) else {
            return false;
        };
        if !room.remove_member(player_id) {
            return true; // не был участником, но комната жива
        }
        room.broadcast(ServerMsg::PlayerLeft { player_id });
        if room.players.is_empty() {
            // Игроки кончились — комната распускается (зрители не держат её).
            inner.rooms.remove(&room_id);
            tracing::info!("комната #{room_id} распущена");
            return false;
        }
        if room.host_id == player_id {
            room.host_id = room.players[0].id;
            room.broadcast(ServerMsg::HostChanged {
                player_id: room.host_id,
            });
        }
        room.broadcast_state();
        true
    }

    pub async fn set_ready(
        &self,
        player_id: u32,
        room_id: u32,
        ready: bool,
    ) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner.rooms.get_mut(&room_id).ok_or(ErrCode::RoomNotFound)?;
        let Some(seat) = room.players.iter_mut().find(|p| p.id == player_id) else {
            return Err(ErrCode::NotAllowed.into());
        };
        seat.ready = ready;
        room.broadcast_state();
        Ok(())
    }

    pub async fn start_match(&self, player_id: u32, room_id: u32) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner.rooms.get_mut(&room_id).ok_or(ErrCode::RoomNotFound)?;
        if room.host_id != player_id {
            return Err(ErrCode::NotHost.into());
        }
        if room.phase != RoomPhase::Lobby {
            return Err(ErrCode::NotAllowed.into());
        }
        // Люди должны быть готовы; боты готовы всегда.
        if !room.players.iter().all(|p| p.is_bot || p.ready) {
            return Err(ErrCode::NotAllowed.into());
        }
        room.phase = RoomPhase::Playing;
        room.broadcast(ServerMsg::MatchStarted);
        room.broadcast_state();
        tracing::info!(
            "комната #{}: матч начал ({} игроков)",
            room.id,
            room.players.len()
        );

        // Запуск игрового цикла: поле, мячи, снапшоты 30 Гц.
        let players = room
            .players
            .iter()
            .map(|p| (p.id, p.name.clone(), p.bot_level))
            .collect();
        let speed = room.settings.speed_factor;
        let events_on = room.settings.events_enabled;
        let mode = room.mode;
        let cmd_tx = game::spawn(self.clone(), room_id, players, speed, events_on, mode);
        inner.games.insert(room_id, cmd_tx);
        Ok(())
    }

    pub async fn kick(&self, host_id: u32, room_id: u32, target: u32) -> Result<(), LobbyError> {
        let mut inner = self.inner.lock().await;
        let room = inner.rooms.get_mut(&room_id).ok_or(ErrCode::RoomNotFound)?;
        if room.host_id != host_id {
            return Err(ErrCode::NotHost.into());
        }
        if target == host_id || !room.remove_member(target) {
            return Err(ErrCode::InvalidInput.into());
        }
        // Цели кика отправляется персональное сообщение ws-слоем (player removed).
        room.broadcast(ServerMsg::PlayerLeft { player_id: target });
        room.broadcast_state();
        Ok(())
    }

    /// Живёт ли игрок в указанной комнате (для очистки при разрыве соединения).
    #[allow(dead_code)] // зарезервировано: REST-эндпоинты комнаты
    pub async fn member_room(&self, player_id: u32) -> Option<u32> {
        let inner = self.inner.lock().await;
        inner
            .rooms
            .values()
            .find(|r| {
                r.players
                    .iter()
                    .chain(&r.spectators)
                    .any(|p| p.id == player_id)
            })
            .map(|r| r.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_lobby() -> Lobby {
        use crate::store::{MemoryStore, Store};
        Lobby::new(Arc::new(Store::Memory(MemoryStore::new())))
    }

    fn rec(id: u32, name: &str) -> PlayerRecord {
        PlayerRecord {
            player_id: id,
            name: name.into(),
            token: format!("tok{id}"),
            xp: 0,
        }
    }

    /// Вытянуть все сообщения из подписки без ожидания.
    fn drain(rx: &mut broadcast::Receiver<ServerMsg>) -> Vec<ServerMsg> {
        let mut out = vec![];
        while let Ok(msg) = rx.try_recv() {
            out.push(msg);
        }
        out
    }

    #[tokio::test]
    async fn create_and_list() {
        let lobby = test_lobby();
        let id = lobby
            .create_room(
                &rec(1, "Тима"),
                "Комната".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let list = lobby.room_list().await;
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].room_id, id);
        assert_eq!(list[0].players, 1);
        assert!(!list[0].has_password);
        assert_eq!(list[0].code.len(), 6);
    }

    #[tokio::test]
    async fn join_password_flow() {
        let lobby = test_lobby();
        lobby
            .create_room(
                &rec(1, "Тима"),
                "Секрет".into(),
                Some("1234".into()),
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();

        assert_eq!(
            lobby.join_room(&rec(2, "Боб"), &code, None).await,
            Err(LobbyError::Code(ErrCode::WrongPassword))
        );
        assert_eq!(
            lobby
                .join_room(&rec(2, "Боб"), "000000", Some("1234"))
                .await,
            Err(LobbyError::Code(ErrCode::RoomNotFound))
        );
        let joined = lobby
            .join_room(&rec(2, "Боб"), &code, Some("1234"))
            .await
            .unwrap();
        assert_eq!(joined.kind, JoinKind::Player);
        // Повторный вход тем же игроком запрещён.
        assert_eq!(
            lobby.join_room(&rec(2, "Боб"), &code, Some("1234")).await,
            Err(LobbyError::Code(ErrCode::NotAllowed))
        );
    }

    #[tokio::test]
    async fn overflow_becomes_spectator_then_full() {
        let lobby = test_lobby();
        let settings = RoomSettings { max_players: 2, ..RoomSettings::default() };
        lobby
            .create_room(
                &rec(1, "Тима"),
                "Тесно".into(),
                None,
                GameMode::Coop,
                settings,
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();

        assert_eq!(
            lobby
                .join_room(&rec(2, "Б"), &code, None)
                .await
                .unwrap()
                .kind,
            JoinKind::Player
        );
        assert_eq!(
            lobby
                .join_room(&rec(3, "В"), &code, None)
                .await
                .unwrap()
                .kind,
            JoinKind::Spectator
        );
        // Зрители: до MAX_SPECTATORS (5) входят, шестой получает RoomFull.
        for i in 0..MAX_SPECTATORS - 1 {
            let r = rec(4 + i as u32, &format!("З{i}"));
            assert_eq!(
                lobby.join_room(&r, &code, None).await.unwrap().kind,
                JoinKind::Spectator
            );
        }
        assert_eq!(
            lobby.join_room(&rec(99, "Лишний"), &code, None).await,
            Err(LobbyError::Code(ErrCode::RoomFull))
        );
    }

    #[tokio::test]
    async fn ready_check_and_host_start() {
        let lobby = test_lobby();
        lobby
            .create_room(
                &rec(1, "Тима"),
                "R".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();

        // Не-хост не может стартовать.
        assert_eq!(
            lobby.start_match(2, 0).await,
            Err(LobbyError::Code(ErrCode::NotHost))
        );
        // Не все готовы.
        assert_eq!(
            lobby.start_match(1, 0).await,
            Err(LobbyError::Code(ErrCode::NotAllowed))
        );
        lobby.set_ready(2, 0, true).await.unwrap();
        // Хост тоже должен быть готов.
        assert_eq!(
            lobby.start_match(1, 0).await,
            Err(LobbyError::Code(ErrCode::NotAllowed))
        );
        lobby.set_ready(1, 0, true).await.unwrap();
        lobby.start_match(1, 0).await.unwrap();
        assert!(lobby.room_list().await[0].in_progress);
    }

    #[tokio::test]
    async fn kick_requires_host() {
        let lobby = test_lobby();
        lobby
            .create_room(
                &rec(1, "Тима"),
                "K".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();

        assert_eq!(
            lobby.kick(2, 0, 1).await,
            Err(LobbyError::Code(ErrCode::NotHost))
        );
        lobby.kick(1, 0, 2).await.unwrap();
        let state = lobby.room_list().await;
        assert_eq!(state[0].players, 1);
    }

    #[tokio::test]
    async fn host_migrates_and_empty_room_dies() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "M".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();
        lobby.join_room(&rec(3, "ВRon"), &code, None).await.unwrap();

        // Хост уходит → хостом становится следующий игрок.
        lobby.leave_room(1, room_id).await;
        assert!(lobby.room_exists(room_id).await);
        let mut rx = lobby.subscribe(room_id).await.unwrap();
        let _ = drain(&mut rx);
        lobby.leave_room(2, room_id).await;
        let msgs = drain(&mut rx);
        assert!(msgs.contains(&ServerMsg::HostChanged { player_id: 3 }));

        // Последний игрок уходит → комната удалена.
        assert!(!lobby.leave_room(3, room_id).await);
        assert!(!lobby.room_exists(room_id).await);
    }

    #[tokio::test]
    async fn reconnect_window_keeps_seat() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "R".into(),
                Some("пароль".into()),
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby
            .join_room(&rec(2, "Боб"), &code, Some("пароль"))
            .await
            .unwrap();

        // обрыв: слот остаётся, флаг online гаснет
        assert!(lobby.player_disconnected(2, room_id).await);
        let state = lobby.room_state(room_id).await.unwrap();
        assert_eq!(state.players.len(), 2);
        assert!(!state.players.iter().find(|p| p.id == 2).unwrap().online);

        // возврат по коду без пароля — реконнект
        let joined = lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();
        assert_eq!(joined.kind, JoinKind::Player);
        let state = lobby.room_state(room_id).await.unwrap();
        assert!(state.players.iter().find(|p| p.id == 2).unwrap().online);
    }

    #[tokio::test]
    async fn sweep_removes_expired_reconnects() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "R".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();
        lobby.player_disconnected(2, room_id).await;

        // вручную протухаем дедлайн
        {
            let mut inner = lobby.inner.lock().await;
            let room = inner.rooms.get_mut(&room_id).unwrap();
            room.reconnects
                .insert(2, Instant::now() - Duration::from_secs(1));
        }
        lobby.sweep_reconnects().await;
        let state = lobby.room_state(room_id).await.unwrap();
        assert_eq!(state.players.len(), 1, "не вернувшийся игрок удалён");
    }

    #[tokio::test]
    async fn bots_added_and_removed_by_host() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "B".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();

        // не-хост не может
        assert_eq!(
            lobby.add_bot(99, room_id, BotLevel::Pro).await,
            Err(LobbyError::Code(ErrCode::NotHost))
        );
        lobby.add_bot(1, room_id, BotLevel::Normal).await.unwrap();
        lobby.add_bot(1, room_id, BotLevel::Pro).await.unwrap();
        let state = lobby.room_state(room_id).await.unwrap();
        assert_eq!(state.players.len(), 3);
        let bot = state.players.iter().find(|p| p.is_bot).unwrap();
        assert!(bot.ready && bot.online);
        assert!(bot.id >= popproto_core::BOT_ID_BASE);

        lobby.remove_bot(1, room_id, bot.id).await.unwrap();
        let state = lobby.room_state(room_id).await.unwrap();
        assert_eq!(state.players.len(), 2);

        // заполнение до отказа
        let settings = RoomSettings { max_players: 2, ..RoomSettings::default() };
        let room2 = lobby
            .create_room(&rec(5, "Два"), "B2".into(), None, GameMode::Coop, settings)
            .await
            .unwrap();
        lobby.add_bot(5, room2, BotLevel::Lazy).await.unwrap();
        assert_eq!(
            lobby.add_bot(5, room2, BotLevel::Lazy).await,
            Err(LobbyError::Code(ErrCode::RoomFull))
        );
    }

    #[tokio::test]
    async fn finish_match_awards_xp_to_players() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "X".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        lobby
            .join_room(
                &rec(2, "Боб"),
                &lobby.room_list().await[0].code.clone(),
                None,
            )
            .await
            .unwrap();

        // 500 и 250 очков -> 50 и 25 XP; бот (>= BOT_ID_BASE) не получает
        lobby
            .finish_match(
                room_id,
                vec![(1, 500), (2, 250), (popproto_core::BOT_ID_BASE, 0)],
            )
            .await;
        let state = lobby.room_state(room_id).await.unwrap();
        let p1 = state.players.iter().find(|p| p.id == 1).unwrap();
        let p2 = state.players.iter().find(|p| p.id == 2).unwrap();
        assert_eq!(p1.xp, 50, "XP = очки/10");
        assert_eq!(p2.xp, 25);
    }

    #[tokio::test]
    async fn room_state_broadcast_to_subscribers() {
        let lobby = test_lobby();
        let room_id = lobby
            .create_room(
                &rec(1, "Тима"),
                "S".into(),
                None,
                GameMode::Coop,
                RoomSettings::default(),
            )
            .await
            .unwrap();
        let mut rx = lobby.subscribe(room_id).await.unwrap();
        let code = lobby.room_list().await[0].code.clone();
        lobby.join_room(&rec(2, "Боб"), &code, None).await.unwrap();

        let msgs = drain(&mut rx);
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::PlayerJoined(_))));
        assert!(msgs.iter().any(|m| matches!(m, ServerMsg::RoomState(_))));
    }
}
