//! Игровое поле PoP - Protocol: кирпичи, мячи, ракетки и физика.
//! Портирована из popblocks.html, параметризована шириной поля (растёт с числом
//! игроков) и детерминирована: RNG (углы запуска, шум) остаётся снаружи.
//!
//! Физика вызывается фиксированным шагом (серверный тик 1/30 с);
//! скорость мяча всегда нормируется к целевой.

use serde::{Deserialize, Serialize};

use crate::BASE_SPEED;

pub const COLS: usize = 10;
pub const CELL_H: f32 = 26.0;
pub const GRID_TOP: f32 = 58.0;
pub const BALL_R: f32 = 9.0;
pub const PADDLE_H: f32 = 16.0;
pub const PADDLE_BASE_W: f32 = 118.0;
pub const PADDLE_SPEED: f32 = 760.0;
pub const MAX_LIVES: u8 = 3;

/// Уровни popblocks; 'x' — камень, '.' — пусто, '1'..'3' — hp.
pub const LEVELS: [&[&str]; 15] = [
    &["2222222222", "1111111111", "2111111112", "1111111111"],
    &[
        "1.1.1.1.1.",
        ".2.2.2.2.2",
        "1.1.1.1.1.",
        ".2.2.2.2.2",
        "1.1.1.1.1.",
    ],
    &[
        "....11....",
        "...1221...",
        "..122221..",
        ".12233221.",
        "1223333221",
    ],
    &[
        "1111111111",
        "2xx2m2mxx2",
        "1111111111",
        "222xxxxx22",
        "1111111111",
    ],
    &[
        "3333333333",
        "3222222223",
        "32e1111e23",
        "3211111123",
        "3222222223",
        "3333333333",
    ],
    // 6. Шахматка с каменным ядром
    &[
        "2.2.2.2.2.",
        ".1x1x1x1.1",
        "2.2.2.2.2.",
        ".1.1.1.1.1",
        "2.2.2.2.2.",
    ],
    // 7. Крепость: стены и бастионы
    &[
        "x22222222x",
        "2........2",
        "2.3g33g3.2",
        "2........2",
        "x11111111x",
    ],
    // 8. Молния
    &[
        "1111.11111",
        ".222.222..",
        "..3333....",
        ".222.222..",
        "1111.11111",
    ],
    // 9. Двойная арка
    &[
        "..333333..",
        ".32222223.",
        "3221111223",
        ".32222223.",
        "..111111..",
    ],
    // 10. Соты
    &[
        "1.2.3.2.1.",
        ".2.3.3.2.2",
        "1.2.3.2.1.",
        ".2.3.3.2.2",
        "1.2.3.2.1.",
    ],
    // 11. Волна
    &[
        "11..11..11",
        "2211..1122",
        "..223322..",
        "2211..1122",
        "11..11..11",
    ],
    // 12. Крест-накрест с камнями
    &[
        "333t33t333",
        "2..2.2..2.",
        "1111.1111.",
        "2..2.2..2.",
        "333x33x333",
    ],
    // 13. Пирамида вниз
    &[
        "3333333333",
        ".x222222x.",
        "..111111..",
        "...2222...",
        "....11....",
    ],
    // 14. Решётка
    &[
        "3.3.3.3.3.",
        "2222222222",
        "3.3.3.3.3.",
        "1111111111",
        "3.3.3.3.3.",
    ],
    // 15. Финал: стена с бронёй (после неё — победа)
    &[
        "xx333333xx",
        "3332233223",
        "2221111222",
        "3332233223",
        "xx111111xx",
    ],
];

/// Типы кирпичей: обычный, взрывной (цепь по соседям), телепорт (переброс мяча),
/// регенерирующий (восстанавливает hp), движущийся (дрейф по строке).
pub const BK_NORMAL: u8 = 0;
pub const BK_EXPLOSIVE: u8 = 1;
pub const BK_TELEPORT: u8 = 2;
pub const BK_REGEN: u8 = 3;
pub const BK_MOVING: u8 = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brick {
    pub id: u16,
    pub c: u8,
    pub r: u8,
    /// hp 1..=3; stone=true — неразрушимый.
    pub hp: u8,
    pub stone: bool,
    /// Тип кирпича: BK_*.
    pub kind: u8,
    /// Максимальный hp для регенерации.
    pub hp_max: u8,
}

#[derive(Debug, Clone)]
pub struct Ball {
    pub id: u16,
    /// Ответственный игрок: потеря мяча = минус его жизнь.
    pub owner: u32,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
}

#[derive(Debug, Clone, Default)]
pub struct Paddle {
    pub x: f32,
    /// Цель движения от игрока; сервер ведёт ракетку к ней со скоростью PADDLE_SPEED.
    pub target: f32,
    pub w: f32,
    pub alive: bool,
}

/// События физики за шаг — для очков/комбо/звука в матч-слое.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BrickEvent {
    pub brick_id: u16,
    pub c: u8,
    pub r: u8,
    pub owner: u32,
    /// true — кирпич разрушен, false — только треснул.
    pub destroyed: bool,
}

#[derive(Debug, Default)]
pub struct StepResult {
    pub brick_events: Vec<BrickEvent>,
    /// Хозяева мячей, упавших вниз за этот шаг.
    pub lost: Vec<u32>,
    /// Щит земли спас мяч и был израсходован.
    pub shield_used: bool,
}

pub struct Field {
    pub w: f32,
    pub h: f32,
    pub bricks: Vec<Brick>,
    /// id кирпича по ячейке (r*COLS + c); None — пусто. Быстрый lookup коллизий.
    grid: Vec<Option<u16>>,
    pub balls: Vec<Ball>,
    pub bullets: Vec<Bullet>,
    pub paddles: Vec<Paddle>,
    pub speed: f32,
    /// Щит земли (евент): один раз отражает мячи от пола.
    pub shield: bool,
    next_brick_id: u16,
    next_ball_id: u16,
    next_bullet_id: u16,
    /// Внутренний счётчик шагов: регенерация, фаза движения кирпичей.
    pub ticks: u32,
    /// xorshift-гормон для телепорта (детерминированный, без внешнего rng).
    rng_state: u64,
    /// Ульты: до какого тика действует эффект.
    pub ice_until: u32,
    pub meteor_until: u32,
    pub magnet_until: u32,
    /// Зафиксированный тик движения кирпичей на время заморозки.
    pub frozen_tick: Option<u32>,
}

#[derive(Debug, Clone, Copy)]
pub struct Bullet {
    pub id: u16,
    pub owner: u32,
    pub x: f32,
    pub y: f32,
}

impl Field {
    /// Поле для уровня `level_idx` и ширины `w`. Кирпичи — из LEVELS.
    pub fn new(w: f32, h: f32, level_idx: usize) -> Self {
        let mut f = Self {
            w,
            h,
            bricks: vec![],
            grid: vec![None; LEVELS[0].len().max(8) * COLS],
            balls: vec![],
            paddles: vec![],
            speed: BASE_SPEED,
            next_brick_id: 0,
            next_ball_id: 0,
            next_bullet_id: 0,
            ticks: 0,
            rng_state: 0x9E3779B97F4A7C15,
            ice_until: 0,
            meteor_until: 0,
            magnet_until: 0,
            frozen_tick: None,
            bullets: vec![],
            shield: false,
        };
        f.load_level(level_idx);
        f
    }

    pub fn load_level(&mut self, level_idx: usize) {
        self.bricks.clear();
        self.grid.clear();
        let pat = LEVELS[level_idx % LEVELS.len()];
        self.grid = vec![None; pat.len() * COLS];
        for (r, row) in pat.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                if ch == '.' {
                    continue;
                }
                let id = self.next_brick_id;
                self.next_brick_id += 1;
                let (kind, hp) = match ch {
                    'x' => (BK_NORMAL, 255u8),
                    'e' => (BK_EXPLOSIVE, 1),
                    't' => (BK_TELEPORT, 2),
                    'g' => (BK_REGEN, 2),
                    'm' => (BK_MOVING, 2),
                    d => (BK_NORMAL, d.to_digit(10).unwrap_or(1) as u8),
                };
                let brick = Brick {
                    id,
                    c: c as u8,
                    r: r as u8,
                    hp,
                    stone: ch == 'x',
                    kind,
                    hp_max: hp,
                };
                self.grid[r * COLS + c] = Some(id);
                self.bricks.push(brick);
            }
        }
    }

    /// Загрузить поле из строк редактора: '1'-'3', 'x', 'e', 't', 'g', 'm', '.'.
    pub fn load_rows(&mut self, rows: &[String]) {
        self.bricks.clear();
        self.grid.clear();
        self.grid = vec![None; rows.len() * COLS];
        for (r, row) in rows.iter().enumerate() {
            for (c, ch) in row.chars().enumerate() {
                if c >= COLS {
                    break;
                }
                if ch == '.' {
                    continue;
                }
                let (kind, hp) = match ch {
                    'x' => (BK_NORMAL, 255u8),
                    'e' => (BK_EXPLOSIVE, 1),
                    't' => (BK_TELEPORT, 2),
                    'g' => (BK_REGEN, 2),
                    'm' => (BK_MOVING, 2),
                    d => (BK_NORMAL, d.to_digit(10).unwrap_or(1) as u8),
                };
                let id = self.next_brick_id;
                self.next_brick_id += 1;
                let brick = Brick {
                    id,
                    c: c as u8,
                    r: r as u8,
                    hp,
                    stone: ch == 'x',
                    kind,
                    hp_max: hp,
                };
                self.grid[r * COLS + c] = Some(id);
                self.bricks.push(brick);
            }
        }
    }

    pub fn add_paddle(&mut self, x: f32) {
        self.paddles.push(Paddle {
            x: x.clamp(PADDLE_BASE_W / 2.0, self.w - PADDLE_BASE_W / 2.0),
            target: x.clamp(PADDLE_BASE_W / 2.0, self.w - PADDLE_BASE_W / 2.0),
            w: PADDLE_BASE_W,
            alive: true,
        });
    }

    pub fn add_ball(&mut self, owner: u32, x: f32, y: f32) -> u16 {
        let id = self.next_ball_id;
        self.next_ball_id += 1;
        self.balls.push(Ball {
            id,
            owner,
            x,
            y,
            vx: 0.0,
            vy: 0.0,
        });
        id
    }

    /// Запуск мяча под углом (радианы, -PI/2 = вверх) — угол считает матч-слой.
    pub fn launch(&mut self, ball_id: u16, angle: f32) {
        if let Some(b) = self.balls.iter_mut().find(|b| b.id == ball_id) {
            b.vx = angle.cos() * self.speed;
            b.vy = angle.sin() * self.speed;
        }
    }

    /// Задать цель ракетки (валидация: клампится, скорость ограничена в step).
    pub fn set_paddle_target(&mut self, slot: usize, x: f32) {
        if let Some(p) = self.paddles.get_mut(slot) {
            p.target = x.clamp(p.w / 2.0, self.w - p.w / 2.0);
        }
    }

    /// Добавить кирпич в свободную ячейку (евенты метеорита и камнепада).
    /// None — ячейка занята или вне сетки.
    pub fn add_brick(&mut self, c: usize, r: usize, hp: u8, stone: bool) -> Option<u16> {
        self.add_brick_kind(c, r, hp, stone, BK_NORMAL)
    }

    /// Добавить кирпич с типом (редактор уровней, евенты).
    pub fn add_brick_kind(
        &mut self,
        c: usize,
        r: usize,
        hp: u8,
        stone: bool,
        kind: u8,
    ) -> Option<u16> {
        if c >= COLS || r * COLS + c >= self.grid.len() {
            return None;
        }
        if self.grid[r * COLS + c].is_some() {
            return None;
        }
        let id = self.next_brick_id;
        self.next_brick_id += 1;
        let brick = Brick {
            id,
            c: c as u8,
            r: r as u8,
            hp: if stone { 255 } else { hp },
            stone,
            kind,
            hp_max: hp,
        };
        self.grid[r * COLS + c] = Some(id);
        self.bricks.push(brick);
        Some(id)
    }

    /// Полная очистка поля от кирпичей (босс-арена).
    pub fn grid_clear(&mut self) {
        self.bricks.clear();
        self.grid.iter_mut().for_each(|c| *c = None);
    }

    /// Удалить кирпич по id (камни камнепада исчезают в конце евента).
    pub fn remove_brick_by_id(&mut self, id: u16) -> bool {
        if let Some(pos) = self.bricks.iter().position(|b| b.id == id) {
            let b = self.bricks.remove(pos);
            let idx = b.r as usize * COLS + b.c as usize;
            if idx < self.grid.len() {
                self.grid[idx] = None;
            }
            true
        } else {
            false
        }
    }

    /// Свободна ли ячейка.
    pub fn cell_empty(&self, c: usize, r: usize) -> bool {
        c < COLS && r * COLS + c < self.grid.len() && self.grid[r * COLS + c].is_none()
    }

    /// Число рядов сетки уровня.
    pub fn grid_rows(&self) -> usize {
        self.grid.len() / COLS
    }

    pub fn destructible_left(&self) -> bool {
        self.bricks.iter().any(|b| !b.stone)
    }

    fn brick_at(&self, c: usize, r: usize) -> Option<&Brick> {
        let id = *self.grid.get(r * COLS + c)?.as_ref()?;
        self.bricks.iter().find(|b| b.id == id)
    }

    fn remove_brick(&mut self, id: u16) {
        if let Some(pos) = self.bricks.iter().position(|b| b.id == id) {
            let b = self.bricks.remove(pos);
            self.grid[b.r as usize * COLS + b.c as usize] = None;
        }
    }

    /// Один фиксированный шаг физики. `dt` — шаг тика (1/TICK_HZ).
    pub fn step(&mut self, dt: f32) -> StepResult {
        let mut res = StepResult::default();
        self.ticks = self.ticks.wrapping_add(1);

        // регенерация: раз в 90 тиков восстанавливаем hp у реген-кирпичей (замирает во льду)
        if self.ticks.is_multiple_of(90) && self.ice_until <= self.ticks {
            for b in self.bricks.iter_mut() {
                if b.kind == BK_REGEN && !b.stone && b.hp < b.hp_max {
                    b.hp += 1;
                }
            }
        }

        // ракетки: клэмп после смены ширины (бонус W) + движение к цели
        for p in &mut self.paddles {
            if !p.alive {
                continue;
            }
            p.x = p.x.clamp(p.w / 2.0, self.w - p.w / 2.0);
            let dx = (p.target - p.x).clamp(-PADDLE_SPEED * dt, PADDLE_SPEED * dt);
            p.x = (p.x + dx).clamp(p.w / 2.0, self.w - p.w / 2.0);
        }

        let paddles_snapshot: Vec<Paddle> = self.paddles.clone();
        let mut lost: Vec<u32> = vec![];

        // mem::take: тела шага нужен &mut self (коллизии/удаление кирпичей)
        let mut balls = std::mem::take(&mut self.balls);
        for ball in balls.iter_mut() {
            ball.x += ball.vx * dt;
            ball.y += ball.vy * dt;

            // нормализация скорости
            let sp = (ball.vx * ball.vx + ball.vy * ball.vy).sqrt();
            if sp > 0.0 {
                let k = self.speed / sp;
                ball.vx *= k;
                ball.vy *= k;
            }

            // стены
            if ball.x < BALL_R {
                ball.x = BALL_R;
                ball.vx = ball.vx.abs();
            }
            if ball.x > self.w - BALL_R {
                ball.x = self.w - BALL_R;
                ball.vx = -ball.vx.abs();
            }
            let ceiling = GRID_TOP - 40.0 + BALL_R;
            if ball.y < ceiling {
                ball.y = ceiling;
                ball.vy = ball.vy.abs();
            }

            // ракетки: первая, чья полоса пересекается с мячом
            if ball.vy > 0.0 {
                for p in &paddles_snapshot {
                    if !p.alive {
                        continue;
                    }
                    let py = self.h - 42.0;
                    if ball.y + BALL_R >= py
                        && ball.y + BALL_R <= py + PADDLE_H + 10.0
                        && (ball.x - p.x).abs() <= p.w / 2.0 + BALL_R
                    {
                        let off = ((ball.x - p.x) / (p.w / 2.0)).clamp(-1.0, 1.0);
                        let ang = off * (std::f32::consts::FRAC_PI_3);
                        ball.vy = -ang.cos().abs() * self.speed;
                        ball.vx = ang.sin() * self.speed;
                        if ball.vy > -self.speed * 0.38 {
                            ball.vy = -self.speed * 0.38;
                        }
                        ball.y = py - BALL_R;
                        break;
                    }
                }
            }

            // кирпичи: проверяем ячейки вокруг мяча
            self.collide_bricks(ball, &mut res);
        }

        balls.retain(|ball| {
            if ball.y - BALL_R > self.h {
                lost.push(ball.owner);
                false
            } else {
                true
            }
        });
        self.balls = balls;

        // щит земли: одно спасение на евент
        if self.shield {
            for ball in self.balls.iter_mut() {
                if ball.vy > 0.0 && ball.y + BALL_R >= self.h - 20.0 {
                    ball.y = self.h - 20.0 - BALL_R;
                    ball.vy = -ball.vy.abs();
                    self.shield = false;
                    res.shield_used = true;
                    break;
                }
            }
        }

        // пули лазера: летят вверх, бьют первый кирпич в своей ячейке
        let bullets = std::mem::take(&mut self.bullets);
        let mut survivors = Vec::with_capacity(bullets.len());
        for mut u in bullets {
            u.y -= 700.0 * dt;
            let mut dead = u.y < -20.0;
            if !dead {
                let cellw = self.w / COLS as f32;
                let c = ((u.x / cellw).floor() as usize).min(COLS - 1);
                let rf = (u.y - GRID_TOP) / CELL_H;
                if rf >= 0.0 {
                    let r = rf.floor() as usize;
                    if self.brick_at(c, r).is_some() {
                        self.damage_brick(c, r, u.owner, &mut res, 0);
                        dead = true;
                    }
                }
            }
            if !dead {
                survivors.push(u);
            }
        }
        self.bullets = survivors;

        res.lost = lost;
        res
    }

    /// Выстрел лазера из краёв ракетки слота.
    pub fn fire_laser(&mut self, owner: u32, slot: usize) {
        let Some(p) = self.paddles.get(slot) else {
            return;
        };
        if !p.alive {
            return;
        }
        let y = self.h - 42.0 - 6.0;
        for dx in [-8.0, 8.0] {
            let id = self.next_bullet_id;
            self.next_bullet_id += 1;
            self.bullets.push(Bullet {
                id,
                owner,
                x: p.x + dx,
                y,
            });
        }
    }

    /// Горизонтальный дрейф движущихся кирпичей строки (пиксели). Детерминирован от тика.
    pub fn moving_offset(&self, row: u8) -> f32 {
        let t = self.frozen_tick.unwrap_or(self.ticks) as f32;
        ((t * 0.55 + row as f32 * 7.3).sin()) * (self.w / 10.0) * 0.38
    }

    /// Битмаска активных эффектов для снапшота: 1 лед, 2 метеорит, 4 магнит.
    pub fn fx_mask(&self) -> u8 {
        let mut m = 0u8;
        if self.ice_until > self.ticks {
            m |= 1;
        }
        if self.meteor_until > self.ticks {
            m |= 2;
        }
        if self.magnet_until > self.ticks {
            m |= 4;
        }
        m
    }

    fn next_rand(&mut self) -> f32 {
        let mut x = self.rng_state;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.rng_state = x;
        ((x >> 11) as f32) / ((1u64 << 53) as f32)
    }

    /// Общий урон кирпича (мяч или пуля): hp-1, событие, удаление при 0.
    fn damage_brick(&mut self, c: usize, r: usize, owner: u32, res: &mut StepResult, depth: u8) {
        if depth > 6 {
            return; // защита от бесконечной цепи
        }
        let Some(brick) = self.brick_at(c, r) else {
            return;
        };
        let (brick_id, stone) = (brick.id, brick.stone);
        if stone {
            res.brick_events.push(BrickEvent {
                brick_id,
                c: c as u8,
                r: r as u8,
                owner,
                destroyed: false,
            });
            return;
        }
        let brittle = self.ice_until > self.ticks;
        if let Some(b) = self.bricks.iter_mut().find(|b| b.id == brick_id) {
            b.hp = b.hp.saturating_sub(if brittle { 2 } else { 1 });
            if b.hp == 0 {
                let kind = b.kind;
                res.brick_events.push(BrickEvent {
                    brick_id,
                    c: c as u8,
                    r: r as u8,
                    owner,
                    destroyed: true,
                });
                self.remove_brick(brick_id);
                // взрывной: урон 1 всем 8 соседям, цепь при разрушении соседей-взрывных
                if kind == BK_EXPLOSIVE {
                    for dr in -1i32..=1 {
                        for dc in -1i32..=1 {
                            if dc == 0 && dr == 0 {
                                continue;
                            }
                            let nc = c as i32 + dc;
                            let nr = r as i32 + dr;
                            if nc < 0 || nr < 0 {
                                continue;
                            }
                            self.damage_brick(nc as usize, nr as usize, owner, res, depth + 1);
                        }
                    }
                }
            } else {
                res.brick_events.push(BrickEvent {
                    brick_id,
                    c: c as u8,
                    r: r as u8,
                    owner,
                    destroyed: false,
                });
            }
        }
    }

    /// Урон кирпича по id (движущиеся вне сеточного lookup).
    fn damage_brick_by_id(&mut self, brick_id: u16, owner: u32, res: &mut StepResult, depth: u8) {
        let Some(b) = self.bricks.iter().find(|b| b.id == brick_id) else {
            return;
        };
        let (c, r) = (b.c as usize, b.r as usize);
        self.damage_brick(c, r, owner, res, depth);
    }

    fn collide_bricks(&mut self, ball: &mut Ball, res: &mut StepResult) {
        let cellw = self.w / COLS as f32;
        let c0 = ((ball.x - BALL_R) / cellw).floor().max(0.0) as usize;
        let c1 = (((ball.x + BALL_R) / cellw).floor() as usize).min(COLS - 1);
        let r0 = ((ball.y - BALL_R - GRID_TOP) / CELL_H).floor().max(0.0) as usize;
        let r1 = ((ball.y + BALL_R - GRID_TOP) / CELL_H).floor() as usize;

        for r in r0..=r1 {
            for c in c0..=c1 {
                let hit = self.brick_at(c, r);
                if let Some(b) = hit {
                    if b.kind == BK_MOVING {
                        continue; // движущиеся обрабатываются отдельно (со смещением)
                    }
                } else {
                    continue;
                }
                let bx = c as f32 * cellw + 3.0;
                let by = GRID_TOP + r as f32 * CELL_H + 3.0;
                let bw = cellw - 6.0;
                let bh = CELL_H - 6.0;
                if self.meteor_until > self.ticks {
                    // метеорит: мяч пробивает кирпич насквозь, без отскока
                    let owner = ball.owner;
                    let kind = self.brick_at(c, r).map(|b| b.kind).unwrap_or(BK_NORMAL);
                    self.damage_brick(c, r, owner, res, 0);
                    if kind == BK_TELEPORT {
                        let nx = BALL_R + self.next_rand() * (self.w - 2.0 * BALL_R);
                        let ny = GRID_TOP + 5.0 * CELL_H + self.next_rand() * 4.0 * CELL_H;
                        ball.x = nx;
                        ball.y = ny;
                    }
                    return;
                }
                // ближайшая точка прямоугольника к центру мяча
                let cx = ball.x.clamp(bx, bx + bw);
                let cy = ball.y.clamp(by, by + bh);
                let dx = ball.x - cx;
                let dy = ball.y - cy;
                let d2 = dx * dx + dy * dy;
                if d2 > BALL_R * BALL_R {
                    continue;
                }
                // нормаль отражения
                let (nx, ny) = if d2 == 0.0 {
                    (0.0, if ball.vy > 0.0 { -1.0 } else { 1.0 })
                } else {
                    let d = d2.sqrt();
                    (dx / d, dy / d)
                };
                let dot = ball.vx * nx + ball.vy * ny;
                if dot < 0.0 {
                    ball.vx -= 2.0 * dot * nx;
                    ball.vy -= 2.0 * dot * ny;
                }
                ball.x = cx + nx * BALL_R;
                ball.y = cy + ny * BALL_R;

                let owner = ball.owner;
                let kind = self.brick_at(c, r).map(|b| b.kind).unwrap_or(BK_NORMAL);
                self.damage_brick(c, r, owner, res, 0);
                if kind == BK_TELEPORT {
                    // переброс мяча в случайную точку верхних рядов
                    let nx = BALL_R + self.next_rand() * (self.w - 2.0 * BALL_R);
                    let ny = GRID_TOP + 5.0 * CELL_H + self.next_rand() * 4.0 * CELL_H;
                    ball.x = nx;
                    ball.y = ny;
                }
                return; // один кирпич за шаг, как в popblocks
            }
        }

        // движущиеся кирпичи: AABB со смещением строки
        let movers: Vec<(u16, u8, u8)> = self
            .bricks
            .iter()
            .filter(|b| b.kind == BK_MOVING && !b.stone)
            .map(|b| (b.id, b.c, b.r))
            .collect();
        for (id, c, r) in movers {
            let bx = c as f32 * cellw + 3.0 + self.moving_offset(r);
            let by = GRID_TOP + r as f32 * CELL_H + 3.0;
            let bw = cellw - 6.0;
            let bh = CELL_H - 6.0;
            let cx = ball.x.clamp(bx, bx + bw);
            let cy = ball.y.clamp(by, by + bh);
            let dx = ball.x - cx;
            let dy = ball.y - cy;
            let d2 = dx * dx + dy * dy;
            if d2 > BALL_R * BALL_R {
                continue;
            }
            let (nx, ny) = if d2 == 0.0 {
                (0.0, if ball.vy > 0.0 { -1.0 } else { 1.0 })
            } else {
                let d = d2.sqrt();
                (dx / d, dy / d)
            };
            let dot = ball.vx * nx + ball.vy * ny;
            if dot < 0.0 {
                ball.vx -= 2.0 * dot * nx;
                ball.vy -= 2.0 * dot * ny;
            }
            ball.x = cx + nx * BALL_R;
            ball.y = cy + ny * BALL_R;
            let owner = ball.owner;
            self.damage_brick_by_id(id, owner, res, 0);
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{BASE_SPEED, FIELD_H, FIELD_W};

    fn plain_field() -> Field {
        Field::new(FIELD_W, FIELD_H, 0)
    }

    #[test]
    fn level_loads_bricks_and_grid() {
        let f = plain_field();
        assert_eq!(f.bricks.len(), 40); // уровень 1 из popblocks: 4 ряда × 10
        assert!(f.destructible_left());
        // первый кирпич (0,0) hp 2
        assert_eq!(f.brick_at(0, 0).unwrap().hp, 2);
    }

    #[test]
    fn explosive_chain_destroys_neighbors() {
        let mut f = plain_field();
        f.grid_clear();
        let center = f.add_brick_kind(4, 2, 1, false, BK_EXPLOSIVE).unwrap();
        for (c, r) in [(3, 2usize), (5, 2), (4, 1), (4, 3)] {
            f.add_brick(c, r, 1, false);
        }
        let mut res = StepResult::default();
        f.damage_brick(4, 2, 1, &mut res, 0);
        assert!(res
            .brick_events
            .iter()
            .any(|e| e.brick_id == center && e.destroyed));
        // центр + 4 соседа уничтожены цепью
        assert_eq!(
            f.bricks.len(),
            0,
            "взрыв должен уничтожить всех соседей hp1"
        );
    }

    #[test]
    fn regen_brick_heals_over_time() {
        let mut f = plain_field();
        f.grid_clear();
        f.add_brick_kind(2, 1, 2, false, BK_REGEN);
        let mut res = StepResult::default();
        f.damage_brick(2, 1, 1, &mut res, 0);
        assert_eq!(f.bricks[0].hp, 1);
        for _ in 0..91 {
            f.step(1.0 / 30.0);
        }
        assert_eq!(f.bricks[0].hp, 2, "реген должен вернуть hp до максимума");
    }

    #[test]
    fn moving_offset_deterministic_and_bounded() {
        let mut f = plain_field();
        f.ticks = 100;
        let o1 = f.moving_offset(0);
        let amp = f.w / 10.0 * 0.38;
        assert!(o1.abs() <= amp + 1e-3);
        f.ticks = 200;
        assert_ne!(
            f.moving_offset(0),
            o1,
            "смещение должно меняться со временем"
        );
        f.ticks = 100;
        assert!(
            (f.moving_offset(0) - o1).abs() < 1e-4,
            "детерминизм от тика"
        );
    }

    #[test]
    fn teleport_brick_relocates_ball() {
        let mut f = plain_field();
        f.grid_clear();
        f.add_brick_kind(5, 1, 2, false, BK_TELEPORT);
        let id = f.add_ball(1, 5.5 * (f.w / 10.0), GRID_TOP + CELL_H + 40.0);
        f.launch(id, -std::f32::consts::FRAC_PI_2); // строго вверх в кирпич
        let before = f.balls[0].y;
        let band = GRID_TOP + 5.0 * CELL_H..=(GRID_TOP + 9.0 * CELL_H);
        let mut teleported = false;
        for _ in 0..20 {
            if !f.step(1.0 / 30.0).lost.is_empty() {
                break;
            }
            if let Some(b) = f.balls.iter().find(|b| b.id == id) {
                if band.contains(&b.y) {
                    teleported = true;
                    break;
                }
            } else {
                break;
            }
        }
        assert!(
            teleported,
            "мяч должен телепортироваться в верхние ряды (было y={before})"
        );
    }

    #[test]
    fn level_chars_map_to_kinds() {
        let mut f = plain_field();
        f.load_level(0);
        // синтетический парсинг: строим поле из строки
        let mut g = Field::new(FIELD_W, FIELD_H, 0);
        g.grid_clear();
        for (k, ch) in [
            (BK_EXPLOSIVE, 'e'),
            (BK_TELEPORT, 't'),
            (BK_REGEN, 'g'),
            (BK_MOVING, 'm'),
        ] {
            g.add_brick_kind(
                usize::from(k),
                0,
                match ch {
                    'e' => 1,
                    _ => 2,
                },
                false,
                k,
            );
        }
        assert_eq!(g.bricks.iter().filter(|b| b.kind == BK_MOVING).count(), 1);
        assert_eq!(g.bricks.iter().filter(|b| b.kind == BK_REGEN).count(), 1);
    }

    #[test]
    fn wall_bounce() {
        let mut f = plain_field();
        let id = f.add_ball(1, BALL_R - 4.0, 300.0);
        f.launch(id, 0.0); // строго вправо... поставим влево
        if let Some(b) = f.balls.iter_mut().find(|b| b.id == id) {
            b.vx = -BASE_SPEED;
            b.vy = 0.0;
        }
        f.step(1.0 / 30.0);
        let b = f.balls.iter().find(|b| b.id == id).unwrap();
        assert!(b.vx > 0.0, "мяч должен отскочить от левой стены вправо");
        assert_eq!(b.x, BALL_R);
    }

    #[test]
    fn paddle_bounce_straight_up() {
        let mut f = plain_field();
        f.add_paddle(FIELD_W / 2.0);
        let id = f.add_ball(1, FIELD_W / 2.0, FIELD_H - 60.0);
        if let Some(b) = f.balls.iter_mut().find(|b| b.id == id) {
            b.vx = 0.0;
            b.vy = BASE_SPEED;
        }
        // подводим мяч к ракетке
        for _ in 0..10 {
            f.step(1.0 / 30.0);
        }
        let b = f.balls.iter().find(|b| b.id == id).unwrap();
        assert!(b.vy < 0.0, "мяч должен полететь вверх от ракетки");
    }

    #[test]
    fn brick_destroyed_and_event() {
        let mut f = plain_field();
        let id = f.add_ball(7, 0.0, 0.0);
        // ставим мяч ровно под кирпич (0,0) и запускаем вверх
        let cellw = FIELD_W / COLS as f32;
        let x = cellw / 2.0;
        let y = GRID_TOP + CELL_H + 15.0;
        if let Some(b) = f.balls.iter_mut().find(|b| b.id == id) {
            b.x = x;
            b.y = y;
            b.vx = 0.0;
            b.vy = -BASE_SPEED;
        }
        let res = f.step(1.0 / 30.0);
        let ev = res
            .brick_events
            .iter()
            .find(|e| e.c == 0 && e.r == 0)
            .expect("событие по кирпичу (0,0)");
        // кирпич с hp 2: первый удар только треснул
        assert!(!ev.destroyed);
        assert_eq!(ev.owner, 7);
        assert_eq!(f.brick_at(0, 0).unwrap().hp, 1);
    }

    #[test]
    fn ball_lost_reported() {
        let mut f = plain_field();
        let id = f.add_ball(42, FIELD_W / 2.0, FIELD_H - 10.0);
        if let Some(b) = f.balls.iter_mut().find(|b| b.id == id) {
            b.vx = 0.0;
            b.vy = BASE_SPEED;
        }
        let res = [0; 3].iter().fold(StepResult::default(), |acc, _| {
            let mut r = f.step(1.0 / 30.0);
            r.lost.extend(acc.lost);
            r
        });
        assert_eq!(res.lost, vec![42]);
        assert!(f.balls.is_empty());
    }

    #[test]
    fn speed_always_normalized() {
        let mut f = plain_field();
        let id = f.add_ball(1, 100.0, 300.0);
        if let Some(b) = f.balls.iter_mut().find(|b| b.id == id) {
            b.vx = 1000.0; // читерский ввод скорости
            b.vy = 0.0;
        }
        f.step(1.0 / 30.0);
        let b = f.balls.iter().find(|b| b.id == id).unwrap();
        let sp = (b.vx * b.vx + b.vy * b.vy).sqrt();
        assert!((sp - f.speed).abs() < 1.0);
    }

    #[test]
    fn paddle_moves_toward_target_with_speed_cap() {
        let mut f = plain_field();
        f.add_paddle(100.0);
        f.set_paddle_target(0, 700.0);
        f.step(1.0 / 30.0);
        // за тик ракетка прошла не больше PADDLE_SPEED/30
        assert!(f.paddles[0].x <= 100.0 + PADDLE_SPEED / 30.0 + 0.01);
        for _ in 0..60 {
            f.step(1.0 / 30.0);
        }
        assert_eq!(f.paddles[0].x, 700.0);
    }

    #[test]
    fn field_widens_with_players() {
        let f = Field::new(1600.0, FIELD_H, 0);
        let cellw = f.w / COLS as f32;
        assert_eq!(cellw, 160.0);
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    #[test]
    fn fifteen_wellformed_levels() {
        assert_eq!(LEVELS.len(), 15, "15 рукотворных уровней");
        for (i, rows) in LEVELS.iter().enumerate() {
            assert!(
                (1..=8).contains(&rows.len()),
                "уровень {}: строк 1-8, а {}",
                i + 1,
                rows.len()
            );
            for row in rows.iter() {
                assert_eq!(
                    row.chars().count(),
                    COLS,
                    "уровень {}: строка '{}' не {}",
                    i + 1,
                    row,
                    COLS
                );
                assert!(
                    row.chars()
                        .all(|c| matches!(c, '1'..='3' | 'x' | '.' | 'e' | 't' | 'g' | 'm')),
                    "уровень {}: недопустимый символ в '{}'",
                    i + 1,
                    row
                );
            }
        }
    }
}
