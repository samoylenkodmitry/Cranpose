use cranpose_core::collections::recent_hashes::RecentHashes;

// How many tags one bucket of the set holds. Texts that return in turn and
// share a bucket keep their tags there. In a bucket of one slot each such
// text would push the other's tag out, so neither would ever count as
// returned.
const WAYS: usize = 4;

// The texts changed in place or drawn lately, by hash. A text laid out
// again while its hash is here came back, so the shared cache keeps it. The
// set starts with no room. It doubles its slots up to `most`, the texts kept
// by the cache, whenever the texts put in it during one span of the cache's
// idle passes pass half its slots. It then remembers a text for about as
// long as the cache would have kept it. An app of a few texts keeps a few
// tags. A trim gives the room back.
pub(super) struct ReturningTexts {
    hashes: RecentHashes<WAYS>,
    most: usize,
    // The hashes put since the span began, and the passes since that.
    puts: usize,
    passes: u64,
}

impl ReturningTexts {
    pub(super) const fn new(most: usize) -> Self {
        Self {
            hashes: RecentHashes::new(0),
            most,
            puts: 0,
            passes: 0,
        }
    }

    // Whether `hash` was put lately.
    pub(super) fn contains(&self, hash: u64) -> bool {
        self.hashes.contains(hash)
    }

    pub(super) fn put(&mut self, hash: u64) {
        self.puts += 1;
        if self.puts > self.hashes.slots() / 2 && self.hashes.slots() < self.most {
            self.grow();
        }
        self.hashes.put(hash);
    }

    // Out of line: a set grows a few times in an app's life.
    #[cold]
    #[inline(never)]
    fn grow(&mut self) {
        self.hashes
            .grow_to((self.hashes.slots() * 2).min(self.most));
    }

    // Starts a pass; a span ends after `idle_passes` of them.
    pub(super) fn begin_pass(&mut self, idle_passes: u64) {
        self.passes += 1;
        if self.passes >= idle_passes {
            self.passes = 0;
            self.puts = 0;
        }
    }

    // Forgets every text and gives back the room the tags took.
    pub(super) fn clear(&mut self) {
        *self = Self::new(self.most);
    }
}
