//! The entitlement check of the Meta Horizon Store, through `CranposeMetaEntitlement`, which an
//! application carries only when its Gradle build lists the `meta-entitlement` service. The
//! service brings the Meta Platform SDK, which is under the Meta Platform Technologies SDK
//! License Agreement.

use jni::{jni_sig, jni_str, objects::JValue};

use super::jni::call_cranpose_class;
pub use super::meta_entitlement_wire::MetaEntitlement;

const ENTITLEMENT_CLASS: &str = "dev/cranpose/android/CranposeMetaEntitlement";

/// Starts the check for the process: the Platform SDK initializes with the app id of the
/// manifest's `com.meta.horizon.platform.ovr.OCULUS_APP_ID`. Then it tells whether the viewer is
/// entitled. A call while the check runs, or after it found the viewer entitled, changes nothing.
/// A call after a failed check starts the check again. Returns whether the Java call landed. Read
/// the answer with [`meta_entitlement`].
///
/// The application must end when the answer is [`MetaEntitlement::NotEntitled`], as Meta's rules
/// for store apps need, or limit itself, as Meta's guide allows.
pub fn begin_meta_entitlement(app: &android_activity::AndroidApp) -> bool {
    let started = call_cranpose_class(app, ENTITLEMENT_CLASS, |env, activity, class| {
        env.call_static_method(
            class,
            jni_str!("cranposeMetaEntitlementBegin"),
            jni_sig!("(Landroid/app/Activity;)V"),
            &[JValue::Object(activity)],
        )
        .map(|_| ())
    });
    match started {
        Ok(()) => true,
        Err(error) => {
            log::warn!(
                "the Meta entitlement check cannot start; list the meta-entitlement service in \
                 the Gradle build: {error}"
            );
            false
        }
    }
}

/// Where the check stands.
pub fn meta_entitlement(app: &android_activity::AndroidApp) -> MetaEntitlement {
    let state = call_cranpose_class(app, ENTITLEMENT_CLASS, |env, _, class| {
        env.call_static_method(
            class,
            jni_str!("cranposeMetaEntitlementState"),
            jni_sig!("()I"),
            &[],
        )
        .and_then(jni::JValueOwned::i)
    });
    state.map_or(MetaEntitlement::Unavailable, MetaEntitlement::from_state)
}

/// Why the check ended as it did, for a log. It is empty while the check runs and when the
/// application's build carries no `CranposeMetaEntitlement`.
pub fn meta_entitlement_detail(app: &android_activity::AndroidApp) -> String {
    call_cranpose_class(app, ENTITLEMENT_CLASS, |env, _, class| {
        let text = env
            .call_static_method(
                class,
                jni_str!("cranposeMetaEntitlementDetail"),
                jni_sig!("()Ljava/lang/String;"),
                &[],
            )
            .and_then(jni::JValueOwned::l)?;
        let text = env.cast_local::<jni::objects::JString>(text)?;
        text.try_to_string(env)
    })
    .unwrap_or_default()
}
