//! Run-time permissions that differ between devices, through
//! `CranposePermissions`, which an application carries only when its build
//! script declares a service that asks for them, such as
//! `Use::hand_tracking()`.

use jni::{jni_sig, jni_str, objects::JValue};

use super::jni::call_cranpose_class;

const PERMISSIONS_CLASS: &str = "dev/cranpose/android/CranposePermissions";

/// What the person has answered for a group of alternative Android permissions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AndroidPermission {
    /// No prompt was answered yet, or the system cannot be asked.
    NotAsked,
    /// One of the permissions is granted.
    Granted,
    /// The person refused. Android shows its prompt once more at most.
    Refused,
    /// The person refused for good: Android shows no prompt any more, and only the application's
    /// page in the system settings can allow it.
    Blocked,
}

impl AndroidPermission {
    // The answer `CranposePermissions.cranposePermissionState` gives: 0 not asked, 1 granted,
    // 2 refused, 3 refused for good.
    pub(crate) const fn from_state(state: i32) -> Self {
        match state {
            1 => Self::Granted,
            2 => Self::Refused,
            3 => Self::Blocked,
            _ => Self::NotAsked,
        }
    }
}

/// The answer for a group of `alternatives`, permissions of which any one satisfies the group,
/// such as the same permission under the names of two headsets' systems. Returns
/// [`AndroidPermission::NotAsked`] when the application's build carries no `CranposePermissions`,
/// which only a declared service brings.
pub fn android_permission(
    app: &android_activity::AndroidApp,
    alternatives: &[&str],
) -> AndroidPermission {
    let names = alternatives.join("\n");
    let state = call_cranpose_class(app, PERMISSIONS_CLASS, |env, activity, class| {
        let names = env.new_string(&names)?;
        let names: &jni::objects::JObject = names.as_ref();
        env.call_static_method(
            class,
            jni_str!("cranposePermissionState"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)I"),
            &[JValue::Object(activity), JValue::Object(names)],
        )
        .and_then(jni::JValueOwned::i)
    });
    match state {
        Ok(state) => AndroidPermission::from_state(state),
        Err(error) => {
            log::warn!("cranpose: the permission {alternatives:?} could not be read: {error}");
            AndroidPermission::NotAsked
        }
    }
}

/// Shows the system's prompt for those of `permissions` that the application does not hold. A
/// permission that the device does not define is refused with no prompt, so a list may name the
/// permissions of several headsets. The answer reaches [`android_permission`].
pub fn request_android_permissions(app: &android_activity::AndroidApp, permissions: &[&str]) {
    let names = permissions.join("\n");
    let shown = call_cranpose_class(app, PERMISSIONS_CLASS, |env, activity, class| {
        let names = env.new_string(&names)?;
        let names: &jni::objects::JObject = names.as_ref();
        env.call_static_method(
            class,
            jni_str!("cranposeRequestPermissions"),
            jni_sig!("(Landroid/app/Activity;Ljava/lang/String;)V"),
            &[JValue::Object(activity), JValue::Object(names)],
        )
        .and_then(jni::JValueOwned::v)
    });
    if let Err(error) = shown {
        log::warn!("cranpose: the permissions {permissions:?} were not requested: {error}");
    }
}
