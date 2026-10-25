#![expect(unsafe_code)]
use std::{
    path::PathBuf,
    sync::{Arc, OnceLock},
};

use cranpose_services::{
    HostController, PlatformDirectories, set_application_id, set_host_controller,
};
use jni::{
    Env, EnvUnowned, jni_sig, jni_str,
    objects::{JObject, JString, JValue},
};

use super::jni::{AndroidContext, clear_pending_android_jni_exception, with_android_activity_env};

#[derive(Clone)]
struct AndroidHost {
    #[cfg(feature = "localization")]
    context: &'static AndroidContext,
    directories: Option<PlatformDirectories>,
    /// The activity once one runs; what needs a window waits for it.
    activity: Option<android_activity::AndroidApp>,
}

/// The host of a process before any activity runs.
static PROCESS_HOST: OnceLock<AndroidHost> = OnceLock::new();

impl AndroidHost {
    /// Calls the activity's `name()` method taking `args`, if an activity runs.
    fn call_activity(
        &self,
        name: &'static jni::strings::JNIStr,
        signature: jni::signature::MethodSignature<'static, 'static>,
        args: &[JValue<'_>],
    ) {
        let Some(app) = &self.activity else {
            return;
        };
        let _ = with_android_activity_env(app, |env, activity| {
            env.call_method(&activity, name, signature, args)
                .map(|_| ())
                .map_err(|e| {
                    clear_pending_android_jni_exception(env);
                    e.to_string()
                })
        });
    }
}

impl HostController for AndroidHost {
    fn set_keep_screen_on(&self, enabled: bool) {
        self.call_activity(
            jni_str!("cranposeSetKeepScreenOn"),
            jni_sig!("(Z)V"),
            &[enabled.into()],
        );
    }
    fn platform_directories(&self) -> Option<PlatformDirectories> {
        self.directories.clone()
    }
    fn exit(&self) {
        if let Some(app) = &self.activity {
            let _ = super::finish::finish_activity(app);
        }
    }
    fn background(&self) {
        self.call_activity(jni_str!("cranposeMoveToBackground"), jni_sig!("()V"), &[]);
    }

    #[cfg(feature = "localization")]
    fn preferred_languages(&self) -> Vec<String> {
        let result = self.context.with_env(|env, context| {
            let query = |env: &mut Env<'_>| -> jni::errors::Result<String> {
                let resources = env
                    .call_method(
                        context,
                        jni_str!("getResources"),
                        jni_sig!("()Landroid/content/res/Resources;"),
                        &[],
                    )?
                    .l()?;
                let config = env
                    .call_method(
                        &resources,
                        jni_str!("getConfiguration"),
                        jni_sig!("()Landroid/content/res/Configuration;"),
                        &[],
                    )?
                    .l()?;
                let locales = env
                    .call_method(
                        &config,
                        jni_str!("getLocales"),
                        jni_sig!("()Landroid/os/LocaleList;"),
                        &[],
                    )?
                    .l()?;
                let tags = env
                    .call_method(
                        &locales,
                        jni_str!("toLanguageTags"),
                        jni_sig!("()Ljava/lang/String;"),
                        &[],
                    )?
                    .l()?;
                JString::cast_local(env, tags)?.try_to_string(env)
            };
            query(env).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        });
        match result {
            Ok(tags) => tags
                .split(',')
                .filter(|tag| !tag.is_empty())
                .map(str::to_owned)
                .collect(),
            Err(error) => {
                log::warn!("could not read Android language preferences: {error}");
                Vec::new()
            }
        }
    }

    fn durable_save_deadline(&self) -> std::time::Duration {
        std::time::Duration::from_secs(2)
    }
}

/// What the process is packaged as and where its files live.
struct Packaged {
    package: String,
    data: Option<PathBuf>,
    shared: Option<PathBuf>,
}

fn read_packaged(context: &AndroidContext) -> Result<Packaged, String> {
    context.with_env(|env, context| {
        let read = |env: &mut Env<'_>| -> jni::errors::Result<Packaged> {
            let package = env
                .call_method(
                    context,
                    jni_str!("getPackageName"),
                    jni_sig!("()Ljava/lang/String;"),
                    &[],
                )?
                .l()?;
            let package = JString::cast_local(env, package)?.try_to_string(env)?;
            let files = env
                .call_method(
                    context,
                    jni_str!("getFilesDir"),
                    jni_sig!("()Ljava/io/File;"),
                    &[],
                )?
                .l()?;
            let external = env
                .call_method(
                    context,
                    jni_str!("getExternalFilesDir"),
                    jni_sig!("(Ljava/lang/String;)Ljava/io/File;"),
                    &[JValue::Object(&JObject::null())],
                )?
                .l()?;
            Ok(Packaged {
                package,
                data: absolute_path(env, &files)?,
                shared: absolute_path(env, &external)?,
            })
        };
        read(env).map_err(|error| {
            clear_pending_android_jni_exception(env);
            error.to_string()
        })
    })
}

/// The absolute path of the `java.io.File` `file`, when there is one.
fn absolute_path(env: &mut Env<'_>, file: &JObject<'_>) -> jni::errors::Result<Option<PathBuf>> {
    if file.is_null() {
        return Ok(None);
    }
    let path = env
        .call_method(
            file,
            jni_str!("getAbsolutePath"),
            jni_sig!("()Ljava/lang/String;"),
            &[],
        )?
        .l()?;
    Ok(Some(PathBuf::from(
        JString::cast_local(env, path)?.try_to_string(env)?,
    )))
}

fn directories(data: PathBuf, shared: Option<PathBuf>) -> PlatformDirectories {
    let sandbox = data.parent().unwrap_or(&data);
    let cache = sandbox.join("cache");
    PlatformDirectories {
        config: data.join("config"),
        documents: Some(sandbox.join("files").join("documents")),
        temporary: cache.join("temporary"),
        cache,
        data,
        shared,
    }
}

/// Installs the host of a process that may have no activity: the
/// application id and its folders.
pub(crate) fn install_process(context: &'static AndroidContext) {
    let (package, directories) = match read_packaged(context) {
        Ok(Packaged {
            package,
            data,
            shared,
        }) => (Some(package), data.map(|data| directories(data, shared))),
        Err(error) => {
            log::warn!("cranpose: the Android package is unreadable: {error}");
            (None, None)
        }
    };
    if let Some(directories) = &directories {
        // SAFETY: set while the process starts, before the application's own
        // threads read the environment.
        unsafe {
            std::env::set_var("XDG_DATA_HOME", &directories.data);
        }
    }
    if let Some(package) = package
        && let Err(error) = set_application_id(&package)
    {
        log::warn!("cranpose: the Android package name is not a usable application id: {error}");
    }
    let host = PROCESS_HOST.get_or_init(|| AndroidHost {
        #[cfg(feature = "localization")]
        context,
        directories,
        activity: None,
    });
    set_host_controller(Arc::new(host.clone()));
}

/// Gives the process host the activity `app` runs.
pub(crate) fn install_activity(app: android_activity::AndroidApp) {
    let Some(process) = PROCESS_HOST.get() else {
        log::error!("cranpose: the activity started before its process");
        return;
    };
    set_host_controller(Arc::new(AndroidHost {
        activity: Some(app),
        ..process.clone()
    }));
    crate::pipeline_cache_file::publish();
}

/// A step of a predictive back gesture (API 34+), for the app while it
/// intercepts back; otherwise the platform's own back takes it.
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnBackGesture(
    _env: EnvUnowned<'_>,
    _class: jni::objects::JClass<'_>,
    kind: jni::sys::jint,
    progress: jni::sys::jfloat,
    edge: jni::sys::jint,
) {
    use cranpose_services::{BackEdge, BackEvent, BackGesture};
    const STARTED: jni::sys::jint = 0;
    const PROGRESSED: jni::sys::jint = 1;
    // `BackEvent.EDGE_RIGHT`.
    const EDGE_RIGHT: jni::sys::jint = 1;
    if !cranpose_services::back_interception_enabled() {
        return;
    }
    let event = BackEvent {
        progress,
        edge: if edge == EDGE_RIGHT {
            BackEdge::Right
        } else {
            BackEdge::Left
        },
    };
    cranpose_services::push_back_gesture(match kind {
        STARTED => BackGesture::Started(event),
        PROGRESSED => BackGesture::Progressed(event),
        _ => BackGesture::Cancelled,
    });
}

#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnBackInvoked(
    _env: EnvUnowned<'_>,
    _class: jni::objects::JClass<'_>,
) -> jni::sys::jboolean {
    if cranpose_services::back_interception_enabled() {
        cranpose_services::push_back_request();
        true
    } else {
        false
    }
}
