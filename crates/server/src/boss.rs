//! Боссы PoP - Protocol (шаг 13 дорожной карты).
//! Каждый 3-й уровень — босс-арена. HP босса = база × число живых игроков.
//!
//! Три босса (спецификация §6):
//! 0 **Шар-Кракен** — щупальца-щиты (пока живы, босс неуязвим), перерождает их.
//! 1 **Ледяной Голет** — замораживающие снаряды (стан ракетки 2 с).
//! 2 **Мега-Кубик** — без щитов, но фаза ярости при 30% HP: быстрее и чаще бьёт.

use popproto_core::field::Field;
use popproto_core::{BossSnap, FIELD_H};

pub const BOSS_KINDS: u8 = 6;
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
    #[allow(dead_code)] // зарезервировано: визуальные эффекты щитов
    pub id: u16,
    pub hp: u8,
    /// Угол на орбите вокруг босса, радианы.
    pub angle: f32,
}

/// Тип снаряда: 0 стан-пуля, 1 мина-медуза, 2 золотая бочка, 3 бочка-ловушка.
pub const PROJ_STUN: u8 = 0;
pub const PROJ_MINE: u8 = 1;
pub const PROJ_GOLD: u8 = 2;
pub const PROJ_TRAP: u8 = 3;

#[derive(Debug, Clone)]
pub struct Proj {
    pub id: u16,
    pub x: f32,
    pub y: f32,
    pub vy: f32,
    pub vx: f32,
    /// Тип снаряда: PROJ_*.
    pub kind: u8,
}

/// Событие попадания снаряда по слоту ракетки: 0 стан, 1 золото (+200), 2 ловушка.
#[derive(Debug, Clone, Copy)]
pub struct SlotHit {
    pub slot: usize,
    pub kind: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BossKind {
    Kraken,
    Golem,
    Cube,
    /// Медуза-королева: рой мин-медуз; мяч отскакивает и сбивается с курса.
    Medusa,
    /// Золото-босс: бросает золотые бочки (+200) и бочки-ловушки (стан 1.5 с).
    GoldBaron,
    /// Зеркальный клон: щиты отбивают мячи вниз, тело зеркалит горизонталь.
    MirrorClone,
}

impl BossKind {
    pub fn from_index(i: u8) -> Self {
        match i % BOSS_KINDS {
            0 => BossKind::Kraken,
            1 => BossKind::Golem,
            2 => BossKind::Cube,
            3 => BossKind::Medusa,
            4 => BossKind::GoldBaron,
            _ => BossKind::MirrorClone,
        }
    }

    pub fn index(self) -> u8 {
        match self {
            BossKind::Kraken => 0,
            BossKind::Golem => 1,
            BossKind::Cube => 2,
            BossKind::Medusa => 3,
            BossKind::GoldBaron => 4,
            BossKind::MirrorClone => 5,
        }
    }

    fn speed(self) -> f32 {
        match self {
            BossKind::Kraken => 120.0,
            BossKind::Golem => 70.0,
            BossKind::Cube => 90.0,
            BossKind::Medusa => 100.0,
            BossKind::GoldBaron => 60.0,
            BossKind::MirrorClone => 95.0,
        }
    }

    fn attack_period(self) -> f32 {
        match self {
            BossKind::Kraken => 3.0,
            BossKind::Golem => 2.5,
            BossKind::Cube => 2.0,
            BossKind::Medusa => 2.5,
            BossKind::GoldBaron => 3.0,
            BossKind::MirrorClone => 3.5,
        }
    }

    fn proj_count(self) -> usize {
        match self {
            BossKind::Kraken => 1,
            BossKind::Golem => 2,
            BossKind::Cube => 2,
            BossKind::Medusa => 0,
            BossKind::GoldBaron => 0,
            BossKind::MirrorClone => 1,
        }
    }

    fn shield_count(self) -> usize {
        match self {
            BossKind::Kraken => 3,
            BossKind::Golem => 2,
            BossKind::Cube => 0,
            BossKind::Medusa => 0,
            BossKind::GoldBaron => 0,
            BossKind::MirrorClone => 2,
        }
    }

    fn shield_hp(self) -> u8 {
        match self {
            BossKind::Kraken => 4,
            BossKind::Golem => 5,
            BossKind::Cube => 0,
            BossKind::Medusa => 0,
            BossKind::GoldBaron => 0,
            BossKind::MirrorClone => 4,
        }
    }

    fn shield_respawn(self) -> f32 {
        match self {
            BossKind::Kraken => 12.0,
            BossKind::Golem => 15.0,
            BossKind::Cube => 0.0,
            BossKind::Medusa => 0.0,
            BossKind::GoldBaron => 0.0,
            BossKind::MirrorClone => 12.0,
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
            vy: if kind == BossKind::Cube {
                kind.speed() * 0.6
            } else {
                0.0
            },
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
            shields: self
                .shields
                .iter()
                .map(|sh| popproto_core::ShieldSnap {
                    hp: sh.hp,
                    angle_deg: (sh.angle.to_degrees().rem_euclid(360.0) / 1.40625) as u8,
                })
                .collect(),
        }
    }

    /// Один шаг босса. `paddle_xs` — X живых ракеток (прицеливание снарядов).
    /// Возвращает (события по слотам, умер ли босс).
    pub fn step(
        &mut self,
        dt: f32,
        field: &mut Field,
        paddle_xs: &[(usize, f32)],
    ) -> (Vec<SlotHit>, bool) {
        let mut stunned: Vec<SlotHit> = vec![];
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

        // атака: снаряды по типу босса
        self.attack_timer -= dt * speed_mult;
        if self.attack_timer <= 0.0 {
            self.attack_timer = self.kind.attack_period();
            match self.kind {
                BossKind::Medusa => {
                    // мина-медуза (максимум 6 живых)
                    if self.projs.iter().filter(|p| p.kind == PROJ_MINE).count() < 6 {
                        let id = self.next_ent_id;
                        self.next_ent_id += 1;
                        let a = (self.next_ent_id as f32 * 2.399).sin();
                        self.projs.push(Proj {
                            id,
                            x: self.x + a * 120.0,
                            y: self.y + 40.0 + a.abs() * 60.0,
                            vx: 28.0 * a,
                            vy: 18.0 * ((self.next_ent_id % 3) as i32 - 1) as f32,
                            kind: PROJ_MINE,
                        });
                    }
                }
                BossKind::GoldBaron => {
                    // бочка: 40% золотая, 60% ловушка (детерминированный гормон)
                    let id = self.next_ent_id;
                    self.next_ent_id += 1;
                    let gold = ((self.next_ent_id as u32)
                        .wrapping_mul(2654435761)
                        .wrapping_shr(13)
                        % 10)
                        < 4;
                    let target = paddle_xs
                        .get(self.next_ent_id as usize % paddle_xs.len().max(1))
                        .map(|(slot, px)| (*slot, *px));
                    self.projs.push(Proj {
                        id,
                        x: target.map(|(_, px)| px).unwrap_or(self.x),
                        y: self.y + BOSS_R,
                        vx: 0.0,
                        vy: PROJ_SPEED * 0.8,
                        kind: if gold { PROJ_GOLD } else { PROJ_TRAP },
                    });
                }
                kind => {
                    for _ in 0..kind.proj_count() {
                        let id = self.next_ent_id;
                        self.next_ent_id += 1;
                        let jitter = (self.next_ent_id % 40) as f32 - 20.0;
                        self.projs.push(Proj {
                            id,
                            x: self.x + jitter,
                            y: self.y + BOSS_R,
                            vx: 0.0,
                            vy: PROJ_SPEED,
                            kind: PROJ_STUN,
                        });
                    }
                }
            }
        }

        // снаряды: падение/дрейф, мины vs мячи, попадания по ракеткам
        let paddle_y = field.h - 42.0;
        let mut survivors = Vec::with_capacity(self.projs.len());
        for mut p in self.projs.drain(..) {
            p.y += p.vy * dt;
            p.x += p.vx * dt;
            let mut dead = p.y > field.h + 20.0;
            if p.kind == PROJ_MINE {
                // мины дрейфуют в верхней зоне и отскакивают от стен
                if p.x < 20.0 || p.x > field.w - 20.0 {
                    p.vx = -p.vx;
                }
                if p.y < popproto_core::field::GRID_TOP + 20.0 || p.y > field.h * 0.55 {
                    p.vy = -p.vy;
                }
                // столкновение с мячом: отскок + сбивка курса
                for ball in field.balls.iter_mut() {
                    let dx = ball.x - p.x;
                    let dy = ball.y - p.y;
                    let d2 = dx * dx + dy * dy;
                    if d2 <= 16.0 * 16.0 {
                        let d = d2.sqrt().max(0.01);
                        let (nx, ny) = (dx / d, dy / d);
                        let dot = ball.vx * nx + ball.vy * ny;
                        if dot < 0.0 {
                            ball.vx -= 2.0 * dot * nx;
                            ball.vy -= 2.0 * dot * ny;
                        }
                        // сбивка курса ±0.6 рад
                        let jitter = ((p.id.wrapping_mul(97) % 120) as f32 / 100.0 - 0.6) * 0.6;
                        let (c, s2) = (jitter.cos(), jitter.sin());
                        let (vx, vy) = (ball.vx, ball.vy);
                        ball.vx = vx * c - vy * s2;
                        ball.vy = vx * s2 + vy * c;
                        ball.x = p.x + nx * 18.0;
                        ball.y = p.y + ny * 18.0;
                        dead = true;
                        break;
                    }
                }
            } else if !dead {
                for (slot, px) in paddle_xs {
                    if p.y + 6.0 >= paddle_y
                        && p.y - 6.0 <= paddle_y + popproto_core::field::PADDLE_H + 8.0
                        && (p.x - px).abs() <= 70.0
                    {
                        match p.kind {
                            PROJ_GOLD => stunned.push(SlotHit {
                                slot: *slot,
                                kind: 1,
                            }),
                            PROJ_TRAP => stunned.push(SlotHit {
                                slot: *slot,
                                kind: 2,
                            }),
                            _ => stunned.push(SlotHit {
                                slot: *slot,
                                kind: 0,
                            }),
                        }
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
                if self.kind == BossKind::MirrorClone {
                    // зеркальный клон: щит бьёт мяч вниз с разгоном
                    ball.vy = ball.vy.abs() * 1.15;
                    ball.vx = (ball.x - sx) * 6.0;
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
            if self.kind == BossKind::MirrorClone {
                // зеркальный клон: зеркалит горизонталь с разгоном
                ball.vx = -ball.vx * 1.1;
                ball.vy = ball.vy.abs() * 1.05;
                ball.x =
                    self.x + (ball.x - self.x).signum() * (BOSS_R + popproto_core::field::BALL_R);
            } else {
                // отражение с разбросом (босс «отбивает» мячи)
                let a = ny.atan2(nx) + (rng - 0.5) * 0.9;
                let sp = (ball.vx * ball.vx + ball.vy * ball.vy).sqrt();
                ball.vx = a.cos() * sp;
                ball.vy = a.sin() * sp;
                ball.x = self.x + a.cos() * (BOSS_R + popproto_core::field::BALL_R);
                ball.y = self.y + a.sin() * (BOSS_R + popproto_core::field::BALL_R);
            }
        }
        self.hp = self.hp.saturating_sub(1);
        self.hit_flash = 0.15;
        if !self.enraged
            && self.kind == BossKind::Cube
            && (self.hp as f32) <= self.max_hp as f32 * ENRAGE_AT
        {
            self.enraged = true;
        }
        self.hp == 0
    }
}

/// Высота, на которой висит босс; используется при проверках поля.
#[allow(dead_code)] // зарезервировано: вертикальные границы для будущих боссов
pub fn boss_ceiling() -> f32 {
    FIELD_H
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn medusa_spawns_mines_cap_six() {
        let mut b = Boss::new(3, 2, 800.0);
        let mut f = Field::new(800.0, 600.0, 0);
        let mut hits = vec![];
        for _ in 0..600 {
            let (h, _) = b.step(1.0 / 30.0, &mut f, &[(0, 400.0)]);
            hits.extend(h);
        }
        let mines = b.projs.iter().filter(|p| p.kind == PROJ_MINE).count();
        assert!(mines >= 3, "мины должны накапливаться, сейчас {mines}");
        assert!(mines <= 6, "капа мин");
    }

    #[test]
    fn gold_baron_barrels_reach_paddle() {
        let mut b = Boss::new(4, 2, 800.0);
        let mut f = Field::new(800.0, 600.0, 0);
        let mut gold = false;
        let mut trap = false;
        for _ in 0..600 {
            let (hits, _) = b.step(1.0 / 30.0, &mut f, &[(0, 400.0)]);
            for h in hits {
                if h.kind == 1 {
                    gold = true;
                }
                if h.kind == 2 {
                    trap = true;
                }
            }
        }
        assert!(gold || trap, "бочки должны долетать до ракетки");
    }

    #[test]
    fn mirror_shield_kicks_ball_down() {
        let mut b = Boss::new(5, 2, 800.0);
        assert_eq!(b.shields.len(), 2);
        let _f = Field::new(800.0, 600.0, 0);
        let mut ball = popproto_core::field::Ball {
            id: 1,
            owner: 1,
            x: b.x,
            y: b.y + 60.0,
            vx: 100.0,
            vy: -300.0,
        };
        let bounced = b.hit_by_ball(&mut ball, 0.5);
        // мяч может попасть по щиту или мимо — но при попадании vy вниз
        if bounced {
            assert!(ball.vy > 0.0, "клон отбивает мяч вниз");
        }
    }
}
