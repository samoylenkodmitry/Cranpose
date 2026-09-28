#![expect(unsafe_code)]

use std::sync::Arc;

use cranpose_services::{
    AppUpdateCapabilities, AppUpdateError, AppUpdateStatus, AppUpdater, GitHubReleaseUpdate,
    PackageDigest, UpdatePackage, set_platform_app_updater,
};
use jni::{
    Env, EnvUnowned, Outcome, jni_sig, jni_str,
    objects::{JClass, JObject, JString, JValue},
    sys::{jint, jlong},
};

use crate::{
    android_jni::{
        clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
    },
    android_services::wake_native_loop,
};

const UPDATE_CLASS: &str = "dev/cranpose/android/CranposeAppUpdate";

pub(crate) fn register(app: android_activity::AndroidApp) {
    set_platform_app_updater(Arc::new(AndroidAppUpdater { app }));
}

struct AndroidAppUpdater {
    app: android_activity::AndroidApp,
}

impl AndroidAppUpdater {
    fn call(
        &self,
        run: impl for<'local> FnOnce(
            &mut Env<'local>,
            &JObject<'local>,
            JClass<'local>,
        ) -> jni::errors::Result<()>,
    ) -> Result<(), AppUpdateError> {
        with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, UPDATE_CLASS)?;
            run(env, &activity, class).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .map_err(AppUpdateError::Request)
    }
}

impl AppUpdater for AndroidAppUpdater {
    fn capabilities(&self) -> AppUpdateCapabilities {
        AppUpdateCapabilities {
            check: true,
            install: true,
        }
    }

    fn check(&self, source: &GitHubReleaseUpdate) -> Result<(), AppUpdateError> {
        self.call(|env, activity, class| {
            let repository = env.new_string(&source.repository)?;
            let current_version = env.new_string(&source.current_version)?;
            let asset_suffix = env.new_string(&source.asset_suffix)?;
            env.call_static_method(
                class,
                jni_str!("cranposeCheckGitHubUpdate"),
                jni_sig!(
                    "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;)V"
                ),
                &[
                    JValue::Object(activity),
                    JValue::Object(repository.as_ref()),
                    JValue::Object(current_version.as_ref()),
                    JValue::Object(asset_suffix.as_ref()),
                ],
            )
            .map(|_| ())
        })
    }

    fn install(&self, package: &UpdatePackage) -> Result<(), AppUpdateError> {
        let digest = package
            .digest
            .as_ref()
            .map(PackageDigest::to_feed_string)
            .unwrap_or_default();
        self.call(|env, activity, class| {
            let download_url = env.new_string(&package.download_url)?;
            let digest = env.new_string(&digest)?;
            env.call_static_method(
                class,
                jni_str!("cranposeInstallUpdate"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;J)V"),
                &[
                    JValue::Object(activity),
                    JValue::Object(download_url.as_ref()),
                    JValue::Object(digest.as_ref()),
                    JValue::Long(package.size.unwrap_or(0) as i64),
                ],
            )
            .map(|_| ())
        })
    }
}

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeAppUpdate_nativeOnAppUpdateStatus<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    kind: jint,
    version: JString<'local>,
    download_url: JString<'local>,
    downloaded: jlong,
    total: jlong,
    message: JString<'local>,
    digest: JString<'local>,
) {
    let decoded = env.with_env(
        |env| -> jni::errors::Result<(String, String, String, String)> {
            Ok((
                version.try_to_string(env)?,
                download_url.try_to_string(env)?,
                message.try_to_string(env)?,
                digest.try_to_string(env)?,
            ))
        },
    );
    let Outcome::Ok((version, download_url, message, digest)) = decoded.into_outcome() else {
        return;
    };
    let status = match kind {
        1 => AppUpdateStatus::Checking,
        2 => AppUpdateStatus::UpToDate,
        3 => {
            let mut package = UpdatePackage::new(version, download_url);
            if total > 0 {
                package = package.with_size(total as u64);
            }
            if let Some(digest) = PackageDigest::parse(&digest) {
                package = package.with_digest(digest);
            }
            AppUpdateStatus::Available { package }
        }
        4 => AppUpdateStatus::Downloading {
            downloaded: downloaded.max(0) as u64,
            total: (total > 0).then_some(total as u64),
        },
        5 => AppUpdateStatus::AwaitingConfirmation,
        6 => AppUpdateStatus::Installing,
        7 => AppUpdateStatus::Error(if message.is_empty() {
            "application update failed".to_string()
        } else {
            message
        }),
        8 => AppUpdateStatus::Verifying,
        _ => AppUpdateStatus::Idle,
    };
    cranpose_services::set_app_update_status(status);
    wake_native_loop();
}
