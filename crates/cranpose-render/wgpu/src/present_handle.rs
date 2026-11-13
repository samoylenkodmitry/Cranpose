// The producer's end of a present thread: it starts the thread, hands it
// packets and controls, and takes back the frames the thread returns.

use std::sync::{
    Arc,
    atomic::Ordering,
    mpsc::{Receiver, RecvTimeoutError, Sender, SyncSender, channel, sync_channel},
};

use crate::{
    frame_packet::{
        CancelReason, CaptureReply, FramePacket, MAX_FRAMES_IN_FLIGHT, PacketOverlap, RenderReturns,
    },
    present_runtime::{
        CAPTURE_TIMEOUT, CONTROL_ACK_TIMEOUT, PresentControl, PresentMsg, PresentRuntimeInit,
        PresentState, PresentStatus, PresentWaker,
    },
    render::GpuRenderer,
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

    pub(crate) fn has_credit(&self, overlap: PacketOverlap) -> bool {
        match overlap {
            PacketOverlap::WithFramesInFlight => self.outstanding < MAX_FRAMES_IN_FLIGHT,
            PacketOverlap::Alone => self.outstanding == 0,
        }
    }

    /// Whether the present thread has handed back a frame that
    /// [`Self::try_drain`] has not taken yet, which frees a credit.
    pub(crate) fn has_undrained_return(&self) -> bool {
        self.status.returned_frames.load(Ordering::Acquire) > self.drained
    }

    pub(crate) fn publish(&mut self, packet: FramePacket) -> Result<(), Box<FramePacket>> {
        debug_assert!(
            self.has_credit(PacketOverlap::WithFramesInFlight),
            "publish without credit"
        );
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

    // Sends the control `build` makes and waits for the thread's answer to
    // it; `None` when no answer came.
    pub(crate) fn send_control_and_wait<T>(
        &self,
        build: impl FnOnce(SyncSender<T>) -> PresentControl,
        what: &str,
    ) -> Option<T> {
        let Some(ack_rx) = self.send_control_unacked(build) else {
            log::error!("[present-runtime] {what}: runtime is gone");
            return None;
        };
        match ack_rx.recv_timeout(CONTROL_ACK_TIMEOUT) {
            Ok(answer) => Some(answer),
            Err(RecvTimeoutError::Timeout) => {
                log::error!(
                    "[present-runtime] {what}: no ack within {CONTROL_ACK_TIMEOUT:?}; \
                     present thread wedged?"
                );
                None
            }
            Err(RecvTimeoutError::Disconnected) => {
                log::error!("[present-runtime] {what}: runtime exited before ack");
                None
            }
        }
    }

    /// Sends `control` behind the packets already sent, waiting for
    /// nothing. False when the runtime is gone.
    pub(crate) fn send_control(&self, control: PresentControl) -> bool {
        self.msg_tx.send(PresentMsg::Control(control)).is_ok()
    }

    // Draws `packet` into an image of `viewport` on the present thread,
    // behind the packets already sent, and waits for its pixels: a robot's
    // capture, which the frame loop never asks for.
    pub(crate) fn capture(&self, packet: FramePacket, viewport: (u32, u32)) -> CaptureReply {
        let (reply, reply_rx) = sync_channel(1);
        let capture = PresentControl::Capture {
            packet,
            viewport,
            reply,
        };
        if let Err(error) = self.msg_tx.send(PresentMsg::Control(capture)) {
            let PresentMsg::Control(PresentControl::Capture { packet, .. }) = error.0 else {
                unreachable!("capture only sends a capture");
            };
            let mut returns = RenderReturns::default();
            GpuRenderer::cancel_packet(packet, CancelReason::SurfaceUnavailable, &mut returns);
            return CaptureReply {
                returns,
                pixels: Err("the present thread is gone".to_string()),
                stats: None,
            };
        }
        reply_rx
            .recv_timeout(CAPTURE_TIMEOUT)
            .unwrap_or_else(|error| CaptureReply {
                returns: RenderReturns::default(),
                pixels: Err(format!("the present thread sent no capture: {error}")),
                stats: None,
            })
    }

    pub(crate) fn send_control_unacked<T>(
        &self,
        build: impl FnOnce(SyncSender<T>) -> PresentControl,
    ) -> Option<Receiver<T>> {
        let (ack_tx, ack_rx) = sync_channel::<T>(1);
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
