#![expect(unsafe_code)]

//! Answers to the permission requests services make. `CranposeActivity`
//! hands Rust every answer to a request it did not make itself, one
//! `permission<TAB>granted` line each, and each goes to the service that
//! asked.

use cranpose_services::{publish_heart_rate_permission, publish_microphone_permission};
use jni::{
    EnvUnowned, Outcome,
    objects::{JClass, JString},
};

use crate::{
    android_heart_rate::HEART_RATE_PERMISSIONS, android_microphone::MICROPHONE_PERMISSION,
};

#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeActivity_nativeOnPermissionsResult<
    'local,
>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    results: JString<'local>,
) {
    let decoded = env.with_env(|env| results.try_to_string(env));
    let Outcome::Ok(results) = decoded.into_outcome() else {
        return;
    };
    if let Some(granted) = answer(&results, &HEART_RATE_PERMISSIONS) {
        publish_heart_rate_permission(granted);
    }
    if answer(&results, &[MICROPHONE_PERMISSION]).is_some() {
        publish_microphone_permission();
    }
}

/// The answer among `results` to a request for one of `permissions`.
fn answer(results: &str, permissions: &[&str]) -> Option<bool> {
    results.lines().find_map(|line| {
        let (name, granted) = line.split_once('\t')?;
        permissions.contains(&name).then_some(granted == "1")
    })
}

#[cfg(test)]
#[path = "tests/android_permissions_tests.rs"]
mod tests;
