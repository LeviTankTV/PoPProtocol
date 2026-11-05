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
pub const LEVELS: [&[&str]; 5] = [
    &["2222222222", "1111111111", "2111111112", "1111111111"],
    &["1.1.1.1.1.", ".2.2.2.2.2", "1.1.1.1.1.", ".2.2.2.2.2", "1.1.1.1.1."],
    &["....11....", "...1221...", "..122221..", ".12233221.", "1223333221"],
    &["1111111111", "2xx2222xx2", "1111111111", "222xxxxx22", "1111111111"],
    &["3333333333", "3222222223", "3211111123", "3211111123", "3222222223", "3333333333"],
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Brick {
    pub id: u16,
    pub c: u8,
    pub r: u8,
    /// hp 1..=3; stone=true — неразрушимый.
    pub hp: u8,
    pub stone: bool,
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
                let stone = ch == 'x';
                let hp = if stone { 255 } else { ch.to_digit(10).unwrap_or(1) as u8 };
                let brick = Brick { id, c: c as u8, r: r as u8, hp, stone };
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
        self.balls.push(Ball { id, owner, x, y, vx: 0.0, vy: 0.0 });
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
        if c >= COLS || r * COLS + c >= self.grid.len() {
            return None;
        }
        if self.grid[r * COLS + c].is_some() {
            return None;
        }
        let id = self.next_brick_id;
        self.next_brick_id += 1;
        let brick = Brick { id, c: c as u8, r: r as u8, hp: if stone { 255 } else { hp }, stone };
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
                    if ball.y + BALL_R >= py && ball.y + BALL_R <= py + PADDLE_H + 10.0
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
        let mut bullets = std::mem::take(&mut self.bullets);
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
                        self.damage_brick(c, r, u.owner, &mut res);
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
        let Some(p) = self.paddles.get(slot) else { return };
        if !p.alive {
            return;
        }
        let y = self.h - 42.0 - 6.0;
        for dx in [-8.0, 8.0] {
            let id = self.next_bullet_id;
            self.next_bullet_id += 1;
            self.bullets.push(Bullet { id, owner, x: p.x + dx, y });
        }
    }

    /// Общий урон кирпича (мяч или пуля): hp-1, событие, удаление при 0.
    fn damage_brick(&mut self, c: usize, r: usize, owner: u32, res: &mut StepResult) {
        let Some(brick) = self.brick_at(c, r) else { return };
        let (brick_id, stone) = (brick.id, brick.stone);
        if stone {
            res.brick_events.push(BrickEvent { brick_id, c: c as u8, r: r as u8, owner, destroyed: false });
            return;
        }
        if let Some(b) = self.bricks.iter_mut().find(|b| b.id == brick_id) {
            b.hp -= 1;
            if b.hp == 0 {
                res.brick_events.push(BrickEvent { brick_id, c: c as u8, r: r as u8, owner, destroyed: true });
                self.remove_brick(brick_id);
            } else {
                res.brick_events.push(BrickEvent { brick_id, c: c as u8, r: r as u8, owner, destroyed: false });
            }
        }
    }

    fn collide_bricks(&mut self, ball: &mut Ball, res: &mut StepResult) {
        let cellw = self.w / COLS as f32;
        let c0 = ((ball.x - BALL_R) / cellw).floor().max(0.0) as usize;
        let c1 = (((ball.x + BALL_R) / cellw).floor() as usize).min(COLS - 1);
        let r0 = ((ball.y - BALL_R - GRID_TOP) / CELL_H).floor().max(0.0) as usize;
        let r1 = ((ball.y + BALL_R - GRID_TOP) / CELL_H).floor() as usize;

        for r in r0..=r1 {
            for c in c0..=c1 {
                if self.brick_at(c, r).is_none() {
                    continue;
                }
                let bx = c as f32 * cellw + 3.0;
                let by = GRID_TOP + r as f32 * CELL_H + 3.0;
                let bw = cellw - 6.0;
                let bh = CELL_H - 6.0;
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
                self.damage_brick(c, r, owner, res);
                return; // один кирпич за шаг, как в popblocks
            }
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
        let ev = res.brick_events.iter().find(|e| e.c == 0 && e.r == 0).expect("событие по кирпичу (0,0)");
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
