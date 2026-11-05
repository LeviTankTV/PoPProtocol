//! Процедурный генератор уровней PoP - Protocol (для режима «Волна»).
//! Детерминирован по сиду: одна и та же комната даёт одни и те же волны.

pub const COLS: usize = 10;

/// Один шаг xorshift64.
fn xorshift(state: &mut u64) -> u64 {
    *state ^= *state << 13;
    *state ^= *state >> 7;
    *state ^= *state << 17;
    *state
}

/// Строка паттерна из 10 ячеек: 1..3 — hp, 'x' — камень (~3%), '.' — пусто (~12%).
pub fn generate_row(state: &mut u64) -> String {
    let mut row = String::with_capacity(COLS);
    for c in 0..COLS {
        let v = (xorshift(state) >> (c * 3 % 24)) & 0xFF;
        let ch = match v % 100 {
            0..=49 => '1',
            50..=74 => '2',
            75..=87 => '3',
            88..=99 => '.',
            _ => 'x',
        };
        row.push(ch);
    }
    row
}

/// Паттерн из `rows` строк, сид — обычно id комнаты.
pub fn generate_pattern(seed: u64, rows: usize) -> Vec<String> {
    let mut state = seed | 1;
    (0..rows).map(|_| generate_row(&mut state)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deterministic_by_seed() {
        let a = generate_pattern(42, 6);
        let b = generate_pattern(42, 6);
        assert_eq!(a, b, "один сид — один паттерн");
        assert_eq!(a.len(), 6);
        for row in &a {
            assert_eq!(row.chars().count(), COLS);
            assert!(row.chars().all(|ch| "123.x".contains(ch)));
        }
    }

    #[test]
    fn different_seeds_differ() {
        assert_ne!(generate_pattern(1, 8), generate_pattern(2, 8));
    }

    #[test]
    fn stones_are_rare() {
        let pat = generate_pattern(7, 40);
        let stones = pat.iter().flat_map(|r| r.chars()).filter(|c| *c == 'x').count();
        let total = 40 * COLS;
        assert!(stones * 20 < total, "камней должно быть ~3%, а не много");
    }
}
