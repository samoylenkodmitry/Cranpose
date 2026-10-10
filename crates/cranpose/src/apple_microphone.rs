#![expect(unsafe_code)]

//! The microphone permission and the recording session on iOS and watchOS.

use std::sync::Arc;

use block2::RcBlock;
use cranpose_services::{
    MicrophoneAccess, MicrophonePermission, publish_microphone_permission,
    set_platform_microphone_access,
};
use dispatch2::DispatchQueue;
use objc2::runtime::Bool;
#[cfg(target_os = "ios")]
use objc2::{MainThreadMarker, rc::Retained, runtime::AnyObject};
use objc2_avf_audio::{
    AVAudioSession, AVAudioSessionCategoryOptions, AVAudioSessionCategoryPlayAndRecord,
    AVAudioSessionModeDefault, AVAudioSessionRecordPermission,
};
#[cfg(target_os = "ios")]
use objc2_foundation::{NSDictionary, NSString, NSURL};
#[cfg(target_os = "ios")]
use objc2_ui_kit::{UIApplication, UIApplicationOpenSettingsURLString};

pub(crate) fn register() {
    set_platform_microphone_access(Some(Arc::new(AppleMicrophone)));
}

struct AppleMicrophone;

impl MicrophoneAccess for AppleMicrophone {
    // The session's permission API serves iOS 16 too; AVAudioApplication's
    // came in iOS 17 and watchOS 10.
    #[expect(deprecated)]
    fn permission(&self) -> MicrophonePermission {
        // SAFETY: the shared session is valid for the life of the process.
        let permission = unsafe { AVAudioSession::sharedInstance().recordPermission() };
        match permission {
            AVAudioSessionRecordPermission::Granted => MicrophonePermission::Granted,
            // The system asks once; after a refusal only the settings can allow it.
            AVAudioSessionRecordPermission::Denied => MicrophonePermission::Blocked,
            _ => MicrophonePermission::NotAsked,
        }
    }

    fn request(&self) {
        let answered = RcBlock::new(|_granted: Bool| {
            // The answer may come on any thread; observers read it on the main one.
            DispatchQueue::main().exec_async(publish_microphone_permission);
        });
        // SAFETY: the block outlives the call; the system copies it until the
        // person answers.
        #[expect(deprecated)]
        unsafe {
            AVAudioSession::sharedInstance().requestRecordPermission(&answered);
        }
    }

    /// The app's page of the iPhone settings. A watch has no settings page
    /// that an app can open.
    fn open_settings(&self) {
        #[cfg(target_os = "ios")]
        let open = || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            // SAFETY: the constant is UIKit's own string for this app's settings.
            let Some(url) = NSURL::URLWithString(unsafe { UIApplicationOpenSettingsURLString })
            else {
                return;
            };
            let options: Retained<NSDictionary<NSString, AnyObject>> = NSDictionary::new();
            // SAFETY: opening a URL on the main thread with no completion handler.
            unsafe {
                UIApplication::sharedApplication(mtm)
                    .openURL_options_completionHandler(&url, &options, None);
            }
        };
        #[cfg(target_os = "ios")]
        if MainThreadMarker::new().is_some() {
            open();
        } else {
            DispatchQueue::main().exec_async(open);
        }
    }

    fn prepare_recording(&self) -> Result<(), String> {
        // SAFETY: the shared session and the category and mode constants are
        // valid for the life of the process.
        unsafe {
            let session = AVAudioSession::sharedInstance();
            let (Some(category), Some(mode)) = (
                AVAudioSessionCategoryPlayAndRecord,
                AVAudioSessionModeDefault,
            ) else {
                return Err("the audio session names are missing".into());
            };
            // Other apps' audio keeps playing; on a phone, Bluetooth headsets
            // can record.
            let options = AVAudioSessionCategoryOptions::MixWithOthers;
            #[cfg(target_os = "ios")]
            let options = options | AVAudioSessionCategoryOptions::AllowBluetoothHFP;
            session
                .setCategory_mode_options_error(category, mode, options)
                .map_err(|error| format!("audio session category: {error}"))?;
            session
                .setActive_error(true)
                .map_err(|error| format!("audio session: {error}"))
        }
    }
}
