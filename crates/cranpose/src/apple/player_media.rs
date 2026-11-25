#![expect(
    unsafe_code,
    reason = "AVFoundation, CoreMedia, and CoreVideo APIs are called through Objective-C FFI"
)]

use std::{
    fs,
    sync::{
        Arc, Condvar, Mutex, PoisonError,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use cranpose_services::{
    Cue, CueAlignment, CueGroup, MediaItem, MediaProperties, PlayerError, PlayerSink,
    SubtitleTrack, TrackEntry, TrackFormat, TrackGroup, TrackSelectionParameters, TrackType,
    Tracks, VideoRotation, VideoSize, parse_subtitles,
};
use dispatch2::DispatchQueue;
use objc2::{
    AllocAnyThread, DefinedClass, define_class, msg_send,
    rc::Retained,
    runtime::{AnyObject, NSObject, ProtocolObject},
    sel,
};
use objc2_av_foundation::{
    AVAsset, AVAssetResourceLoaderDelegate, AVAssetResourceLoadingRequest,
    AVMediaCharacteristicAudible, AVMediaCharacteristicLegible, AVMediaSelectionGroup,
    AVMediaTypeAudio, AVMediaTypeVideo, AVPlayerItem, AVPlayerItemDidPlayToEndTimeNotification,
    AVPlayerItemFailedToPlayToEndTimeNotification, AVPlayerItemLegibleOutput,
    AVPlayerItemLegibleOutputPushDelegate, AVPlayerItemOutput, AVPlayerItemOutputPushDelegate,
    AVPlayerItemVideoOutput, AVURLAsset, AVURLAssetOverrideMIMETypeKey,
    NSValueAVFoundationExtensions,
};
use objc2_core_foundation::{CGAffineTransform, CGSize};
use objc2_core_media::{CMTime, kCMTimeZero};
use objc2_core_video::{
    kCVPixelBufferIOSurfacePropertiesKey, kCVPixelBufferMetalCompatibilityKey,
    kCVPixelBufferPixelFormatTypeKey, kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
};
use objc2_foundation::{
    NSArray, NSAttributedString, NSData, NSDictionary, NSNotification, NSNotificationCenter,
    NSNumber, NSObjectProtocol, NSString, NSURL,
};

const SEEK_TIMESCALE: i32 = 1_000_000;

pub(super) const OUTPUT_PIXEL_FORMATS: [u32; 4] = [
    kCVPixelFormatType_420YpCbCr8BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr8BiPlanarFullRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarVideoRange,
    kCVPixelFormatType_420YpCbCr10BiPlanarFullRange,
];

pub(super) struct LoadedSubtitle {
    pub(super) id: String,
    pub(super) language: Option<String>,
    pub(super) label: Option<String>,
    pub(super) track: SubtitleTrack,
}

pub(super) struct OpenedAsset {
    pub(super) asset: Retained<AVURLAsset>,
    pub(super) _memory_delegate: Option<Retained<MemoryResourceDelegate>>,
}

pub(super) struct ItemSignal {
    pub(super) ended: AtomicBool,
    pub(super) failed: Mutex<Option<String>>,
    pub(super) wake_lock: Arc<Mutex<()>>,
    pub(super) wake_cv: Arc<Condvar>,
}

impl ItemSignal {
    pub(super) fn new(wake_lock: Arc<Mutex<()>>, wake_cv: Arc<Condvar>) -> Arc<Self> {
        Arc::new(Self {
            ended: AtomicBool::new(false),
            failed: Mutex::new(None),
            wake_lock,
            wake_cv,
        })
    }

    fn notify(&self) {
        let _guard = self
            .wake_lock
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        self.wake_cv.notify_all();
    }
}

pub(super) struct MemoryDelegateIvars {
    bytes: Arc<[u8]>,
    content_type: String,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeMemoryResourceDelegate"]
    #[ivars = MemoryDelegateIvars]
    pub(super) struct MemoryResourceDelegate;

    unsafe impl NSObjectProtocol for MemoryResourceDelegate {}

    unsafe impl AVAssetResourceLoaderDelegate for MemoryResourceDelegate {
        #[unsafe(method(resourceLoader:shouldWaitForLoadingOfRequestedResource:))]
        fn should_wait(
            &self,
            _resource_loader: &objc2_av_foundation::AVAssetResourceLoader,
            loading_request: &AVAssetResourceLoadingRequest,
        ) -> bool {
            let ivars = self.ivars();
            let total_len = ivars.bytes.len();
            unsafe {
                if let Some(info) = loading_request.contentInformationRequest() {
                    let uti = NSString::from_str(&ivars.content_type);
                    info.setContentType(Some(&uti));
                    info.setContentLength(total_len as i64);
                    info.setByteRangeAccessSupported(true);
                    info.setEntireLengthAvailableOnDemand(true);
                }
                if let Some(data_req) = loading_request.dataRequest() {
                    let offset =
                        usize::try_from(data_req.requestedOffset().max(0)).unwrap_or(total_len);
                    let start = offset.min(total_len);
                    let end = if data_req.requestsAllDataToEndOfResource() {
                        total_len
                    } else {
                        let req_len =
                            usize::try_from(data_req.requestedLength().max(0)).unwrap_or(0);
                        start.saturating_add(req_len).min(total_len)
                    };
                    if start < end {
                        let slice = &ivars.bytes[start..end];
                        let ns_data = NSData::with_bytes(slice);
                        data_req.respondWithData(&ns_data);
                    }
                }
                loading_request.finishLoading();
            }
            true
        }
    }
);

impl MemoryResourceDelegate {
    fn new(bytes: Arc<[u8]>, content_type: String) -> Retained<Self> {
        let this = Self::alloc().set_ivars(MemoryDelegateIvars {
            bytes,
            content_type,
        });
        unsafe { msg_send![super(this), init] }
    }
}

pub(super) struct ItemObserverIvars {
    signal: Arc<ItemSignal>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeVideoItemObserver"]
    #[ivars = ItemObserverIvars]
    pub(super) struct ItemObserver;

    unsafe impl NSObjectProtocol for ItemObserver {}

    impl ItemObserver {
        #[unsafe(method(cranposeVideoItemDidPlayToEnd:))]
        fn did_play_to_end(&self, _notification: &NSNotification) {
            self.ivars().signal.ended.store(true, Ordering::Release);
            self.ivars().signal.notify();
        }

        #[unsafe(method(cranposeVideoItemFailedToPlayToEnd:))]
        fn failed_to_play_to_end(&self, _notification: &NSNotification) {
            *self
                .ivars()
                .signal
                .failed
                .lock()
                .unwrap_or_else(PoisonError::into_inner) =
                Some("playback failed before reaching end of item".to_owned());
            self.ivars().signal.notify();
        }
    }
);

impl ItemObserver {
    pub(super) fn attach(item: &AVPlayerItem, signal: Arc<ItemSignal>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(ItemObserverIvars { signal });
        let observer: Retained<Self> = unsafe { msg_send![super(this), init] };
        unsafe {
            let center = NSNotificationCenter::defaultCenter();
            let scope: &AnyObject = item.as_ref();
            center.addObserver_selector_name_object(
                &observer,
                sel!(cranposeVideoItemDidPlayToEnd:),
                Some(AVPlayerItemDidPlayToEndTimeNotification),
                Some(scope),
            );
            center.addObserver_selector_name_object(
                &observer,
                sel!(cranposeVideoItemFailedToPlayToEnd:),
                Some(AVPlayerItemFailedToPlayToEndTimeNotification),
                Some(scope),
            );
        }
        observer
    }

    pub(super) fn detach(&self) {
        unsafe {
            NSNotificationCenter::defaultCenter().removeObserver(self);
        }
    }
}

pub(super) struct LegibleIvars {
    sink: PlayerSink,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "CranposeLegibleOutputDelegate"]
    #[ivars = LegibleIvars]
    pub(super) struct LegibleDelegate;

    unsafe impl NSObjectProtocol for LegibleDelegate {}

    unsafe impl AVPlayerItemOutputPushDelegate for LegibleDelegate {
        #[unsafe(method(outputSequenceWasFlushed:))]
        fn output_flushed(&self, _output: &AVPlayerItemOutput) {
            self.ivars().sink.set_cues(CueGroup::EMPTY);
        }
    }

    unsafe impl AVPlayerItemLegibleOutputPushDelegate for LegibleDelegate {
        #[unsafe(method(legibleOutput:didOutputAttributedStrings:nativeSampleBuffers:forItemTime:))]
        fn did_output(
            &self,
            _output: &AVPlayerItemLegibleOutput,
            strings: &NSArray<NSAttributedString>,
            _native_samples: &NSArray,
            item_time: CMTime,
        ) {
            let pts = cm_time_to_duration(item_time).unwrap_or(Duration::ZERO);
            let mut cues = Vec::new();
            for attributed in strings {
                let text = attributed.string().to_string();
                let trimmed = text.trim();
                if !trimmed.is_empty() {
                    cues.push(Cue {
                        text: trimmed.to_owned(),
                        line: None,
                        position: None,
                        size: None,
                        text_alignment: CueAlignment::Center,
                        start_time: pts,
                        end_time: pts.saturating_add(Duration::from_secs(5)),
                    });
                }
            }
            self.ivars().sink.set_cues(CueGroup {
                cues,
                presentation_time: pts,
            });
        }
    }
);

unsafe impl Send for LegibleDelegate {}
unsafe impl Sync for LegibleDelegate {}

impl LegibleDelegate {
    pub(super) fn attach(
        item: &AVPlayerItem,
        sink: PlayerSink,
    ) -> (Retained<AVPlayerItemLegibleOutput>, Retained<Self>) {
        let this = Self::alloc().set_ivars(LegibleIvars { sink });
        let delegate: Retained<Self> = unsafe { msg_send![super(this), init] };
        let output = unsafe {
            let output = AVPlayerItemLegibleOutput::init(AVPlayerItemLegibleOutput::alloc());
            let proto: &ProtocolObject<dyn AVPlayerItemLegibleOutputPushDelegate> =
                ProtocolObject::from_ref(&*delegate);
            let queue = DispatchQueue::new("cranpose.apple.player.legible", None);
            output.setDelegate_queue(Some(proto), Some(&queue));
            item.addOutput(&output);
            output
        };
        (output, delegate)
    }
}

pub(super) fn build_video_output() -> Retained<AVPlayerItemVideoOutput> {
    unsafe {
        let formats: Vec<Retained<NSNumber>> = OUTPUT_PIXEL_FORMATS
            .iter()
            .map(|&fmt| NSNumber::new_u32(fmt))
            .collect();
        let format_refs: Vec<&NSNumber> = formats.iter().map(Retained::as_ref).collect();
        let format_array = NSArray::from_slice(&format_refs);
        let metal_true = NSNumber::new_bool(true);
        let empty_io_surface: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices::<NSString>(&[], &[]);

        let key_fmt = cf_str_as_ns(kCVPixelBufferPixelFormatTypeKey);
        let key_metal = cf_str_as_ns(kCVPixelBufferMetalCompatibilityKey);
        let key_io = cf_str_as_ns(kCVPixelBufferIOSurfacePropertiesKey);
        let keys: [&NSString; 3] = [key_fmt, key_metal, key_io];
        let vals: [&AnyObject; 3] = [
            format_array.as_ref(),
            metal_true.as_ref(),
            empty_io_surface.as_ref(),
        ];
        let attrs = NSDictionary::from_slices(&keys, &vals);
        AVPlayerItemVideoOutput::initWithPixelBufferAttributes(
            AVPlayerItemVideoOutput::alloc(),
            Some(&attrs),
        )
    }
}

unsafe fn cf_str_as_ns(cf: &'static objc2_core_foundation::CFString) -> &'static NSString {
    let ptr: *const objc2_core_foundation::CFString = cf;
    // SAFETY: CFString and NSString are toll-free bridged immutable Foundation types.
    unsafe { &*ptr.cast::<NSString>() }
}

pub(super) fn open_asset(item: &MediaItem) -> Result<OpenedAsset, PlayerError> {
    if let Some(bytes) = &item.bytes {
        let uti = uti_for_mime(item.mime_type.as_deref());
        let url_str = NSString::from_str("cranpose-mem://item.mp4");
        let url = NSURL::URLWithString(&url_str)
            .ok_or_else(|| PlayerError::Source("invalid in-memory media URL".into()))?;
        let delegate = MemoryResourceDelegate::new(Arc::clone(bytes), uti.to_owned());
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, None) };
        unsafe {
            let loader = asset.resourceLoader();
            let proto: &ProtocolObject<dyn AVAssetResourceLoaderDelegate> =
                ProtocolObject::from_ref(&*delegate);
            let queue = DispatchQueue::new("cranpose.apple.player.mem", None);
            loader.setDelegate_queue(Some(proto), Some(&queue));
        }
        return Ok(OpenedAsset {
            asset,
            _memory_delegate: Some(delegate),
        });
    }

    let url = url_for(&item.uri)
        .ok_or_else(|| PlayerError::Source(format!("unsupported media URI: {}", item.uri)))?;
    let options = build_asset_options(item);
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&url, options.as_deref()) };
    Ok(OpenedAsset {
        asset,
        _memory_delegate: None,
    })
}

fn build_asset_options(item: &MediaItem) -> Option<Retained<NSDictionary<NSString, AnyObject>>> {
    if item.headers.is_empty() && item.mime_type.is_none() {
        return None;
    }
    let mut keys: Vec<Retained<NSString>> = Vec::new();
    let mut vals: Vec<Retained<AnyObject>> = Vec::new();
    if !item.headers.is_empty() {
        let h_keys: Vec<Retained<NSString>> = item
            .headers
            .iter()
            .map(|(k, _)| NSString::from_str(k))
            .collect();
        let h_vals: Vec<Retained<NSString>> = item
            .headers
            .iter()
            .map(|(_, v)| NSString::from_str(v))
            .collect();
        let h_k_refs: Vec<&NSString> = h_keys.iter().map(Retained::as_ref).collect();
        let h_v_refs: Vec<&NSString> = h_vals.iter().map(Retained::as_ref).collect();
        let header_dict = NSDictionary::from_slices(&h_k_refs, &h_v_refs);
        keys.push(NSString::from_str("AVURLAssetHTTPHeaderFieldsKey"));
        vals.push(Retained::into_super(Retained::into_super(header_dict)));
    }
    if let Some(mime) = &item.mime_type {
        let mime_ns = NSString::from_str(mime);
        let key_ref = unsafe { AVURLAssetOverrideMIMETypeKey };
        keys.push(NSString::from_str(&key_ref.to_string()));
        vals.push(Retained::into_super(Retained::into_super(mime_ns)));
    }
    let k_refs: Vec<&NSString> = keys.iter().map(Retained::as_ref).collect();
    let v_refs: Vec<&AnyObject> = vals.iter().map(|v| &**v).collect();
    Some(NSDictionary::from_slices(&k_refs, &v_refs))
}

fn url_for(uri: &str) -> Option<Retained<NSURL>> {
    if let Some(path) = cranpose_services::media::path_from_uri(uri) {
        let text = path.to_str()?;
        return Some(NSURL::fileURLWithPath(&NSString::from_str(text)));
    }
    let (scheme, rest) = uri.split_once("://")?;
    if rest.is_empty()
        || (!scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https"))
    {
        return None;
    }
    NSURL::URLWithString(&NSString::from_str(uri))
}

fn uti_for_mime(mime: Option<&str>) -> &'static str {
    match mime.map(str::to_ascii_lowercase).as_deref() {
        Some("video/quicktime") => "com.apple.quicktime-movie",
        Some("video/mp2t") => "public.mpeg-2-transport-stream",
        Some("audio/mpeg") => "public.mp3",
        Some("audio/mp4" | "audio/aac") => "public.mpeg-4-audio",
        _ => "public.mpeg-4",
    }
}

pub(super) fn load_sidecar_subtitles(item: &MediaItem) -> Vec<LoadedSubtitle> {
    let mut loaded = Vec::new();
    for (idx, cfg) in item.subtitle_configurations.iter().enumerate() {
        let Some(path) = cranpose_services::media::path_from_uri(&cfg.uri) else {
            continue;
        };
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let hint = cfg.mime_type.as_deref().or(Some(cfg.uri.as_str()));
        let track = parse_subtitles(hint, &text);
        if !track.is_empty() {
            loaded.push(LoadedSubtitle {
                id: format!("sidecar-text-{idx}"),
                language: cfg.language.clone(),
                label: cfg.label.clone(),
                track,
            });
        }
    }
    loaded
}

pub(super) fn cm_time(position: Duration) -> CMTime {
    unsafe { CMTime::with_seconds(position.as_secs_f64(), SEEK_TIMESCALE) }
}

pub(super) fn cm_time_zero() -> CMTime {
    unsafe { kCMTimeZero }
}

pub(super) fn cm_time_to_duration(time: CMTime) -> Option<Duration> {
    let secs = unsafe { time.seconds() };
    (secs.is_finite() && secs >= 0.0).then(|| Duration::from_secs_f64(secs))
}

pub(super) fn rotation_from_transform(t: CGAffineTransform) -> VideoRotation {
    let angle = t.b.atan2(t.a).to_degrees().rem_euclid(360.0);
    if (45.0..135.0).contains(&angle) {
        VideoRotation::Degrees90
    } else if (135.0..225.0).contains(&angle) {
        VideoRotation::Degrees180
    } else if (225.0..315.0).contains(&angle) {
        VideoRotation::Degrees270
    } else {
        VideoRotation::Degrees0
    }
}

pub(super) fn buffered_position_of(item: &AVPlayerItem) -> Duration {
    let mut furthest = Duration::ZERO;
    unsafe {
        let ranges = item.loadedTimeRanges();
        for value in ranges {
            let range = value.CMTimeRangeValue();
            let end = range.start.seconds() + range.duration.seconds();
            if end.is_finite() && end > 0.0 {
                furthest = furthest.max(Duration::from_secs_f64(end));
            }
        }
    }
    furthest
}

pub(super) fn probe_item_metadata(
    item: &AVPlayerItem,
    stated_duration: Option<Duration>,
    sidecars: &[LoadedSubtitle],
) -> (VideoSize, VideoRotation, MediaProperties, Tracks) {
    let asset: Retained<AVAsset> = unsafe { item.asset() };
    let (video_size, rotation, video_group) = probe_video_tracks(&asset);
    let duration = cm_time_to_duration(unsafe { item.duration() })
        .filter(|d| !d.is_zero())
        .or(stated_duration);
    let seekable_ranges = unsafe { item.seekableTimeRanges() };
    let has_seekable_range = !seekable_ranges.is_empty();
    let is_live = duration.is_none() && has_seekable_range;
    let properties = MediaProperties {
        duration,
        is_live,
        is_seekable: has_seekable_range || duration.is_some(),
    };

    let mut groups = Vec::new();
    if let Some(vg) = video_group {
        groups.push(vg);
    }
    if let Some(ag) = probe_selection_group(&asset, item, TrackType::Audio, "audio") {
        groups.push(ag);
    } else if let Some(ag) = probe_basic_audio_tracks(&asset) {
        groups.push(ag);
    }
    if let Some(tg) = probe_selection_group(&asset, item, TrackType::Text, "text") {
        groups.push(tg);
    }
    if !sidecars.is_empty() {
        let entries = sidecars
            .iter()
            .enumerate()
            .map(|(i, s)| TrackEntry {
                format: TrackFormat {
                    id: s.id.clone(),
                    mime_type: Some("text/vtt".into()),
                    language: s.language.clone(),
                    label: s.label.clone(),
                    ..TrackFormat::default()
                },
                supported: true,
                selected: i == 0,
            })
            .collect();
        groups.push(TrackGroup {
            id: "sidecar-text".into(),
            track_type: TrackType::Text,
            tracks: entries,
            adaptive_supported: false,
        });
    }

    (video_size, rotation, properties, Tracks { groups })
}

fn non_negative_u32(value: f64) -> u32 {
    if !value.is_finite() || value <= 0.0 {
        return 0;
    }
    let clamped = value.round().clamp(0.0, f64::from(u32::MAX));
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to finite 0..=u32::MAX; budgeted lints only active under clippy_budget"
    )]
    {
        clamped as u32
    }
}

fn probe_video_tracks(asset: &AVAsset) -> (VideoSize, VideoRotation, Option<TrackGroup>) {
    let Some(video_type) = (unsafe { AVMediaTypeVideo }) else {
        return (VideoSize::ZERO, VideoRotation::Degrees0, None);
    };
    #[expect(
        deprecated,
        reason = "synchronous track probe runs on the off-UI worker after AVPlayerItemStatus::ReadyToPlay"
    )]
    let tracks = unsafe { asset.tracksWithMediaType(video_type) };
    let mut video_size = VideoSize::ZERO;
    let mut rotation = VideoRotation::Degrees0;
    let mut entries = Vec::new();
    for (idx, track) in tracks.iter().enumerate() {
        let natural = unsafe { track.naturalSize() };
        let transform = unsafe { track.preferredTransform() };
        let fps = unsafe { track.nominalFrameRate() };
        let bitrate = unsafe { track.estimatedDataRate() };
        let width = non_negative_u32(natural.width);
        let height = non_negative_u32(natural.height);
        let rot = rotation_from_transform(transform);
        if idx == 0 && width > 0 && height > 0 {
            rotation = rot;
            video_size = VideoSize {
                width,
                height,
                unapplied_rotation_degrees: u32::from(rot.degrees()),
                pixel_width_height_ratio: 1.0,
            };
        }
        entries.push(TrackEntry {
            format: TrackFormat {
                id: format!("video-{idx}"),
                width: (width > 0).then_some(width),
                height: (height > 0).then_some(height),
                frame_rate: (fps.is_finite() && fps > 0.0).then_some(fps),
                bitrate: (bitrate.is_finite() && bitrate > 0.0)
                    .then(|| non_negative_u32(f64::from(bitrate))),
                ..TrackFormat::default()
            },
            supported: true,
            selected: idx == 0,
        });
    }
    let group = (!entries.is_empty()).then(|| TrackGroup {
        id: "video".into(),
        track_type: TrackType::Video,
        tracks: entries,
        adaptive_supported: true,
    });
    (video_size, rotation, group)
}

fn probe_basic_audio_tracks(asset: &AVAsset) -> Option<TrackGroup> {
    let audio_type = unsafe { AVMediaTypeAudio }?;
    #[expect(
        deprecated,
        reason = "synchronous track probe runs on the off-UI worker after AVPlayerItemStatus::ReadyToPlay"
    )]
    let tracks = unsafe { asset.tracksWithMediaType(audio_type) };
    if tracks.is_empty() {
        return None;
    }
    let entries = tracks
        .iter()
        .enumerate()
        .map(|(idx, track)| {
            let lang = unsafe { track.languageCode() }.map(|s| s.to_string());
            let bitrate = unsafe { track.estimatedDataRate() };
            TrackEntry {
                format: TrackFormat {
                    id: format!("audio-{idx}"),
                    language: lang,
                    bitrate: (bitrate.is_finite() && bitrate > 0.0)
                        .then(|| non_negative_u32(f64::from(bitrate))),
                    ..TrackFormat::default()
                },
                supported: true,
                selected: idx == 0,
            }
        })
        .collect();
    Some(TrackGroup {
        id: "audio".into(),
        track_type: TrackType::Audio,
        tracks: entries,
        adaptive_supported: false,
    })
}

fn probe_selection_group(
    asset: &AVAsset,
    item: &AVPlayerItem,
    track_type: TrackType,
    group_id: &str,
) -> Option<TrackGroup> {
    let group = selection_group(asset, track_type)?;
    let options = unsafe { group.options() };
    if options.is_empty() {
        return None;
    }
    let selection = unsafe { item.currentMediaSelection() };
    let selected_opt = unsafe { selection.selectedMediaOptionInMediaSelectionGroup(&group) };
    let entries = options
        .iter()
        .enumerate()
        .map(|(idx, opt)| {
            let language = unsafe { opt.extendedLanguageTag() }.map(|s| s.to_string());
            let label = Some(unsafe { opt.displayName() }.to_string());
            let selected = selected_opt
                .as_ref()
                .is_some_and(|sel| std::ptr::eq(Retained::as_ptr(sel), &*opt));
            TrackEntry {
                format: TrackFormat {
                    id: format!("{group_id}-{idx}"),
                    language,
                    label,
                    ..TrackFormat::default()
                },
                supported: true,
                selected,
            }
        })
        .collect();
    Some(TrackGroup {
        id: group_id.into(),
        track_type,
        tracks: entries,
        adaptive_supported: false,
    })
}

fn selection_group(
    asset: &AVAsset,
    track_type: TrackType,
) -> Option<Retained<AVMediaSelectionGroup>> {
    let characteristic = match track_type {
        TrackType::Audio => unsafe { AVMediaCharacteristicAudible }?,
        TrackType::Text => unsafe { AVMediaCharacteristicLegible }?,
        TrackType::Video => return None,
    };
    #[expect(
        deprecated,
        reason = "synchronous selection-group probe runs on the off-UI worker after ReadyToPlay"
    )]
    unsafe {
        asset.mediaSelectionGroupForMediaCharacteristic(characteristic)
    }
}

pub(super) fn apply_track_selection(
    item: &AVPlayerItem,
    params: &TrackSelectionParameters,
    sidecars: &[LoadedSubtitle],
) -> Option<usize> {
    unsafe {
        let bitrate = f64::from(params.max_video_bitrate.unwrap_or(0));
        item.setPreferredPeakBitRate(bitrate);
        let max_res = match (params.max_video_width, params.max_video_height) {
            (Some(w), Some(h)) => CGSize::new(f64::from(w), f64::from(h)),
            _ => CGSize::new(0.0, 0.0),
        };
        item.setPreferredMaximumResolution(max_res);
    }
    let asset = unsafe { item.asset() };
    apply_group_selection(&asset, item, TrackType::Audio, "audio", params);
    apply_group_selection(&asset, item, TrackType::Text, "text", params);

    if sidecars.is_empty() || params.is_type_disabled(TrackType::Text) {
        return None;
    }
    if let Some(ov) = params
        .overrides
        .iter()
        .find(|o| o.group_id == "sidecar-text")
    {
        return ov
            .track_indices
            .first()
            .copied()
            .filter(|&i| i < sidecars.len());
    }
    for pref in &params.preferred_text_languages {
        if let Some(idx) = sidecars.iter().position(|s| {
            s.language
                .as_deref()
                .is_some_and(|lang| lang.eq_ignore_ascii_case(pref))
        }) {
            return Some(idx);
        }
    }
    Some(0)
}

fn apply_group_selection(
    asset: &AVAsset,
    item: &AVPlayerItem,
    track_type: TrackType,
    group_id: &str,
    params: &TrackSelectionParameters,
) {
    let Some(group) = selection_group(asset, track_type) else {
        return;
    };
    if params.is_type_disabled(track_type) {
        unsafe { item.selectMediaOption_inMediaSelectionGroup(None, &group) };
        return;
    }
    let options = unsafe { group.options() };
    if let Some(ov) = params.overrides.iter().find(|o| o.group_id == group_id) {
        let opt = ov
            .track_indices
            .first()
            .and_then(|&idx| (idx < options.len()).then(|| options.objectAtIndex(idx)));
        unsafe { item.selectMediaOption_inMediaSelectionGroup(opt.as_deref(), &group) };
        return;
    }
    let preferred = match track_type {
        TrackType::Audio => &params.preferred_audio_languages,
        TrackType::Text => &params.preferred_text_languages,
        TrackType::Video => return,
    };
    for lang in preferred {
        for opt in &options {
            let tag = unsafe { opt.extendedLanguageTag() }.map(|s| s.to_string());
            if tag.as_deref().is_some_and(|t| t.eq_ignore_ascii_case(lang)) {
                unsafe { item.selectMediaOption_inMediaSelectionGroup(Some(&opt), &group) };
                return;
            }
        }
    }
}
