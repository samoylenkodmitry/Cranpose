//! Where the chat's tokens come from. On the desktop that is the data
//! layer's thread. A browser page has no thread to give it, so there a worker
//! job makes the tokens in batches, in a Web Worker, and the page takes them
//! from the batch it holds.

#[cfg(target_family = "wasm")]
pub(super) use worker::ChatFeed;

#[cfg(not(target_family = "wasm"))]
pub(super) use crate::data::ChatFeed;

#[cfg(target_family = "wasm")]
mod worker {
    use std::{
        cell::{Cell, RefCell},
        collections::VecDeque,
        rc::Rc,
    };

    use cranpose::{launchBlocking, worker_job};

    use crate::data::{CHAT_TOKENS_PER_FRAME, ChatToken};

    // A batch holds the tokens of about a second of frames, and the next one
    // is asked for while a quarter of a batch is left.
    const BATCH: u32 = (CHAT_TOKENS_PER_FRAME * 64) as u32;
    const LOW: usize = CHAT_TOKENS_PER_FRAME * 16;

    // The tokens from number `first` on: each one's block and how much of the
    // block shows, as little-endian u32s.
    #[worker_job]
    fn chat_tokens(first: u32) -> Vec<u8> {
        (first..first + BATCH)
            .flat_map(|count| {
                let token = ChatToken::after(count as usize);
                [token.block as u32, token.shown as u32]
            })
            .flat_map(u32::to_le_bytes)
            .collect()
    }

    struct Held {
        ready: RefCell<VecDeque<ChatToken>>,
        // The number of the first token of the next batch.
        next: Cell<u32>,
        asked: Cell<bool>,
    }

    pub struct ChatFeed {
        held: Rc<Held>,
    }

    impl ChatFeed {
        pub fn start() -> std::io::Result<Self> {
            Ok(Self {
                held: Rc::new(Held {
                    ready: RefCell::new(VecDeque::new()),
                    next: Cell::new(1),
                    asked: Cell::new(false),
                }),
            })
        }

        // The next token, when a batch that holds it has arrived.
        pub fn take(&self) -> Option<ChatToken> {
            let (token, left) = {
                let mut ready = self.held.ready.borrow_mut();
                (ready.pop_front(), ready.len())
            };
            if left < LOW && !self.held.asked.replace(true) {
                self.ask();
            }
            token
        }

        fn ask(&self) {
            let held = Rc::clone(&self.held);
            let first = held.next.get();
            let asked = launchBlocking(chat_tokens.with(first), move |answer| match answer {
                Ok(bytes) => {
                    let mut ready = held.ready.borrow_mut();
                    for (index, token) in bytes.chunks_exact(8).enumerate() {
                        let word = |at: usize| {
                            u32::from_le_bytes([
                                token[at],
                                token[at + 1],
                                token[at + 2],
                                token[at + 3],
                            ])
                        };
                        ready.push_back(ChatToken {
                            count: first as usize + index,
                            block: word(0) as usize,
                            shown: word(4) as usize,
                        });
                    }
                    held.next.set(first + BATCH);
                    held.asked.set(false);
                }
                Err(error) => log::warn!("chat tokens: {error}"),
            });
            // Without a runtime yet, the next frame asks again.
            if asked.is_err() {
                self.held.asked.set(false);
            }
        }
    }
}
