// The producer's end of a present thread: it starts the thread, hands it
// packets and controls, and takes back the frames the thread returns.

use std::sync::{
    Arc,
    atomic::Ordering,
    mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel},
};

use crate::{
    frame_packet::{FramePacket, MAX_FRAMES_IN_FLIGHT, RenderReturns},
    present_runtime::{
        CONTROL_ACK_TIMEOUT, PresentControl, PresentMsg, PresentRuntimeInit, PresentState,
        PresentStatus, PresentWaker,
    },
};

pub(crate) struct PresentHandle {
    msg_tx: Sender<PresentMsg>,
    returns_rx: Receiver<RenderReturns>,
    status: Arc<PresentStatus>,
    thread: Option<std::thread::JoinHandle<()>>,
    outstanding: usize,
    drained: u64,
}

const PRESENT_THREAD_STACK_BYTES: usize = 8 * 1024 * 1024;

impl PresentHandle {
    pub(crate) fn spawn(init: PresentRuntimeInit, waker: PresentWaker) -> Result<Self, String> {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(MAX_FRAMES_IN_FLIGHT);
        let status = Arc::new(PresentStatus::default());
        let thread_status = Arc::clone(&status);
        let thread = std::thread::Builder::new()
            .name("cranpose-present".to_string())
            .stack_size(PRESENT_THREAD_STACK_BYTES)
            .spawn(move || {
                crate::fast_cores::pin_current_thread_to_fast_cores("present");
                if let Some(id) = crate::fast_cores::current_thread_id() {
                    thread_status.thread_id.store(id, Ordering::Relaxed);
                }
                PresentState::new(init, returns_tx, thread_status, waker).run(msg_rx);
            })
            .map_err(|error| format!("failed to spawn present thread: {error}"))?;
        Ok(Self {
            msg_tx,
            returns_rx,
            status,
            thread: Some(thread),
            outstanding: 0,
            drained: 0,
        })
    }

    #[cfg(feature = "test-support")]
    pub(crate) fn new_inline(
        init: PresentRuntimeInit,
        waker: PresentWaker,
    ) -> (Self, PresentState, Receiver<PresentMsg>) {
        let (msg_tx, msg_rx) = channel::<PresentMsg>();
        let (returns_tx, returns_rx) = sync_channel::<RenderReturns>(MAX_FRAMES_IN_FLIGHT);
        let status = Arc::new(PresentStatus::default());
        let state = PresentState::new(init, returns_tx, Arc::clone(&status), waker);
        (
            Self {
                msg_tx,
                returns_rx,
                status,
                thread: None,
                outstanding: 0,
                drained: 0,
            },
            state,
            msg_rx,
        )
    }

    pub(crate) fn status(&self) -> &PresentStatus {
        &self.status
    }

    pub(crate) fn has_credit(&self) -> bool {
        self.outstanding < MAX_FRAMES_IN_FLIGHT
    }

    /// Whether the present thread has handed back a frame that
    /// [`Self::try_drain`] has not taken yet, which frees a credit.
    pub(crate) fn has_undrained_return(&self) -> bool {
        self.status.returned_frames.load(Ordering::Acquire) > self.drained
    }

    pub(crate) fn publish(&mut self, packet: FramePacket) -> Result<(), Box<FramePacket>> {
        debug_assert!(self.has_credit(), "publish without credit");
        match self.msg_tx.send(PresentMsg::Packet(packet)) {
            Ok(()) => {
                self.outstanding += 1;
                Ok(())
            }
            Err(error) => {
                let PresentMsg::Packet(packet) = error.0 else {
                    unreachable!("publish only sends packets");
                };
                Err(Box::new(packet))
            }
        }
    }

    pub(crate) fn try_drain(&mut self) -> Option<RenderReturns> {
        match self.returns_rx.try_recv() {
            Ok(returns) => {
                self.outstanding = self.outstanding.saturating_sub(1);
                self.drained += 1;
                Some(returns)
            }
            Err(_) => None,
        }
    }

    // Sends the control `build` makes and waits for the thread's ack of
    // it; false when no ack came.
    pub(crate) fn send_control_and_wait(
        &self,
        build: impl FnOnce(SyncSender<()>) -> PresentControl,
        what: &str,
    ) -> bool {
        let Some(ack_rx) = self.send_control_unacked(build) else {
            log::error!("[present-runtime] {what}: runtime is gone");
            return false;
        };
        match ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT) {
            Ok(()) => true,
            Err(RecvTimeoutError::Timeout) => {
                log::error!(
                    "[present-runtime] {what}: no ack within {CONTROL_ACK_TIMEOUT:?}; \
                     present thread wedged?"
                );
                false
            }
            Err(RecvTimeoutError::Disconnected) => {
                log::error!("[present-runtime] {what}: runtime exited before ack");
                false
            }
        }
    }

    /// Sends `control` behind the packets already sent, waiting for
    /// nothing. False when the runtime is gone.
    pub(crate) fn send_control(&self, control: PresentControl) -> bool {
        self.msg_tx.send(PresentMsg::Control(control)).is_ok()
    }

    pub(crate) fn send_control_unacked(
        &self,
        build: impl FnOnce(SyncSender<()>) -> PresentControl,
    ) -> Option<Receiver<()>> {
        let (ack_tx, ack_rx) = sync_channel::<()>(1);
        self.msg_tx
            .send(PresentMsg::Control(build(ack_tx)))
            .ok()
            .map(|()| ack_rx)
    }

    pub(crate) fn shutdown(&mut self) {
        let _ = self
            .msg_tx
            .send(PresentMsg::Control(PresentControl::Shutdown));
        if let Some(thread) = self.thread.take()
            && thread.join().is_err()
        {
            log::error!("[present-runtime] present thread panicked");
        }
    }
}

impl Drop for PresentHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}
