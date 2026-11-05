//! Сетевой протокол PoP - Protocol, версия 1.
//! Кадр WebSocket = bincode-код одного `ClientMsg` (клиент -> сервер)
//! или одного `ServerMsg` (сервер -> клиент).
//!
//! Правила совместимости: enum-варианты только добавлять в конец,
//! поля существующих вариантов не переименовывать и не менять тип.

use serde::{Deserialize, Serialize};

use crate::MAX_PLAYERS;

pub const PROTOCOL_VERSION: u16 = 4;

/// Синтетические id ботов: диапазон не пересекается с id игроков из БД.
pub const BOT_ID_BASE: u32 = 0xF000_0000;

/* ---------- клиент -> сервер ---------- */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ClientMsg {
    /// Первый пакет соединения. Сервер отвечает Welcome или Error.
    Hello { protocol: u16, name: String, token: Option<String> },
    RequestRoomList,
    CreateRoom { name: String, password: Option<String>, mode: GameMode, settings: RoomSettings },
    JoinRoom { code: String, password: Option<String> },
    LeaveRoom,
    SetReady(bool),
    /// Только хост; стартует матч, если все игроки готовы.
    StartMatch,
    /// Только хост.
    Kick { player_id: u32 },
    /// Движение своей ракетки, координата X поля.
    PaddleX(f32),
    /// Запуск мяча из состояния ready / выстрел лазером.
    Action,
    /// Текстовый чат — только в лобби комнаты; в бою действует колесо эмоций.
    Chat { text: String },
    /// Колесо эмоций в бою, индекс фразы 0..=9.
    Emote { emote: u8 },
    Ping(u64),
    /// Только хост: добавить бота в свободный слот.
    AddBot { level: BotLevel },
    /// Только хост: убрать бота.
    RemoveBot { player_id: u32 },
    /// Сменить цвет ракетки/мячей (если цвет свободен в комнате).
    SetColor { color: u8 },
}

/* ---------- сервер -> клиент ---------- */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ServerMsg {
    Welcome { player_id: u32, token: String, name: String },
    Error { code: ErrCode, text: String },
    Pong(u64),

    RoomList(Vec<RoomSummary>),
    /// Полное состояние комнаты: рассылается всем её участникам при любом изменении.
    RoomState(Box<RoomState>),
    MatchStarted,
    Snapshot(Box<Snapshot>),
    LevelCleared { level: u32 },
    MatchOver { reason: MatchOverReason, scores: Vec<FinalScore> },

    EventStarted { event: EventKind, duration_secs: f32 },
    EventEnded { event: EventKind },

    Chat { player_id: u32, text: String },
    Emote { player_id: u32, emote: u8 },

    PlayerJoined(Box<PlayerInfo>),
    PlayerLeft { player_id: u32 },
    HostChanged { player_id: u32 },
}

/* ---------- лобби и комнаты ---------- */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum GameMode {
    /// Кооп: общее поле, каждый 3-й уровень — босс.
    Coop,
    /// Босс-раш: боссы подряд, счёт по скорости зачистки.
    BossRush,
    /// Волна: бесконечная процедурная подача кирпичей.
    Wave,
}

/// Настройки комнаты; меняет только хост.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoomSettings {
    /// Максимум игроков, жёсткий потолок MAX_PLAYERS (10).
    pub max_players: u8,
    pub events_enabled: bool,
    pub bosses_enabled: bool,
    /// Множитель базовой скорости мячей, 0.5..2.0.
    pub speed_factor: f32,
}

impl Default for RoomSettings {
    fn default() -> Self {
        Self { max_players: MAX_PLAYERS as u8, events_enabled: true, bosses_enabled: true, speed_factor: 1.0 }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoomSummary {
    pub room_id: u32,
    /// 6-значный код для входа в приватную комнату.
    pub code: String,
    pub name: String,
    pub mode: GameMode,
    pub players: u8,
    pub max_players: u8,
    pub has_password: bool,
    pub in_progress: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RoomState {
    pub room_id: u32,
    pub code: String,
    pub name: String,
    pub mode: GameMode,
    pub settings: RoomSettings,
    pub host_id: u32,
    pub phase: RoomPhase,
    /// Игроки и занятые ими слоты; зрители считаются отдельно (см. spectators).
    pub players: Vec<PlayerInfo>,
    pub spectators: Vec<PlayerInfo>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum RoomPhase {
    Lobby,
    Playing,
    Finished,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerInfo {
    pub id: u32,
    pub name: String,
    pub ready: bool,
    pub is_bot: bool,
    pub bot_level: Option<BotLevel>,
    pub score: u32,
    pub lives: u8,
    /// Индекс цвета из фиксированной палитры клиента.
    pub color: u8,
    pub xp: u32,
    /// Соединение живо (после обрыва игрок остаётся в слоте на окно реконнекта).
    pub online: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum BotLevel {
    Lazy,
    Normal,
    Pro,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum ErrCode {
    /// Несовпадение PROTOCOL_VERSION.
    OldProtocol,
    NameTaken,
    BadToken,
    BadName,
    RoomNotFound,
    RoomFull,
    WrongPassword,
    NotHost,
    NotAllowed,
    InvalidInput,
}

/* ---------- игровой снапшот ---------- */

/// Координаты в снапшотах — u16 в сотых долях единицы поля: value = (x * 100) как u16.
/// Это даёт точность 0.01 при поле шириной до 655 единиц... но поле до 1600,
/// поэтому X/Y кодируются как (value * 10): точность 0.1, диапазон 6553.5.
pub const COORD_SCALE: f32 = 10.0;

pub fn encode_coord(v: f32) -> u16 {
    (v * COORD_SCALE).round().max(0.0) as u16
}
pub fn decode_coord(v: u16) -> f32 {
    v as f32 / COORD_SCALE
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Snapshot {
    pub tick: u32,
    pub field_w: u16,
    pub field_h: u16,
    pub phase: MatchPhase,
    pub level: u32,
    pub players: Vec<PlayerSnap>,
    pub balls: Vec<BallSnap>,
    /// Новые и изменившиеся кирпичи; кирпичи с тем же id не пересылаются.
    pub bricks_upsert: Vec<BrickSnap>,
    /// id кирпичей, исчезнувших с прошлого снапшота.
    pub bricks_removed: Vec<u16>,
    pub capsules: Vec<CapsuleSnap>,
    pub bullets: Vec<BulletSnap>,
    pub floats: Vec<FloatSnap>,
    pub boss: Option<BossSnap>,
    pub events: Vec<ActiveEvent>,
    /// Индивидуальное комбо игрока (для плашки ×N), синхронизируется в снапшоте.
    pub combo: Vec<ComboSnap>,
    /// Очки и жизни игроков каждый тик (для HUD).
    pub scores: Vec<ScoreSnap>,
    /// Снаряды босса, летящие в ракетки (стан/заморозка).
    pub projs: Vec<ProjSnap>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ProjSnap {
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ScoreSnap {
    pub player_id: u32,
    pub score: u32,
    pub lives: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchPhase {
    /// Мячи на ракетках, ждём Action.
    Ready,
    Play,
    /// Уровень зачищен, короткая пауза перед следующим.
    LevelClear,
    Over,
    Win,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct PlayerSnap {
    pub id: u32,
    pub x: u16,
    pub w: u16,
    pub state: PlayerState,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum PlayerState {
    Alive,
    /// Потерял все жизни — зритель до следующего уровня.
    Spectator,
    /// Стан от босса: ракетка сжата и не движется.
    Stunned,
    /// АФК: ракеткой управляет бот.
    BotControlled,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct BallSnap {
    pub id: u16,
    /// Хозяин мяча: потеря этого мяча = минус жизнь хозяину.
    pub owner_id: u32,
    pub x: u16,
    pub y: u16,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct BrickSnap {
    pub id: u16,
    pub c: u8,
    pub r: u8,
    /// hp 1..=3; 255 = неразрушимый камень.
    pub hp: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct CapsuleSnap {
    pub id: u16,
    pub x: u16,
    pub y: u16,
    /// 0=M мультишар, 1=W широкая, 2=L лазер, 3=S замедление, 4=H жизнь.
    pub kind: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct BulletSnap {
    pub id: u16,
    pub owner_id: u32,
    pub x: u16,
    pub y: u16,
}

/// Всплывающий текст (+120, WIDE!, MULTI!).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FloatSnap {
    pub x: u16,
    pub y: u16,
    pub text: String,
    pub color: u8,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ComboSnap {
    pub player_id: u32,
    pub combo: u8,
}

/* ---------- боссы ---------- */

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BossSnap {
    /// 0 = Шар-Кракен, 1 = Ледяной Голет, 2 = Мега-Кубик.
    pub kind: u8,
    pub hp: u32,
    pub max_hp: u32,
    /// 0 — обычная, 1 — ярость (30% HP).
    pub phase: u8,
    pub x: u16,
    pub y: u16,
}

/* ---------- евенты ---------- */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EventKind {
    /// Кирпичи-кометы падают сверху и встраиваются в поле.
    MeteorShower,
    /// Зона видимости сужается вокруг мячей.
    Darkness,
    /// Зеркальное управление ракеткой.
    Inversion,
    /// Все кирпичи дают ×3 очков.
    GoldRush,
    /// Мячи ускоряются и краснеют.
    SpeedFrenzy,
    /// Падающие камни-зловреды, нужно выбить.
    Rockfall,
    /// 15 капсул разом.
    CapsuleFountain,
    /// У каждого второй мяч до конца уровня.
    DoubleBall,
    /// Общая стена внизу, спасает мячи один раз.
    GroundShield,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct ActiveEvent {
    pub event: EventKind,
    /// Секунд до конца евента.
    pub remaining_secs: f32,
}

/* ---------- завершение матча ---------- */

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum MatchOverReason {
    /// Все игроки выбыли.
    AllDead,
    /// Все уровни пройдены.
    Victory,
    /// В режимах Wave/BossRush — не осталось игроков.
    Exhausted,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FinalScore {
    pub player_id: u32,
    pub name: String,
    pub score: u32,
    /// Место в матче, 1 = лучший.
    pub place: u32,
    /// Лучшая серия комбо за матч (ачивка «Снайпер серии»).
    pub max_combo: u32,
    /// Разбитых кирпичей (ачивка «Клейщик»).
    pub bricks: u32,
    /// Попаданий по боссу (ачивка «Охотник»).
    pub boss_hits: u32,
}

/* ---------- вспомогательная палитра ---------- */

/// Фиксированная палитра цветов игроков, индексы в PlayerInfo.color.
pub const PLAYER_COLORS: [&str; 10] = [
    "#ff6b6b", "#feca57", "#48dbfb", "#1dd1a1", "#a55eea",
    "#ff9ff3", "#ee7724", "#4a69bd", "#10ac84", "#e0181d",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(msg: ServerMsg) {
        let bytes = crate::encode(&msg);
        assert_eq!(crate::decode::<ServerMsg>(&bytes), Some(msg));
    }

    #[test]
    fn hello_welcome_roundtrip() {
        roundtrip(ServerMsg::Welcome { player_id: 1, token: "tok123".into(), name: "Тима".into() });
        roundtrip(ServerMsg::Error { code: ErrCode::RoomFull, text: "комната полна".into() });
    }

    #[test]
    fn full_snapshot_roundtrip() {
        let snap = Snapshot {
            tick: 1234,
            field_w: encode_coord(1600.0),
            field_h: encode_coord(600.0),
            phase: MatchPhase::Play,
            level: 3,
            players: vec![
                PlayerSnap { id: 1, x: encode_coord(120.5), w: encode_coord(118.0), state: PlayerState::Alive },
                PlayerSnap { id: 2, x: encode_coord(400.0), w: encode_coord(118.0), state: PlayerState::BotControlled },
            ],
            balls: vec![BallSnap { id: 1, owner_id: 1, x: encode_coord(321.6), y: encode_coord(512.3) }],
            bricks_upsert: vec![BrickSnap { id: 10, c: 3, r: 1, hp: 2 }, BrickSnap { id: 11, c: 4, r: 1, hp: 255 }],
            bricks_removed: vec![9],
            capsules: vec![CapsuleSnap { id: 5, x: encode_coord(100.0), y: encode_coord(300.0), kind: 0 }],
            bullets: vec![BulletSnap { id: 6, owner_id: 1, x: encode_coord(120.0), y: encode_coord(540.0) }],
            floats: vec![FloatSnap { x: encode_coord(50.0), y: encode_coord(60.0), text: "+120".into(), color: 3 }],
            boss: Some(BossSnap { kind: 2, hp: 900, max_hp: 1000, phase: 1, x: encode_coord(800.0), y: encode_coord(150.0) }),
            events: vec![ActiveEvent { event: EventKind::Darkness, remaining_secs: 7.5 }],
            combo: vec![ComboSnap { player_id: 1, combo: 4 }],
            scores: vec![ScoreSnap { player_id: 1, score: 1230, lives: 2 }],
            projs: vec![ProjSnap { x: 100, y: 400 }],
        };
        roundtrip(ServerMsg::Snapshot(Box::new(snap)));
    }

    #[test]
    fn room_state_roundtrip() {
        let state = RoomState {
            room_id: 1,
            code: "482913".into(),
            name: "Комната друзей".into(),
            mode: GameMode::Coop,
            settings: RoomSettings::default(),
            host_id: 7,
            phase: RoomPhase::Lobby,
            players: vec![PlayerInfo {
                id: 7, name: "Тима".into(), ready: true, is_bot: false,
                bot_level: None, score: 0, lives: 3, color: 0, xp: 42, online: true,
            }],
            spectators: vec![],
        };
        roundtrip(ServerMsg::RoomState(Box::new(state)));
    }

    #[test]
    fn coord_roundtrip() {
        assert_eq!(decode_coord(encode_coord(800.0)), 800.0);
        // Точность кодирования — 0.05 единицы поля (полшага шкалы 0.1).
        let v = decode_coord(encode_coord(123.46));
        assert!((v - 123.46).abs() <= 0.05);
        assert_eq!(encode_coord(-5.0), 0);
    }

    #[test]
    fn event_kinds_complete() {
        // Ровно 9 евентов из спецификации §5 — новые добавлять только в конец.
        let all = [
            EventKind::MeteorShower, EventKind::Darkness, EventKind::Inversion,
            EventKind::GoldRush, EventKind::SpeedFrenzy, EventKind::Rockfall,
            EventKind::CapsuleFountain, EventKind::DoubleBall, EventKind::GroundShield,
        ];
        assert_eq!(all.len(), 9);
    }
}
