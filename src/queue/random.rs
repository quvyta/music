//! A generator of numbers that are not in order, so that a shuffle is told apart from a sort.

/// A generator that gives the same numbers for the same seed: a shuffle can be repeated, and a
/// test can pin the order it expects.
pub(super) struct Random {
    /// The step the next number is made from.
    state: u64,
}

impl Random {
    /// A generator whose numbers begin at `seed`.
    pub(super) fn seeded(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next number.
    fn next(&mut self) -> u64 {
        // The whole state is stepped and then mixed, so two seeds close together do not shuffle
        // two lists the same way.
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut mixed = self.state;
        mixed = (mixed ^ (mixed >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        mixed = (mixed ^ (mixed >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        mixed ^ (mixed >> 31)
    }

    /// A number in `0..bound`, every number as likely as any other.
    pub(super) fn place(&mut self, bound: usize) -> usize {
        debug_assert!(bound > 0, "there is nowhere to choose from");
        // The draws that would land in the last, short run of numbers are taken again, so no place
        // is more likely than another.
        let bound = u64::try_from(bound).unwrap_or(u64::MAX);
        let unbroken = u64::MAX - u64::MAX % bound;
        loop {
            let draw = self.next();
            if draw < unbroken {
                return usize::try_from(draw % bound).unwrap_or(0);
            }
        }
    }
}

/// Puts the given places in a new order, every order as likely as any other. The same seed always
/// gives the same order, so a shuffle the person has seen can be told from a new one.
pub(super) fn shuffle(places: &mut [usize], seed: u64) {
    let mut random = Random::seeded(seed);
    // Walking backwards and swapping each place with one at or below it leaves every place equally
    // likely to be chosen for every one of its positions.
    for from in (1..places.len()).rev() {
        places.swap(from, random.place(from + 1));
    }
}
