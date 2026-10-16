/// Values in slots that their ids index. An id holds its slot in its low 32
/// bits and the slot's use count in its high ones, so the id of a value that
/// left names none of the values the slot holds later.
pub(super) struct Slots<T> {
    slots: Vec<Slot<T>>,
    free: Vec<u32>,
    len: usize,
}

struct Slot<T> {
    /// How many values the slot took, the one it holds included. It starts
    /// at one, so no id is zero.
    uses: u32,
    value: Option<T>,
}

impl<T> Slots<T> {
    pub(super) const fn new() -> Self {
        Self {
            slots: Vec::new(),
            free: Vec::new(),
            len: 0,
        }
    }

    /// Stores `value` and returns its id.
    pub(super) fn insert(&mut self, value: T) -> u64 {
        let index = match self.free.pop() {
            Some(index) => index,
            None => {
                self.slots.push(Slot {
                    uses: 0,
                    value: None,
                });
                (self.slots.len() - 1) as u32
            }
        };
        let slot = &mut self.slots[index as usize];
        slot.uses = slot.uses.wrapping_add(1).max(1);
        slot.value = Some(value);
        self.len += 1;
        (u64::from(slot.uses) << 32) | u64::from(index)
    }

    fn slot_mut(&mut self, id: u64) -> Option<&mut Slot<T>> {
        let slot = self.slots.get_mut(id as u32 as usize)?;
        (u64::from(slot.uses) == id >> 32).then_some(slot)
    }

    pub(super) fn get_mut(&mut self, id: u64) -> Option<&mut T> {
        self.slot_mut(id)?.value.as_mut()
    }

    pub(super) fn contains(&self, id: u64) -> bool {
        self.slots
            .get(id as u32 as usize)
            .is_some_and(|slot| u64::from(slot.uses) == id >> 32 && slot.value.is_some())
    }

    /// Takes the value `id` names out of its slot, which the next insert
    /// may reuse.
    pub(super) fn remove(&mut self, id: u64) -> Option<T> {
        let value = self.slot_mut(id)?.value.take()?;
        self.free.push(id as u32);
        self.len -= 1;
        Some(value)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (u64, &T)> {
        self.slots.iter().enumerate().filter_map(|(index, slot)| {
            let value = slot.value.as_ref()?;
            Some(((u64::from(slot.uses) << 32) | index as u64, value))
        })
    }

    pub(super) fn len(&self) -> usize {
        self.len
    }

    pub(super) fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub(super) fn capacity(&self) -> usize {
        self.slots.capacity()
    }
}
