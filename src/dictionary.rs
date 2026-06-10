use std::collections::HashMap;

use crate::packed_bits::PackedBits;

#[derive(Debug, Clone)]
struct ActiveBase {
    base: PackedBits,
    previous: Option<u32>,
    next: Option<u32>,
}

#[derive(Debug, Clone)]
pub struct Dictionary {
    active_ids_by_base: HashMap<PackedBits, u32>,
    active_bases_by_id: HashMap<u32, ActiveBase>,
    head: Option<u32>,
    tail: Option<u32>,
    max_active_bases: Option<usize>,
    next_id: u32,
}

impl Dictionary {
    pub fn new(base_byte_len: usize, max_active_dictionary_bytes: Option<usize>) -> Self {
        let max_active_bases = max_active_dictionary_bytes.map(|bytes| {
            let bytes_per_entry = base_byte_len.saturating_add(4);
            bytes / bytes_per_entry
        });
        Self {
            active_ids_by_base: HashMap::new(),
            active_bases_by_id: HashMap::new(),
            head: None,
            tail: None,
            max_active_bases,
            next_id: 0,
        }
    }

    pub fn get_or_insert(&mut self, base: PackedBits) -> (u32, bool) {
        if let Some(&id) = self.active_ids_by_base.get(&base) {
            self.touch(id);
            return (id, false);
        }

        let id = self.next_id;
        self.next_id = self
            .next_id
            .checked_add(1)
            .expect("Titchy dictionary exhausted all u32 IDs");
        self.active_ids_by_base.insert(base.clone(), id);
        self.active_bases_by_id.insert(
            id,
            ActiveBase {
                base,
                previous: None,
                next: None,
            },
        );
        self.attach_front(id);
        self.enforce_cap();
        (id, true)
    }

    pub fn len(&self) -> usize {
        self.next_id as usize
    }

    pub fn max_active_bases(&self) -> Option<usize> {
        self.max_active_bases
    }

    fn touch(&mut self, id: u32) {
        if self.head == Some(id) {
            return;
        }
        self.detach(id);
        self.attach_front(id);
    }

    fn enforce_cap(&mut self) {
        let Some(max_active_bases) = self.max_active_bases else {
            return;
        };
        while self.active_bases_by_id.len() > max_active_bases {
            let Some(evicted) = self.tail else {
                break;
            };
            self.detach(evicted);
            if let Some(entry) = self.active_bases_by_id.remove(&evicted) {
                self.active_ids_by_base.remove(&entry.base);
            }
        }
    }

    fn attach_front(&mut self, id: u32) {
        let old_head = self.head;
        let entry = self
            .active_bases_by_id
            .get_mut(&id)
            .expect("active dictionary ID must exist");
        entry.previous = None;
        entry.next = old_head;

        if let Some(old_head) = old_head {
            self.active_bases_by_id
                .get_mut(&old_head)
                .expect("LRU head must exist")
                .previous = Some(id);
        } else {
            self.tail = Some(id);
        }
        self.head = Some(id);
    }

    fn detach(&mut self, id: u32) {
        let entry = self
            .active_bases_by_id
            .get(&id)
            .expect("active dictionary ID must exist");
        let previous = entry.previous;
        let next = entry.next;

        if let Some(previous) = previous {
            self.active_bases_by_id
                .get_mut(&previous)
                .expect("LRU predecessor must exist")
                .next = next;
        } else {
            self.head = next;
        }
        if let Some(next) = next {
            self.active_bases_by_id
                .get_mut(&next)
                .expect("LRU successor must exist")
                .previous = previous;
        } else {
            self.tail = previous;
        }

        let entry = self
            .active_bases_by_id
            .get_mut(&id)
            .expect("active dictionary ID must exist");
        entry.previous = None;
        entry.next = None;
    }
}

#[cfg(test)]
mod tests {
    use super::Dictionary;
    use crate::packed_bits::PackedBits;

    #[test]
    fn retained_state_is_bounded_while_persistent_ids_keep_growing() {
        let mut dictionary = Dictionary::new(1, Some(5));

        for value in 0..32 {
            let (id, is_new) = dictionary.get_or_insert(PackedBits::from_bytes(vec![value], 8));
            assert_eq!(id, value as u32);
            assert!(is_new);
        }

        assert_eq!(dictionary.len(), 32);
        assert_eq!(dictionary.active_ids_by_base.len(), 1);
        assert_eq!(dictionary.active_bases_by_id.len(), 1);
    }
}
