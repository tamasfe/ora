pub struct Entry<T, R> {
    pub entry: T,
    pub rest: R,
}

const NUM_SLOTS: usize = 256;

type Entries<T, R> = alloc::vec::Vec<Entry<T, R>>;

#[must_use]
pub struct ByteWheel<T, R> {
    slots: [Option<Entries<T, R>>; NUM_SLOTS],
    count: usize,
    current: u8,
}

impl<T, R> ByteWheel<T, R> {
    const INIT_VALUE: Option<Entries<T, R>> = None;

    pub fn new() -> Self {
        ByteWheel {
            slots: [Self::INIT_VALUE; NUM_SLOTS],
            count: 0,
            current: 0,
        }
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn insert(&mut self, pos: u8, e: T, r: R) {
        let index = pos as usize;
        self.slots[index]
            .get_or_insert_with(Default::default)
            .push(Entry { entry: e, rest: r });
        self.count += 1;
    }

    #[must_use]
    pub const fn len(&self) -> usize {
        self.count
    }

    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.count == 0
    }

    #[must_use]
    pub const fn current(&self) -> u8 {
        self.current
    }

    pub fn set_current(&mut self, current: u8) {
        self.current = current;
    }

    #[tracing::instrument(level = "trace", skip_all)]
    pub fn tick(&mut self) -> (impl Iterator<Item = Entry<T, R>>, u8) {
        self.current = self.current.wrapping_add(1u8);
        let index = self.current as usize;
        let cur = self.slots[index].take();
        if let Some(l) = &cur {
            self.count = self.count.saturating_sub(l.len());
        }
        (cur.unwrap_or_default().into_iter(), self.current)
    }
}

impl<EntryType, RestType> Default for ByteWheel<EntryType, RestType> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn smoke() {
        let mut w: ByteWheel<usize, [u8; 0]> = ByteWheel::new();
        w.insert(2, 0, []);
        assert_eq!(w.current(), 0);
        let (slot, prev_current) = w.tick();
        assert_eq!(slot.count(), 0);
        assert_eq!(prev_current, 1);

        let (mut slot, prev_current) = w.tick();
        assert_eq!(slot.next().unwrap().entry, 0);
        assert_eq!(prev_current, 2);
    }
}
