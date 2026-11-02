//! Voice and video calls of the account, one at a time.
//!
//! The library does the signalling, the encryption, the relay and the audio codec; the application gives it frames of
//! the microphone (60 ms of 16 kHz PCM) and, for video, H.264 access units, and receives the same from the other side.
//! Here the page (the window of the conversation) is the microphone, the camera and the screen: its frames come in as
//! commands, and what the library delivers goes out as `Sink::media`.

use super::Ctx;
use crate::chat::event::{CallInfo, CallState, Event as Ev, LineKind};
use crate::chat::CallCommand;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;
use whatsapp_rust::async_channel;
use whatsapp_rust::prelude::*;
use whatsapp_rust::voip::{CallEvent, CallHandle, VideoFrame};
use whatsapp_rust::wacore::types::call::{CallAction, IncomingCall};

/// Frames waiting between the page and the library (a late frame is dropped, never queued).
const QUEUE: usize = 6;
/// One microphone frame: 960 samples of 16 bits.
const MIC_FRAME_BYTES: usize = 1920;
/// The largest access unit taken from the camera.
const MAX_AU_BYTES: usize = 400_000;

#[derive(Default)]
pub(super) struct Calls {
    /// Calls that ring, by call identifier, with the buffer of the caller.
    incoming: Mutex<HashMap<String, (IncomingCall, String)>>,
    live: Mutex<Option<Live>>,
}

struct Live {
    id: String,
    buffer: String,
    video: bool,
    handle: Arc<CallHandle>,
    mic: async_channel::Sender<Vec<i16>>,
    cam: Option<async_channel::Sender<Vec<u8>>>,
    /// When the call became active.
    since: Arc<Mutex<Option<Instant>>>,
}

impl Ctx {
    fn call_event(&self, id: &str, buffer: &str, state: CallState, video: bool, text: &str) {
        self.sink.emit(Ev::Call { call: CallInfo { id: id.to_string(), buffer: buffer.to_string(), state, video, text: text.to_string() } });
    }

    /// A call stanza reached us: someone rings, or hangs up before we answered.
    pub(super) async fn on_incoming_call(self: &Arc<Self>, client: &Arc<Client>, c: &IncomingCall) {
        match &c.action {
            CallAction::Offer { call_id, is_video, .. } => {
                let chat = c.from.to_string();
                let busy = self.calls.live.lock().unwrap().is_some() || !self.calls.incoming.lock().unwrap().is_empty();
                if chat.ends_with("@g.us") || busy {
                    // a group call, or a call while we are in one: not taken
                    let _ = client.voip().reject(c).await;
                    if chat.ends_with("@g.us") {
                        self.line("", LineKind::Info, "Un appel de groupe a été refusé : Messages ne les prend pas en charge.");
                    }
                    return;
                }
                let buffer = self.buffer_of_chat(client, &chat, false, &c.from).await;
                let who = self.registry.lock().unwrap().display(&chat);
                self.calls.incoming.lock().unwrap().insert(call_id.clone(), (c.clone(), buffer.clone()));
                self.call_event(call_id, &buffer, CallState::Incoming, *is_video, &who);
            }
            CallAction::Terminate { call_id, .. } => self.end_incoming(call_id, "Appel manqué"),
            _ => {}
        }
    }

    /// A call that rang is over (the caller gave up, or another device answered).
    pub(super) fn end_incoming(&self, id: &str, text: &str) {
        let gone = self.calls.incoming.lock().unwrap().remove(id);
        if let Some((_, buffer)) = gone {
            self.call_event(id, &buffer, CallState::Ended, false, text);
            self.line(&buffer, LineKind::Info, text);
        }
    }

    /// A frame of the microphone or of the camera: handed over at once (no waiting, so the frames keep their order).
    pub(super) fn call_media_in(&self, cmd: CallCommand) {
        let live = self.calls.live.lock().unwrap();
        let Some(live) = live.as_ref() else { return };
        match cmd {
            CallCommand::Audio { id, pcm } if id == live.id && pcm.len() == MIC_FRAME_BYTES => {
                let samples: Vec<i16> = pcm.chunks_exact(2).map(|b| i16::from_le_bytes([b[0], b[1]])).collect();
                let _ = live.mic.try_send(samples);
            }
            CallCommand::Video { id, au } if id == live.id && au.len() <= MAX_AU_BYTES => {
                if let Some(cam) = &live.cam {
                    let _ = cam.try_send(au);
                }
            }
            _ => {}
        }
    }

    /// Everything else a page asks of a call.
    pub(super) async fn on_call_command(self: &Arc<Self>, client: &Arc<Client>, cmd: CallCommand) {
        match cmd {
            CallCommand::Start { buffer, video } => {
                if let Err(e) = self.call_start(client, &buffer, video).await {
                    self.line(&buffer, LineKind::Error, &format!("Appel impossible : {e}"));
                    self.call_event("", &buffer, CallState::Ended, video, &e);
                }
            }
            CallCommand::Accept { id, video } => {
                let buffer = self.calls.incoming.lock().unwrap().get(&id).map(|(_, b)| b.clone()).unwrap_or_default();
                if let Err(e) = self.call_accept(client, &id, video).await {
                    self.line(&buffer, LineKind::Error, &format!("Appel impossible : {e}"));
                    self.calls.incoming.lock().unwrap().remove(&id);
                    self.call_event(&id, &buffer, CallState::Ended, video, &e);
                }
            }
            CallCommand::Reject { id } => {
                let gone = self.calls.incoming.lock().unwrap().remove(&id);
                if let Some((incoming, buffer)) = gone {
                    let _ = client.voip().reject(&incoming).await;
                    self.call_event(&id, &buffer, CallState::Ended, false, "Appel refusé");
                }
            }
            CallCommand::Hangup { id } => {
                let handle = self.calls.live.lock().unwrap().as_ref().filter(|l| l.id == id).map(|l| l.handle.clone());
                if let Some(h) = handle {
                    h.hangup().await;
                }
            }
            CallCommand::Mute { id, muted } => {
                if let Some(l) = self.calls.live.lock().unwrap().as_ref().filter(|l| l.id == id) {
                    l.handle.set_muted(muted);
                }
            }
            CallCommand::Audio { .. } | CallCommand::Video { .. } => self.call_media_in(cmd),
        }
    }

    /// The call in progress is over because the connection is being closed.
    pub(super) async fn hang_up_everything(&self) {
        let handle = self.calls.live.lock().unwrap().as_ref().map(|l| l.handle.clone());
        if let Some(h) = handle {
            h.hangup().await;
        }
    }

    async fn call_start(self: &Arc<Self>, client: &Arc<Client>, buffer: &str, video: bool) -> Result<(), String> {
        if self.calls.live.lock().unwrap().is_some() || !self.calls.incoming.lock().unwrap().is_empty() {
            return Err("Un appel est déjà en cours".into());
        }
        let target = self.registry.lock().unwrap().chat_of(buffer).filter(|(_, group)| !group).map(|(j, _)| j.to_string());
        let jid = target.and_then(|j| j.parse::<Jid>().ok()).ok_or("On n'appelle que les personnes, pas les groupes")?;
        let (mic_tx, mic_rx) = async_channel::bounded::<Vec<i16>>(QUEUE);
        let (spk_tx, spk_rx) = async_channel::bounded::<Vec<i16>>(QUEUE);
        let voip = client.voip();
        let mut call = voip.call(&jid).audio(mic_rx, spk_tx);
        let (mut cam, mut seen) = (None, None);
        if video {
            let (cam_tx, cam_rx) = async_channel::bounded::<Vec<u8>>(QUEUE);
            let (vid_tx, vid_rx) = async_channel::bounded::<VideoFrame>(QUEUE);
            call = call.video(cam_rx, vid_tx);
            (cam, seen) = (Some(cam_tx), Some(vid_rx));
        }
        let handle = call.start().await.map_err(|e| e.to_string())?;
        let who = self.registry.lock().unwrap().display(&jid.to_string());
        self.run_call(handle, buffer, video, mic_tx, cam, spk_rx, seen, CallState::Calling, &who);
        Ok(())
    }

    async fn call_accept(self: &Arc<Self>, client: &Arc<Client>, id: &str, video: bool) -> Result<(), String> {
        if self.calls.live.lock().unwrap().is_some() {
            return Err("Un appel est déjà en cours".into());
        }
        let (incoming, buffer) = self.calls.incoming.lock().unwrap().remove(id).ok_or("Cet appel n'existe plus")?;
        let offered_video = matches!(incoming.action, CallAction::Offer { is_video: true, .. });
        let with_video = offered_video && video;
        let (mic_tx, mic_rx) = async_channel::bounded::<Vec<i16>>(QUEUE);
        let (spk_tx, spk_rx) = async_channel::bounded::<Vec<i16>>(QUEUE);
        let voip = client.voip();
        let mut call = voip.accept(&incoming).audio(mic_rx, spk_tx);
        let (mut cam, mut seen) = (None, None);
        if with_video {
            let (cam_tx, cam_rx) = async_channel::bounded::<Vec<u8>>(QUEUE);
            let (vid_tx, vid_rx) = async_channel::bounded::<VideoFrame>(QUEUE);
            call = call.video(cam_rx, vid_tx);
            (cam, seen) = (Some(cam_tx), Some(vid_rx));
        }
        let handle = call.start().await.map_err(|e| e.to_string())?;
        let who = self.registry.lock().unwrap().display(&incoming.from.to_string());
        self.run_call(handle, &buffer, with_video, mic_tx, cam, spk_rx, seen, CallState::Calling, &who);
        Ok(())
    }

    /// The call is set up: what the library delivers goes to the page, and its end is told.
    #[allow(clippy::too_many_arguments)]
    fn run_call(
        self: &Arc<Self>,
        handle: CallHandle,
        buffer: &str,
        video: bool,
        mic: async_channel::Sender<Vec<i16>>,
        cam: Option<async_channel::Sender<Vec<u8>>>,
        speaker: async_channel::Receiver<Vec<i16>>,
        screen: Option<async_channel::Receiver<VideoFrame>>,
        state: CallState,
        who: &str,
    ) {
        let handle = Arc::new(handle);
        let id = handle.call_id().to_string();
        let since = Arc::new(Mutex::new(None));
        *self.calls.live.lock().unwrap() = Some(Live { id: id.clone(), buffer: buffer.to_string(), video, handle: handle.clone(), mic, cam, since: since.clone() });
        self.call_event(&id, buffer, state, video, who);

        // the voice of the other side
        let (ctx, call) = (self.clone(), id.clone());
        tokio::spawn(async move {
            while let Ok(pcm) = speaker.recv().await {
                let bytes: Vec<u8> = pcm.iter().flat_map(|s| s.to_le_bytes()).collect();
                ctx.sink.media(&call, "audio", &bytes);
            }
        });
        // its picture
        if let Some(screen) = screen {
            let (ctx, call) = (self.clone(), id.clone());
            tokio::spawn(async move {
                while let Ok(frame) = screen.recv().await {
                    let mut data = Vec::with_capacity(frame.data.len() + 1);
                    data.push(u8::from(frame.keyframe));
                    data.extend_from_slice(&frame.data);
                    ctx.sink.media(&call, "video", &data);
                }
            });
        }
        // what happens to the call
        let (ctx, call, events, buffer_owned, who_owned) = (self.clone(), id.clone(), handle.events(), buffer.to_string(), who.to_string());
        let since_events = since.clone();
        tokio::spawn(async move {
            while let Ok(ev) = events.recv().await {
                match ev {
                    CallEvent::RelayAllocated => {
                        *since_events.lock().unwrap() = Some(Instant::now());
                        ctx.call_event(&call, &buffer_owned, CallState::Active, video, &who_owned);
                    }
                    CallEvent::RelayAllocateFailed(_) | CallEvent::RelayAllocateTimedOut | CallEvent::RelayReconnectTimedOut => {
                        ctx.line(&buffer_owned, LineKind::Error, "Le serveur de WhatsApp n'a pas pu relayer l'appel.");
                    }
                    _ => {}
                }
            }
        });
        // its end
        let ctx = self.clone();
        tokio::spawn(async move {
            handle.wait_ended().await;
            ctx.finish_call(&id);
        });
    }

    fn finish_call(&self, id: &str) {
        let live = {
            let mut slot = self.calls.live.lock().unwrap();
            if slot.as_ref().is_some_and(|l| l.id == id) { slot.take() } else { None }
        };
        let Some(live) = live else { return };
        let took = live.since.lock().unwrap().map(|t| t.elapsed().as_secs());
        let text = match took {
            Some(s) => format!("Appel {} terminé ({:02}:{:02})", if live.video { "vidéo" } else { "audio" }, s / 60, s % 60),
            None => "Appel terminé sans réponse".to_string(),
        };
        self.line(&live.buffer, LineKind::Info, &text);
        self.call_event(id, &live.buffer, CallState::Ended, live.video, &text);
    }
}
