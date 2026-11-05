//! Боссы PoP - Protocol (шаг 13 дорожной карты).
//! Каждый 3-й уровень — босс-арена. HP босса = база × число живых игроков.
//!
//! Три босса (спецификация §6):
//! 0 **Шар-Кракен** — щупальца-щиты (пока живы, босс неуязвим), перерождает их.
//! 1 **Ледяной Голет** — замораживающие снаряды (стан ракетки 2 с).
//! 2 **Мега-Кубик** — без щитов, но фаза ярости при 30% HP: быстрее и чаще бьёт.

use popproto_core::field::Field;
use popproto_core::{BossSnap, FIELD_H};

pub const BOSS_KINDS: u8 = 3;
/// Ударов мячом на одного живого игрока.
pub const BOSS_HP_PER_PLAYER: u32 = 30;
pub const BOSS_R: f32 = 42.0;
/// Снаряды босса.
pub const PROJ_SPEED: f32 = 260.0;
pub const STUN_SECS: f32 = 2.0;
/// Доля HP, после которой Мега-Кубик впадает в ярость.
pub const ENRAGE_AT: f32 = 0.3;

#[derive(Debug, Clone)]
pub struct Shield {
    pub id: u16,
    pub hp: u8,
    /// Угол на орбите вокруг босса, радианы.
    pub angle: f32,
}

#[derive(Debug, Clone)]
pub struct Proj {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub vx: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossKind {
    Kraken,
    Golem,
    Cube,
}

impl BossKind {
    pub fn from_index(i: u8) -> Self {
        match i % BOSS_KINDS {
            0 => BossKind::Kraken,
            1 => BossKind::Golem,
            _ => BossKind::Cube,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            BossKind::Kraken => 0,
            BossKind::Golem => 1,
            BossKind::Cube => 2,
        }
    }

    fn speed(self) -> f32 {
        match self {
            BossKind::Kraken => 120.0,
            BossKind::Golem => 70.0,
            BossKind::Cube => 90.0,
        }
    }

    fn attack_period(self) -> f32 {
        match self {
            BossKind::Kraken => 3.0,
            BossKind::Golem => 2.5,
            BossKind::Cube => 2.0,
        }
    }

    fn proj_count(self) -> usize {
        match self {
            BossKind::Kraken => 1,
            BossKind::Golem => 2,
            BossKind::Cube => 2,
        }
    }

    fn shield_count(self) -> usize {
        match self {
            BossKind::Kraken => 3,
            BossKind::Golem => 2,
            BossKind::Cube => 0,
        }
    }

    fn shield_hp(self) -> u8 {
        match self {
            BossKind::Kraken => 4,
            BossKind::Golem => 5,
            BossKind::Cube => 0,
        }
    }

    fn shield_respawn(self) -> f32 {
        match self {
            BossKind::Kraken => 12.0,
            BossKind::Golem => 15.0,
            BossKind::Cube => 0.0,
        }
    }
}

#[derive(Debug)]
pub struct Boss {
    pub kind: BossKind,
    pub hp: u32,
    pub max_hp: u32,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub shields: Vec<Shield>,
    /// Таймер до перерождения щитов.
    pub shield_respawn: f32,
    pub attack_timer: f32,
    pub projs: Vec<Proj>,
    pub enraged: bool,
    /// Вздрогнуть при уроне (клиент рисует вспышку).
    pub hit_flash: f32,
    next_ent_id: u16,
}

impl Boss {
    /// Босс для уровня: kind чередуется с номером босс-уровня.
    pub fn new(kind_index: u8, players: usize, field_w: f32) -> Self {
        let kind = BossKind::from_index(kind_index);
        let hp = BOSS_HP_PER_PLAYER * players.max(1) as u32;
        let mut b = Self {
            kind,
            hp,
            max_hp: hp,
            x: field_w / 2.0,
            y: 160.0,
            vx: kind.speed(),
            vy: if kind == BossKind::Cube { kind.speed() * 0.6 } else { 0.0 },
            shields: vec![],
            shield_respawn: 0.0,
            attack_timer: kind.attack_period(),
            projs: vec![],
            enraged: false,
            hit_flash: 0.0,
            next_ent_id: 0,
        };
        b.spawn_shields();
        b
    }

    fn spawn_shields(&mut self) {
        let count = self.kind.shield_count();
        let hp = self.kind.shield_hp();
        for i in 0..count {
            self.shields.push(Shield {
                id: self.next_ent_id,
                hp,
                angle: i as f32 * std::f32::consts::TAU / count.max(1) as f32,
            });
            self.next_ent_id += 1;
        }
    }

    pub fn vulnerable(&self) -> bool {
        self.shields.is_empty()
    }

    pub fn snap(&self) -> BossSnap {
        BossSnap {
            kind: self.kind.index(),
            hp: self.hp,
            max_hp: self.max_hp,
            phase: if self.enraged { 1 } else { 0 },
            x: popproto_core::encode_coord(self.x),
            y: popproto_core::encode_coord(self.y),
        }
    }

    /// Один шаг босса. `paddle_xs` — X живых ракеток (прицеливание снарядов).
    /// Возвращает (станут ли ракетки слотов, умер ли босс).
    pub fn step(&mut self, dt: f32, field: &mut Field, paddle_xs: &[(usize, f32)]) -> (Vec<usize>, bool) {
        let mut stunned: Vec<usize> = vec![];
        let mut died = false;
        let speed_mult = if self.enraged { 1.6 } else { 1.0 };
        self.hit_flash = (self.hit_flash - dt).max(0.0);

        // движение: горизонталь туда-сюда; кубик скачет по диагонали
        self.x += self.vx * speed_mult * dt;
        self.y += self.vy * speed_mult * dt;
        if self.x < BOSS_R {
            self.x = BOSS_R;
            self.vx = self.vx.abs();
        }
        if self.x > field.w - BOSS_R {
            self.x = field.w - BOSS_R;
            self.vx = -self.vx.abs();
        }
        let top = popproto_core::field::GRID_TOP + 40.0;
        let bottom = 300.0;
        if self.kind == BossKind::Cube {
            if self.y < top {
                self.y = top;
                self.vy = self.vy.abs();
            }
            if self.y > bottom {
                self.y = bottom;
                self.vy = -self.vy.abs();
            }
        } else {
            // парение по синусоиде
            self.y = 160.0 + (self.x * 0.01).sin() * 30.0;
        }

        // щиты: перерождение
        if self.kind.shield_count() > 0 && self.shields.is_empty() {
            self.shield_respawn -= dt;
            if self.shield_respawn <= 0.0 {
                self.spawn_shields();
                self.shield_respawn = self.kind.shield_respawn();
            }
        }
        // орбита щитов
        let t = self.attack_timer;
        for (i, sh) in self.shields.iter_mut().enumerate() {
            sh.angle += dt * 1.5;
            let _ = (i, t);
        }

        // атака: снаряды в случайные живые ракетки
        self.attack_timer -= dt * speed_mult;
        if self.attack_timer <= 0.0 && !paddle_xs.is_empty() {
            self.attack_timer = self.kind.attack_period();
            for _ in 0..self.kind.proj_count() {
                let id = self.next_ent_id;
                self.next_ent_id += 1;
                let jitter = (self.next_ent_id % 40) as f32 - 20.0;
                self.projs.push(Proj {
                    id,
                    x: self.x + jitter,
                    y: self.y + BOSS_R,
                    vx: 0.0,
                    vy: PROJ_SPEED,
                });
            }
        }

        // снаряды: падение, попадание по ракетке
        let paddle_y = field.h - 42.0;
        let mut survivors = Vec::with_capacity(self.projs.len());
        for mut p in self.projs.drain(..) {
            p.y += p.vy * dt;
            p.x += p.vx * dt;
            let mut dead = p.y > field.h + 20.0;
            if !dead {
                for (slot, px) in paddle_xs {
                    if p.y + 6.0 >= paddle_y && p.y - 6.0 <= paddle_y + popproto_core::field::PADDLE_H + 8.0
                        && (p.x - px).abs() <= 70.0
                    {
                        stunned.push(*slot);
                        dead = true;
                        break;
                    }
                }
            }
            if !dead {
                survivors.push(p);
            }
        }
        self.projs = survivors;

        if self.hp == 0 {
            died = true;
            // босс умирает: снаряды исчезают
            self.projs.clear();
        }
        (stunned, died)
    }

    /// Попадание мяча: урон по щиту или боссу. Возвращает true, если мяч погашен щитом.
    pub fn hit_by_ball(&mut self, ball: &mut popproto_core::field::Ball, rng: f32) -> bool {
        // щит ближе — мяч бьётся об него
        for sh in &mut self.shields {
            let sx = self.x + sh.angle.cos() * (BOSS_R + 18.0);
            let sy = self.y + sh.angle.sin() * (BOSS_R + 18.0);
            let dx = ball.x - sx;
            let dy = ball.y - sy;
            if dx * dx + dy * dy <= 22.0 * 22.0 {
                let d = (dx * dx + dy * dy).sqrt().max(0.01);
                let (nx, ny) = (dx / d, dy / d);
                let dot = ball.vx * nx + ball.vy * ny;
                if dot < 0.0 {
                    ball.vx -= 2.0 * dot * nx;
                    ball.vy -= 2.0 * dot * ny;
                    ball.x = sx + nx * 23.0;
                    ball.y = sy + ny * 23.0;
                }
                sh.hp -= 1;
                if sh.hp == 0 {
                    self.shields.retain(|s| s.hp > 0);
                    self.shield_respawn = self.kind.shield_respawn();
                }
                return true;
            }
        }
        if !self.vulnerable() {
            return false;
        }
        // босс: отражение + урон
        let dx = ball.x - self.x;
        let dy = ball.y - self.y;
        let d = (dx * dx + dy * dy).sqrt().max(0.01);
        let (nx, ny) = (dx / d, dy / d);
        let dot = ball.vx * nx + ball.vy * ny;
        if dot < 0.0 {
            // отражение с разбросом (босс «отбивает» мячи)
            let a = ny.atan2(nx) + (rng - 0.5) * 0.9;
            let sp = (ball.vx * ball.vx + ball.vy * ball.vy).sqrt();
            ball.vx = a.cos() * sp;
            ball.vy = a.sin() * sp;
            ball.x = self.x + a.cos() * (BOSS_R + popproto_core::field::BALL_R);
            ball.y = self.y + a.sin() * (BOSS_R + popproto_core::field::BALL_R);
        }
        self.hp = self.hp.saturating_sub(1);
        self.hit_flash = 0.15;
        if !self.enraged && self.kind == BossKind::Cube && (self.hp as f32) <= self.max_hp as f32 * ENRAGE_AT {
            self.enraged = true;
        }
        self.hp == 0
    }
}

/// Высота, на которой висит босс; используется при проверках поля.
pub fn boss_ceiling() -> f32 {
    FIELD_H
}
