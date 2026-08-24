//! Browser media playback: one `<audio>` element, the Media Session API for
//! the lock screen, and Web Audio for the visualiser.
//!
//! The element lives on the browser thread and cannot be held by a
//! `Send + Sync` service, so — like [`crate::web_host_surface`] — the service
//! itself holds nothing. The element, its listeners and the analysis graph live
//! in a thread-local that only the browser thread ever reaches, which on wasm
//! is the only thread there is.
//!
//! The element is never added to the document: it is an audio pipeline, not a
//! control, and the application draws its own transport.
//!
//! The Media Session API is reached through `js_sys::Reflect` rather than
//! `web_sys`, whose bindings for it are behind `--cfg=web_sys_unstable_apis`.
//! An application should not have to set a rustc flag to get a lock screen,
//! and a browser without the API should not fail to play — so it is looked up
//! by name and skipped when it is not there.

use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Duration,
};

use cranpose_services::{
    publish_media_command, publish_media_samples, publish_playback_progress,
    publish_playback_state, EqualizerBand, EqualizerSettings, MediaCapabilities, MediaCommand,
    MediaError, MediaItem, MediaMetadata, MediaPlayer, MediaSamples, PlaybackProgress,
    PlaybackState,
};
use wasm_bindgen::{closure::Closure, JsCast, JsValue};
use web_sys::{
    AnalyserNode, AudioContext, BiquadFilterNode, BiquadFilterType, GainNode, HtmlAudioElement,
    MediaElementAudioSourceNode,
};

/// How many samples one analysis block carries. 2048 is the smallest FFT size
/// a browser guarantees to a useful resolution, and ~46 ms at 44.1 kHz.
const ANALYSIS_FFT_SIZE: u32 = 2048;
/// How often the analysis graph is read while a visualiser wants it.
const ANALYSIS_INTERVAL_MS: i32 = 16;

/// The equalizer's band centres and how far each reaches. The contract's own
/// octave set, so a curve saved on a desktop means the same thing here.
const EQUALIZER_BAND_CENTERS_HZ: [f32; 10] = cranpose_services::OCTAVE_BAND_CENTERS_HZ;
const EQUALIZER_RANGE_DB: f32 = 12.0;
/// One octave between centres works out at roughly this Q, which is what makes
/// the bands cover the spectrum evenly rather than leaving dips between them.
const EQUALIZER_BAND_Q: f32 = 1.41;

thread_local! {
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
}

/// The element, everything keeping its callbacks alive, and the analysis graph.
struct Browser {
    element: HtmlAudioElement,
    /// Dropping a `Closure` unregisters the JavaScript function it wraps, so
    /// every listener is held for the life of the element.
    _element_listeners: Vec<Closure<dyn FnMut()>>,
    _session_handlers: Vec<Closure<dyn FnMut(JsValue)>>,
    /// The object URL the lock-screen artwork is served from, revoked when it
    /// is replaced so a long playlist does not leak one per track.
    artwork_url: Option<String>,
    graph: Option<AudioGraph>,
}

/// The Web Audio graph the equalizer shapes and the visualiser reads.
///
/// One graph, because `createMediaElementSource` may be called once per
/// element: asking for a visualiser and asking for an equalizer cannot each
/// build their own.
///
/// `element -> preamp -> band filters -> analyser -> destination`
struct AudioGraph {
    _source: MediaElementAudioSourceNode,
    context: AudioContext,
    /// The gain ahead of the bands.
    preamp: GainNode,
    /// One peaking filter per reported band, in band order.
    filters: Vec<BiquadFilterNode>,
    /// Held for the life of the graph: dropping it disconnects the node the
    /// pump reads through its own clone.
    _analyser: AnalyserNode,
    /// The `setInterval` handle, present only while samples are wanted.
    timer: Option<i32>,
    _pump: Closure<dyn FnMut()>,
}

/// Plays media through the browser.
struct WebMediaPlayer;

/// Installs the browser as the platform media player.
pub(crate) fn install() {
    let Ok(element) = HtmlAudioElement::new() else {
        log::warn!("cranpose: this browser has no <audio> element; media playback is off");
        return;
    };
    element.set_preload("auto");
    let listeners = attach_listeners(&element);
    let handlers = attach_session_handlers();
    BROWSER.with(|slot| {
        *slot.borrow_mut() = Some(Browser {
            element,
            _element_listeners: listeners,
            _session_handlers: handlers,
            artwork_url: None,
            graph: None,
        });
    });
    cranpose_services::set_platform_media_player(Arc::new(WebMediaPlayer));
}

fn with_browser<R>(action: impl FnOnce(&mut Browser) -> R) -> Option<R> {
    BROWSER.with(|slot| slot.borrow_mut().as_mut().map(action))
}

// --- The element's own events ----------------------------------------------

fn attach_listeners(element: &HtmlAudioElement) -> Vec<Closure<dyn FnMut()>> {
    let mut listeners = Vec::new();
    // `loadedmetadata` is when the duration becomes known, `timeupdate` and
    // `progress` are the position and the buffer moving, and the rest are the
    // browser telling us what it did with a `play()` we may not have asked for
    // — a headset button reaches the element directly.
    for (event, publish) in [
        ("loadedmetadata", publish_ready as fn()),
        ("durationchange", publish_ready),
        ("timeupdate", publish_position),
        ("progress", publish_position),
        ("playing", publish_playing),
        ("play", publish_playing),
        ("pause", publish_paused),
        ("waiting", publish_waiting),
        ("ended", publish_ended),
        ("error", publish_failure),
    ] {
        let closure = Closure::wrap(Box::new(publish) as Box<dyn FnMut()>);
        if element
            .add_event_listener_with_callback(event, closure.as_ref().unchecked_ref())
            .is_err()
        {
            log::warn!("cranpose: the browser refused a `{event}` listener on the media element");
        }
        listeners.push(closure);
    }
    listeners
}

fn publish_ready() {
    publish_position();
    if with_browser(|browser| browser.element.paused()).unwrap_or(true) {
        publish_playback_state(PlaybackState::Paused);
    }
}

fn publish_playing() {
    publish_playback_state(PlaybackState::Playing);
}

fn publish_paused() {
    // `pause` also fires as part of ending an item; the `ended` listener has
    // already published that, and re-publishing `Paused` over it would lose
    // the one event a playlist advances on.
    if with_browser(|browser| browser.element.ended()).unwrap_or(false) {
        return;
    }
    publish_playback_state(PlaybackState::Paused);
}

fn publish_waiting() {
    publish_playback_state(PlaybackState::Loading);
}

fn publish_ended() {
    publish_playback_state(PlaybackState::Ended);
}

fn publish_failure() {
    publish_playback_state(PlaybackState::Failed(MediaError::Failed(
        "the browser could not play this item".to_string(),
    )));
}

fn publish_position() {
    let Some(progress) = with_browser(|browser| {
        let element = &browser.element;
        let duration = finite_seconds(element.duration());
        PlaybackProgress {
            position: seconds(element.current_time()),
            duration,
            buffered: buffered_end(element).or(duration).unwrap_or_default(),
        }
    }) else {
        return;
    };
    publish_playback_progress(progress);
}

fn seconds(value: f64) -> Duration {
    if value.is_finite() && value > 0.0 {
        Duration::from_secs_f64(value)
    } else {
        Duration::ZERO
    }
}

/// A duration the browser actually knows. `NaN` before metadata arrives and
/// infinite for a live stream, both of which mean "no length" to a seek bar.
fn finite_seconds(value: f64) -> Option<Duration> {
    (value.is_finite() && value > 0.0).then(|| Duration::from_secs_f64(value))
}

fn buffered_end(element: &HtmlAudioElement) -> Option<Duration> {
    let ranges = element.buffered();
    let count = ranges.length();
    if count == 0 {
        return None;
    }
    ranges.end(count - 1).ok().map(seconds)
}

// --- The Media Session API --------------------------------------------------

fn media_session() -> Option<js_sys::Object> {
    let navigator = web_sys::window()?.navigator();
    let session = js_sys::Reflect::get(&navigator, &JsValue::from_str("mediaSession")).ok()?;
    session.dyn_into::<js_sys::Object>().ok()
}

fn attach_session_handlers() -> Vec<Closure<dyn FnMut(JsValue)>> {
    let Some(session) = media_session() else {
        return Vec::new();
    };
    let Ok(set_handler) = js_sys::Reflect::get(&session, &JsValue::from_str("setActionHandler"))
    else {
        return Vec::new();
    };
    let Ok(set_handler) = set_handler.dyn_into::<js_sys::Function>() else {
        return Vec::new();
    };

    let mut handlers = Vec::new();
    for (action, command) in [
        ("play", Some(MediaCommand::Play)),
        ("pause", Some(MediaCommand::Pause)),
        ("stop", Some(MediaCommand::Stop)),
        ("previoustrack", Some(MediaCommand::Previous)),
        ("nexttrack", Some(MediaCommand::Next)),
        ("seekto", None),
    ] {
        let closure = Closure::wrap(Box::new(move |details: JsValue| match command {
            Some(command) => publish_media_command(command),
            None => {
                if let Some(seconds) = seek_time(&details) {
                    publish_media_command(MediaCommand::SeekTo(Duration::from_secs_f64(seconds)));
                }
            }
        }) as Box<dyn FnMut(JsValue)>);
        let registered = set_handler.call2(
            &session,
            &JsValue::from_str(action),
            closure.as_ref().unchecked_ref(),
        );
        if registered.is_err() {
            // A browser that does not know an action throws for that one
            // action; the ones it does know are still registered.
            log::debug!("cranpose: this browser has no `{action}` media-session action");
        }
        handlers.push(closure);
    }
    handlers
}

fn seek_time(details: &JsValue) -> Option<f64> {
    js_sys::Reflect::get(details, &JsValue::from_str("seekTime"))
        .ok()?
        .as_f64()
        .filter(|seconds| seconds.is_finite() && *seconds >= 0.0)
}

fn publish_session_metadata(metadata: &MediaMetadata) {
    let Some(session) = media_session() else {
        return;
    };
    let Some(window) = web_sys::window() else {
        return;
    };
    let Ok(constructor) = js_sys::Reflect::get(&window, &JsValue::from_str("MediaMetadata")) else {
        return;
    };
    let Ok(constructor) = constructor.dyn_into::<js_sys::Function>() else {
        return;
    };

    let init = js_sys::Object::new();
    set_string(&init, "title", &metadata.title);
    set_string(&init, "artist", &metadata.artist);
    set_string(&init, "album", &metadata.album);
    if let Some(url) = artwork_url(metadata) {
        let image = js_sys::Object::new();
        set_string(&image, "src", &url);
        let artwork = js_sys::Array::new();
        artwork.push(&image);
        let _ = js_sys::Reflect::set(&init, &JsValue::from_str("artwork"), &artwork);
    }

    let arguments = js_sys::Array::new();
    arguments.push(&init);
    let Ok(built) = js_sys::Reflect::construct(&constructor, &arguments) else {
        return;
    };
    let _ = js_sys::Reflect::set(&session, &JsValue::from_str("metadata"), &built);
}

fn set_string(object: &js_sys::Object, key: &str, value: &str) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &JsValue::from_str(value));
}

/// Publishes the artwork as an object URL, revoking the one the previous item
/// used so a long playlist does not leak one blob per track.
fn artwork_url(metadata: &MediaMetadata) -> Option<String> {
    let artwork = metadata.artwork.as_ref();
    let url = artwork.and_then(|artwork| {
        let bytes = js_sys::Uint8Array::from(&artwork.bytes[..]);
        let parts = js_sys::Array::new();
        parts.push(&bytes.buffer());
        let options = web_sys::BlobPropertyBag::new();
        options.set_type(&artwork.mime);
        let blob = web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).ok()?;
        web_sys::Url::create_object_url_with_blob(&blob).ok()
    });
    with_browser(|browser| {
        if let Some(previous) = browser.artwork_url.take() {
            let _ = web_sys::Url::revoke_object_url(&previous);
        }
        browser.artwork_url = url.clone();
    });
    url
}

// --- Web Audio analysis -----------------------------------------------------

fn ensure_graph(browser: &mut Browser) -> Option<&mut AudioGraph> {
    if browser.graph.is_none() {
        let context = AudioContext::new().ok()?;
        let source = context.create_media_element_source(&browser.element).ok()?;
        let analyser = context.create_analyser().ok()?;
        analyser.set_fft_size(ANALYSIS_FFT_SIZE);

        let preamp = context.create_gain().ok()?;
        preamp.gain().set_value(1.0);
        source.connect_with_audio_node(&preamp).ok()?;

        // A peaking filter at 0 dB passes its input through, so the bands stay
        // in the chain whether or not the equalizer is switched on. Rewiring
        // the graph per switch would cost a click; ten transparent biquads
        // cost the browser nothing anyone can measure.
        let mut filters: Vec<BiquadFilterNode> = Vec::new();
        for center in EQUALIZER_BAND_CENTERS_HZ {
            let filter = context.create_biquad_filter().ok()?;
            filter.set_type(BiquadFilterType::Peaking);
            filter.frequency().set_value(center);
            filter.q().set_value(EQUALIZER_BAND_Q);
            filter.gain().set_value(0.0);
            filters.push(filter);
        }
        let mut tail: &web_sys::AudioNode = preamp.as_ref();
        for filter in &filters {
            tail.connect_with_audio_node(filter).ok()?;
            tail = filter.as_ref();
        }
        tail.connect_with_audio_node(&analyser).ok()?;
        analyser
            .connect_with_audio_node(&context.destination())
            .ok()?;

        let pump_analyser = analyser.clone();
        let sample_rate = context.sample_rate() as u32;
        let buffer = RefCell::new(vec![0.0f32; ANALYSIS_FFT_SIZE as usize]);
        let sequence = Cell::new(0u64);
        let pump = Closure::wrap(Box::new(move || {
            let mut buffer = buffer.borrow_mut();
            pump_analyser.get_float_time_domain_data(&mut buffer);
            sequence.set(sequence.get() + 1);
            // The analyser mixes the graph down to one channel, which is the
            // channel a visualiser draws anyway.
            if let Some(samples) =
                MediaSamples::new(sample_rate, 1, sequence.get(), buffer.as_slice())
            {
                publish_media_samples(samples);
            }
        }) as Box<dyn FnMut()>);

        browser.graph = Some(AudioGraph {
            _source: source,
            context,
            preamp,
            filters,
            timer: None,
            _analyser: analyser,
            _pump: pump,
        });
    }
    browser.graph.as_mut()
}

/// Applies a curve, building the graph on first use.
fn set_equalizer_curve(browser: &mut Browser, settings: &EqualizerSettings) {
    let Some(graph) = ensure_graph(browser) else {
        return;
    };
    // Routing the element through the graph makes the graph's state the one
    // that matters, and a context created before the user has interacted with
    // the page starts suspended.
    let _ = graph.context.resume();
    let preamp_db = if settings.enabled {
        settings.preamp_db
    } else {
        0.0
    };
    graph.preamp.gain().set_value(10f32.powf(preamp_db / 20.0));
    for (index, filter) in graph.filters.iter().enumerate() {
        let gain = if settings.enabled {
            settings.gains_db.get(index).copied().unwrap_or(0.0)
        } else {
            0.0
        };
        filter.gain().set_value(gain);
    }
}

fn set_analysis_running(browser: &mut Browser, enabled: bool) -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    if !enabled {
        if let Some(graph) = browser.graph.as_mut() {
            if let Some(timer) = graph.timer.take() {
                window.clear_interval_with_handle(timer);
            }
        }
        return true;
    }
    let Some(graph) = ensure_graph(browser) else {
        return false;
    };
    if graph.timer.is_some() {
        return true;
    }
    let _ = graph.context.resume();
    let started = window.set_interval_with_callback_and_timeout_and_arguments_0(
        graph._pump.as_ref().unchecked_ref(),
        ANALYSIS_INTERVAL_MS,
    );
    match started {
        Ok(timer) => {
            graph.timer = Some(timer);
            true
        }
        Err(_) => false,
    }
}

// --- The service contract ---------------------------------------------------

/// Extension and the media type to ask the browser about. Which of these play
/// is the browser's answer, not ours: the same page in two browsers decodes a
/// different set, and Safari and Firefox disagree about most of the second
/// half of this list.
const WEB_AUDIO_CANDIDATES: &[(&str, &str)] = &[
    ("aac", "audio/aac"),
    ("aif", "audio/aiff"),
    ("aiff", "audio/aiff"),
    ("caf", "audio/x-caf"),
    ("flac", "audio/flac"),
    ("m4a", "audio/mp4"),
    ("m4b", "audio/mp4"),
    ("mka", "audio/x-matroska"),
    ("mp1", "audio/mpeg"),
    ("mp2", "audio/mpeg"),
    ("mp3", "audio/mpeg"),
    ("mp4", "audio/mp4"),
    ("oga", "audio/ogg"),
    ("ogg", "audio/ogg"),
    ("opus", "audio/ogg; codecs=opus"),
    ("wav", "audio/wav"),
    ("wave", "audio/wav"),
    ("webm", "audio/webm"),
];

impl MediaPlayer for WebMediaPlayer {
    fn capabilities(&self) -> MediaCapabilities {
        MediaCapabilities {
            seeking: true,
            speed: true,
            looping: true,
            analysis: true,
            session: media_session().is_some(),
            equalizer: true,
            // An `HTMLAudioElement` only knows a duration once it has loaded
            // the metadata, so there is nothing to answer a synchronous probe
            // with.
            probing: false,
        }
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
        // `canPlayType` is the browser's own answer, and the only honest one:
        // an empty string is "no", and anything else ("probably", "maybe") is
        // as much of a yes as the specification allows a browser to give.
        with_browser(|browser| {
            WEB_AUDIO_CANDIDATES
                .iter()
                .filter(|(_, media_type)| !browser.element.can_play_type(media_type).is_empty())
                .map(|(extension, _)| *extension)
                .collect()
        })
        .unwrap_or_default()
    }
    fn prepare(&self, item: &MediaItem) -> Result<(), MediaError> {
        with_browser(|browser| {
            browser.element.set_src(&item.uri);
            browser.element.load();
        })
        .ok_or(MediaError::Unsupported)?;
        publish_playback_progress(PlaybackProgress {
            position: Duration::ZERO,
            duration: item.metadata.duration,
            buffered: Duration::ZERO,
        });
        Ok(())
    }

    fn play(&self) -> Result<(), MediaError> {
        let promise =
            with_browser(|browser| browser.element.play()).ok_or(MediaError::Unsupported)?;
        match promise {
            // The promise rejects when the browser's autoplay policy refuses a
            // `play()` that no gesture asked for. The element stays paused and
            // publishes `pause` itself, so there is nothing to undo — but the
            // rejection has to be taken, or the browser reports it as an
            // unhandled one on the console of every application.
            Ok(promise) => {
                wasm_bindgen_futures::spawn_local(async move {
                    if let Err(error) = wasm_bindgen_futures::JsFuture::from(promise).await {
                        log::debug!("cranpose: the browser refused to start playback: {error:?}");
                    }
                });
                Ok(())
            }
            Err(error) => Err(MediaError::Failed(format!("{error:?}"))),
        }
    }

    fn pause(&self) {
        with_browser(|browser| {
            let _ = browser.element.pause();
        });
    }

    fn stop(&self) {
        with_browser(|browser| {
            let _ = browser.element.pause();
            browser.element.set_src("");
            if let Some(url) = browser.artwork_url.take() {
                let _ = web_sys::Url::revoke_object_url(&url);
            }
        });
        let _ = self.set_analysis_enabled(false);
    }

    fn seek_to(&self, position: Duration) -> Result<(), MediaError> {
        with_browser(|browser| browser.element.set_current_time(position.as_secs_f64()))
            .ok_or(MediaError::Unsupported)?;
        publish_position();
        Ok(())
    }

    fn set_volume(&self, volume: f32) {
        with_browser(|browser| browser.element.set_volume(volume.clamp(0.0, 1.0) as f64));
    }

    fn set_speed(&self, speed: f32) -> bool {
        with_browser(|browser| {
            browser
                .element
                .set_playback_rate(speed.clamp(0.25, 4.0) as f64)
        })
        .is_some()
    }

    fn set_looping(&self, looping: bool) {
        with_browser(|browser| browser.element.set_loop(looping));
    }

    fn equalizer_bands(&self) -> Vec<EqualizerBand> {
        cranpose_services::octave_equalizer_bands(EQUALIZER_RANGE_DB)
    }

    fn set_equalizer(&self, settings: &EqualizerSettings) {
        with_browser(|browser| set_equalizer_curve(browser, settings));
    }

    fn set_analysis_enabled(&self, enabled: bool) -> bool {
        with_browser(|browser| set_analysis_running(browser, enabled)).unwrap_or(false)
    }

    fn set_session_metadata(&self, metadata: &MediaMetadata) {
        publish_session_metadata(metadata);
    }
}
