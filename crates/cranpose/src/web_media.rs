use std::{
    cell::{Cell, RefCell},
    sync::Arc,
    time::Duration,
};

use cranpose_services::{
    EqualizerBand, EqualizerSettings, MediaCapabilities, MediaCommand, MediaError, MediaItem,
    MediaMetadata, MediaPlayer, MediaSamples, PlaybackProgress, PlaybackState,
    publish_media_command, publish_media_samples, publish_playback_progress,
    publish_playback_state,
};
use wasm_bindgen::{JsCast, JsValue, closure::Closure};
use web_sys::{
    AnalyserNode, AudioContext, BiquadFilterNode, BiquadFilterType, GainNode, HtmlAudioElement,
    MediaElementAudioSourceNode,
};

const ANALYSIS_FFT_SIZE: u32 = 2048;
const ANALYSIS_INTERVAL_MS: i32 = 16;

const EQUALIZER_BAND_CENTERS_HZ: [f32; 10] = cranpose_services::OCTAVE_BAND_CENTERS_HZ;
const EQUALIZER_RANGE_DB: f32 = 12.0;
const EQUALIZER_BAND_Q: f32 = 1.41;

thread_local! {
    static BROWSER: RefCell<Option<Browser>> = const { RefCell::new(None) };
}

struct Browser {
    element: HtmlAudioElement,
    _element_listeners: Vec<Closure<dyn FnMut()>>,
    _session_handlers: Vec<Closure<dyn FnMut(JsValue)>>,
    artwork_url: Option<String>,
    graph: Option<AudioGraph>,
}

struct AudioGraph {
    _source: MediaElementAudioSourceNode,
    context: AudioContext,
    preamp: GainNode,
    filters: Vec<BiquadFilterNode>,
    _analyser: AnalyserNode,
    timer: Option<i32>,
    _pump: Closure<dyn FnMut()>,
}

struct WebMediaPlayer;

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

fn attach_listeners(element: &HtmlAudioElement) -> Vec<Closure<dyn FnMut()>> {
    let mut listeners = Vec::new();
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

fn ensure_graph(browser: &mut Browser) -> Option<&mut AudioGraph> {
    if browser.graph.is_none() {
        let context = AudioContext::new().ok()?;
        let source = context.create_media_element_source(&browser.element).ok()?;
        let analyser = context.create_analyser().ok()?;
        analyser.set_fft_size(ANALYSIS_FFT_SIZE);

        let preamp = context.create_gain().ok()?;
        preamp.gain().set_value(1.0);
        source.connect_with_audio_node(&preamp).ok()?;

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

fn set_equalizer_curve(browser: &mut Browser, settings: &EqualizerSettings) {
    let Some(graph) = ensure_graph(browser) else {
        return;
    };
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
        if let Some(graph) = browser.graph.as_mut()
            && let Some(timer) = graph.timer.take()
        {
            window.clear_interval_with_handle(timer);
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
            probing: false,
        }
    }

    fn audio_extensions(&self) -> Vec<&'static str> {
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
