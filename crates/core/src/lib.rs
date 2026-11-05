//! PoP - Protocol core: общие константы, игровые размеры и сетевой протокол.
//! Используется сервером и ботом; клиент получает те же типы как бинарный протокол.

pub mod field;
pub mod generator;
pub mod protocol;

pub use protocol::*;

use serde::{Deserialize, Serialize};

/* ---------- константы из спецификации ---------- */
pub const MAX_PLAYERS: usize = 10;
pub const MAX_SPECTATORS: usize = 5;
pub const TICK_HZ: u32 = 30;
pub const SNAPSHOT_INTERVAL_MS: u64 = 1000 / TICK_HZ as u64;
pub const RECONNECT_WINDOW_SECS: u64 = 90;
pub const AFK_TIMEOUT_SECS: u64 = 20;

pub const BASE_SPEED: f32 = 400.0;
pub const MAX_SPEED: f32 = 720.0;

/// Размеры поля для 1–2 игроков; ширина растёт с числом игроков
/// до ~1600 при десяти (см. спецификацию §4).
pub const FIELD_W: f32 = 800.0;
pub const FIELD_H: f32 = 600.0;
/// Прирост ширины поля за каждого дополнительного игрока сверх двух.
pub const FIELD_W_PER_PLAYER: f32 = 100.0;

/// Ширина поля для заданного числа живых игроков.
pub fn field_width(players: usize) -> f32 {
    if players <= 2 {
        FIELD_W
    } else {
        FIELD_W + FIELD_W_PER_PLAYER * (players.min(MAX_PLAYERS) - 2) as f32
    }
}

/* ---------- кодирование ---------- */

/// Сериализация сообщения в бинарный кадр WebSocket.
pub fn encode(msg: &impl Serialize) -> Vec<u8> {
    bincode::serialize(msg).expect("protocol types are always serializable")
}

/// Декодирование бинарного кадра; None на мусоре или чужой версии протокола.
pub fn decode<'a, T: Deserialize<'a>>(bytes: &'a [u8]) -> Option<T> {
    bincode::deserialize(bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn client_msg_roundtrip() {
        let msgs = vec![
            ClientMsg::Hello {
                protocol: PROTOCOL_VERSION,
                name: "Тима".into(),
                token: Some("abc123".into()),
            },
            ClientMsg::PaddleX(412.75),
            ClientMsg::Action,
            ClientMsg::Ping(42),
        ];
        for msg in msgs {
            assert_eq!(decode::<ClientMsg>(&encode(&msg)), Some(msg));
        }
    }

    #[test]
    fn garbage_decodes_to_none() {
        let mut junk = encode(&ClientMsg::Ping(1));
        junk[0] ^= 0xFF;
        assert_eq!(decode::<ClientMsg>(&junk), None);
    }

    #[test]
    fn field_width_scales_with_players() {
        assert_eq!(field_width(1), 800.0);
        assert_eq!(field_width(2), 800.0);
        assert_eq!(field_width(6), 1200.0);
        assert_eq!(field_width(10), 1600.0);
        assert_eq!(field_width(12), 1600.0); // потолок MAX_PLAYERS
    }
}
