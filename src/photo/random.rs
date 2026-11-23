use super::*;

impl PhotoList {
    pub fn reset_random_pass(&mut self) {
        self.random_pass = None;
    }

    pub fn pick_random(&mut self, seed: u64) -> bool {
        let choices = self.len().saturating_sub(1);
        if choices == 0 {
            return false;
        }
        let choice = random_index(seed, choices);
        self.index = choice + usize::from(choice >= self.index);
        true
    }

    /// A slide pass samples the remaining rows without reordering the playlist.
    pub fn pick_random_slide(&mut self, seed: u64, repeat: bool) -> bool {
        let remaining = self.prepare_random_pass();
        if remaining.is_empty() {
            if !repeat {
                return false;
            }
            self.pick_random(seed);
            self.reset_random_pass();
            self.prepare_random_pass();
        } else {
            self.index = remaining.swap_remove(random_index(seed, remaining.len()));
        }
        true
    }

    fn prepare_random_pass(&mut self) -> &mut Vec<usize> {
        let count = self.len();
        let current = self.index;
        self.random_pass
            .get_or_insert_with(|| (0..count).filter(|&index| index != current).collect())
    }
}

fn random_index(seed: u64, count: usize) -> usize {
    XorShift64 { state: seed.max(1) }.next_u64() as usize % count
}
