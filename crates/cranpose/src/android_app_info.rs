use std::rc::Rc;

use cranpose_services::app_info::{AppInfo, set_platform_app_info};
use jni::{jni_sig, jni_str, objects::JString};

struct AndroidAppInfo {
    version_name: Option<String>,
    build_version: Option<String>,
}

impl AppInfo for AndroidAppInfo {
    fn version_name(&self) -> Option<String> {
        self.version_name.clone()
    }

    fn build_version(&self) -> Option<String> {
        self.build_version.clone()
    }
}

pub(crate) fn install_app_info(app: &android_activity::AndroidApp) {
    match query_app_info(app) {
        Ok((version_name, build_version)) => {
            set_platform_app_info(Rc::new(AndroidAppInfo {
                version_name,
                build_version,
            }));
        }
        Err(error) => {
            log::warn!("[android-app-info] could not read the packaged version: {error}");
        }
    }
}

fn query_app_info(
    app: &android_activity::AndroidApp,
) -> Result<(Option<String>, Option<String>), String> {
    crate::android_jni::with_android_activity_env(app, |env, activity| {
        let describe = |env: &mut jni::Env<'_>, what: &str, error: jni::errors::Error| {
            crate::android_jni::clear_pending_android_jni_exception(env);
            format!("{what} failed: {error}")
        };
        let manager = env
            .call_method(
                &activity,
                jni_str!("getPackageManager"),
                jni_sig!("()Landroid/content/pm/PackageManager;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Activity.getPackageManager", error))?;
        let package = env
            .call_method(
                &activity,
                jni_str!("getPackageName"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "Activity.getPackageName", error))?;
        let info = env
            .call_method(
                &manager,
                jni_str!("getPackageInfo"),
                jni_sig!("(Ljava/lang/String;I)Landroid/content/pm/PackageInfo;"),
                &[(&package).into(), 0i32.into()],
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "PackageManager.getPackageInfo", error))?;

        let name_object = env
            .get_field(
                &info,
                jni_str!("versionName"),
                jni_sig!("Ljava/lang/String;"),
            )
            .and_then(|value| value.l())
            .map_err(|error| describe(env, "PackageInfo.versionName", error))?;
        let version_name = if name_object.is_null() {
            None
        } else {
            JString::cast_local(env, name_object)
                .and_then(|text| text.try_to_string(env))
                .ok()
        };

        let build_version = env
            .call_method(&info, jni_str!("getLongVersionCode"), jni_sig!("()J"), &[])
            .and_then(|value| value.j())
            .or_else(|_| {
                crate::android_jni::clear_pending_android_jni_exception(env);
                env.get_field(&info, jni_str!("versionCode"), jni_sig!("I"))
                    .and_then(|value| value.i())
                    .map(i64::from)
            })
            .ok()
            .map(|version| version.to_string());

        Ok((version_name, build_version))
    })
}
