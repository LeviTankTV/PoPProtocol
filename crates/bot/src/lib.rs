//! ИИ ракеток-ботов PoP - Protocol (шаг 11 дорожной карты: три уровня сложности;
//! на шаге 10 — тот же ИИ подхватывает АФК-ракетки).

use popproto_core::BotLevel;

/// Максимальная скорость ракетки бота, единиц поля/сек.
pub fn paddle_speed(level: BotLevel) -> f32 {
    match level {
        BotLevel::Lazy => 300.0,
        BotLevel::Normal => 560.0,
        BotLevel::Pro => 780.0,
    }
}

/// Срез состояния мяча, нужный боту для прогноза.
#[derive(Debug, Clone, Copy)]
pub struct BallView {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
}

/// Состояние одного бота: уровень, задержка реакции, шум прицела.
#[derive(Debug, Clone, Copy)]
pub struct BotBrain {
    pub level: BotLevel,
    /// Сколько тиков назад бот видит мяч (задержка реакции).
    pub reaction_ticks: u8,
    /// Разброс целевой точки; ослабляет игру и делает ботов «живыми».
    pub aim_noise: f32,
    /// Насколько сильно бот смещает точку встречи, чтобы направить отскок
    /// в сторону кирпичей (управление отскоком краем ракетки).
    pub steer_px: f32,
}

impl BotBrain {
    pub fn new(level: BotLevel) -> Self {
        let (reaction_ticks, aim_noise, steer_px) = match level {
            BotLevel::Lazy => (9, 90.0, 0.0),
            BotLevel::Normal => (4, 30.0, 16.0),
            BotLevel::Pro => (1, 4.0, 34.0),
        };
        Self {
            level,
            reaction_ticks,
            aim_noise,
            steer_px,
        }
    }

    /// В какую сторону боту выгоднее отвести мяч: знак = направление к кирпичам.
    /// `bias` в -1..1 (отрицательный — кирпичи слева от точки падения).
    pub fn steer_bias(&self, bias: f32) -> f32 {
        self.steer_px * bias.clamp(-1.0, 1.0)
    }

    /// Прогноз X падения мяча на линию ракетки с отражениями от стен.
    /// Нисходящий мяч (vy > 0): летим в точку падения, а не за хвостом.
    fn predict_landing(ball: &BallView, field_w: f32, paddle_y: f32) -> f32 {
        if ball.vy <= 1.0 {
            return ball.x;
        }
        let t = (paddle_y - ball.y) / ball.vy;
        if t <= 0.0 {
            return ball.x;
        }
        let mut x = ball.x + ball.vx * t;
        let (lo, hi) = (9.0, field_w - 9.0);
        for _ in 0..4 {
            if x < lo {
                x = lo + (lo - x);
            } else if x > hi {
                x = hi - (x - hi);
            } else {
                break;
            }
        }
        x.clamp(lo, hi)
    }

    /// Целевая X ракетки. `delayed_x` — позиция мяча reaction_ticks назад
    /// (задержка реакции), скорость — текущая (упреждение по траектории).
    pub fn target_x(
        &self,
        delayed_x: f32,
        ball: &BallView,
        field_w: f32,
        paddle_y: f32,
        brick_bias: f32,
        rng: &mut impl FnMut() -> f32,
    ) -> f32 {
        let descending = ball.vy > 0.0;
        let base = match self.level {
            // лентяй просто бежит за мячом, куда бы тот ни летел
            BotLevel::Lazy => delayed_x,
            // обычный и профи встают в точку падения; обычный промахивается сильнее
            BotLevel::Normal | BotLevel::Pro => {
                if descending {
                    let view = BallView {
                        x: delayed_x,
                        ..*ball
                    };
                    // управление отскоком: чуть смещаемся, чтобы мяч ушёл к кирпичам
                    Self::predict_landing(&view, field_w, paddle_y) + self.steer_bias(brick_bias)
                } else {
                    // мяч летит вверх — можно сместиться к центру, но не далеко
                    delayed_x * 0.75 + field_w / 2.0 * 0.25
                }
            }
        };
        let noise = (rng() * 2.0 - 1.0) * self.aim_noise;
        (base + noise).clamp(0.0, field_w)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Детерминированный «рандом» для тестов: 0.0, 1.0, 0.5, 0.25, 0.0, ...
    fn fixed_rng() -> impl FnMut() -> f32 {
        let seq = [0.0, 1.0, 0.5, 0.25];
        let mut i = 0;
        move || {
            i += 1;
            seq[i % seq.len()]
        }
    }

    #[test]
    fn pro_reacts_faster_and_stricter_than_lazy() {
        let pro = BotBrain::new(BotLevel::Pro);
        let lazy = BotBrain::new(BotLevel::Lazy);
        assert!(pro.reaction_ticks < lazy.reaction_ticks);
        assert!(pro.aim_noise < lazy.aim_noise);
        assert!(paddle_speed(BotLevel::Lazy) < paddle_speed(BotLevel::Normal));
        assert!(paddle_speed(BotLevel::Normal) < paddle_speed(BotLevel::Pro));
    }

    #[test]
    fn target_stays_in_field() {
        let brain = BotBrain::new(BotLevel::Lazy);
        let ball = BallView {
            x: 400.0,
            y: 300.0,
            vx: 100.0,
            vy: 200.0,
        };
        let x = brain.target_x(-500.0, &ball, 800.0, 558.0, 0.0, &mut fixed_rng());
        assert!((0.0..=800.0).contains(&x));
    }

    #[test]
    fn pro_meets_descending_ball_at_landing_point() {
        let brain = BotBrain::new(BotLevel::Pro);
        // мяч летит вправо-вниз: из (200, 100), vx=200, vy=200 -> падение через 2.29 с в x = 200+458
        let ball = BallView {
            x: 200.0,
            y: 100.0,
            vx: 200.0,
            vy: 200.0,
        };
        let x = brain.target_x(200.0, &ball, 800.0, 558.0, 0.0, &mut fixed_rng());
        // прогноз: t=(558-100)/200=2.29, x=200+458=658; steer=0 при bias=0, шум профи мал
        assert!(
            (630.0..=686.0).contains(&x),
            "профи ждёт у точки падения, получил {x}"
        );
    }

    #[test]
    fn steering_pushes_toward_bricks() {
        let brain = BotBrain::new(BotLevel::Pro);
        let ball = BallView {
            x: 400.0,
            y: 100.0,
            vx: 0.0,
            vy: 200.0,
        };
        let plain = brain.target_x(400.0, &ball, 800.0, 558.0, 0.0, &mut fixed_rng());
        let left = brain.target_x(400.0, &ball, 800.0, 558.0, -1.0, &mut fixed_rng());
        let right = brain.target_x(400.0, &ball, 800.0, 558.0, 1.0, &mut fixed_rng());
        assert!(
            left < plain && plain < right,
            "steer смещает точку встречи: {left} {plain} {right}"
        );
    }

    #[test]
    fn prediction_reflects_off_walls() {
        // мяч летит круто вправо: без отражения ушёл бы за поле
        let ball = BallView {
            x: 700.0,
            y: 100.0,
            vx: 600.0,
            vy: 100.0,
        };
        let x = BotBrain::predict_landing(&ball, 800.0, 600.0);
        assert!(
            (0.0..=800.0).contains(&x),
            "прогноз не выходит за поле: {x}"
        );
        assert!(
            x > 400.0,
            "отскок влево должен оставить мяч в правой части: {x}"
        );
    }

    #[test]
    fn ascending_ball_drifts_toward_center() {
        let brain = BotBrain::new(BotLevel::Pro);
        let ball = BallView {
            x: 100.0,
            y: 500.0,
            vx: 0.0,
            vy: -400.0,
        };
        let x = brain.target_x(100.0, &ball, 800.0, 558.0, 0.0, &mut fixed_rng());
        assert!(
            x > 100.0,
            "при мяче вверху ракетка подтягивается к центру: {x}"
        );
    }
}
