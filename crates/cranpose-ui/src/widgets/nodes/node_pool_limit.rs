//! How many removed layout nodes stay for later nodes to reuse.

use crate::MemoryHeadroom;

// With ample memory, the shells of up to this many removed nodes stay.
const AMPLE_NODE_POOL_LIMIT: usize = 128;

// The shells of removed layout nodes the recycle pool keeps under the
// current memory headroom. With little memory left it keeps none: a removed
// node goes, and a new one takes fresh memory, since shells kept for later
// nodes, or made ahead of them, hold memory the screen needs.
pub(crate) fn node_pool_limit() -> usize {
    match crate::current_memory_headroom() {
        MemoryHeadroom::Ample => AMPLE_NODE_POOL_LIMIT,
        MemoryHeadroom::Tight => 0,
    }
}
