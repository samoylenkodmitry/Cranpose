#![expect(unsafe_code)]

//! The Wear OS Data Layer, through `CranposeWearable` and
//! `CranposeWearableService`, which an application carries only when it
//! declares the `wearable` service in its Gradle build.

use std::{
    io::{self, Read, Write},
    sync::Arc,
};

use cranpose_services::{
    WearableError, WearableEvent, WearableLink, WearablePeer, WearableStream,
    publish_wearable_event, set_platform_wearable_link,
};
use jni::{
    Env, EnvUnowned, JavaVM, Outcome, jni_sig, jni_str,
    objects::{Global, JByteArray, JClass, JObject, JString, JValue},
    sys::jbyte,
};

use crate::android_jni::{
    clear_pending_android_jni_exception, load_cranpose_java_class, with_android_activity_env,
};

const WEARABLE_CLASS: &str = "dev/cranpose/android/CranposeWearable";
const CHUNK: usize = 8192;

pub(crate) fn register(app: android_activity::AndroidApp) {
    let declared = with_android_activity_env(&app, |env, activity| {
        load_cranpose_java_class(env, &activity, WEARABLE_CLASS).map(|_| ())
    });
    if declared.is_ok() {
        set_platform_wearable_link(Some(Arc::new(AndroidWearableLink { app })));
    }
}

struct AndroidWearableLink {
    app: android_activity::AndroidApp,
}

impl AndroidWearableLink {
    fn call<T>(
        &self,
        run: impl for<'local> FnOnce(
            &mut Env<'local>,
            &JObject<'local>,
            JClass<'local>,
        ) -> jni::errors::Result<T>,
    ) -> Result<T, WearableError> {
        with_android_activity_env(&self.app, |env, activity| {
            let class = load_cranpose_java_class(env, &activity, WEARABLE_CLASS)?;
            run(env, &activity, class).map_err(|error| {
                clear_pending_android_jni_exception(env);
                error.to_string()
            })
        })
        .map_err(WearableError::Failed)
    }
}

impl WearableLink for AndroidWearableLink {
    fn peers(&self) -> Result<Vec<WearablePeer>, WearableError> {
        let listing = self.call(|env, activity, class| {
            let listing = env
                .call_static_method(
                    class,
                    jni_str!("peers"),
                    jni_sig!("(Landroid/content/Context;)Ljava/lang/String;"),
                    &[JValue::Object(activity)],
                )
                .and_then(jni::JValueOwned::l)?;
            env.cast_local::<JString>(listing)?.try_to_string(env)
        })?;
        Ok(listing
            .lines()
            .filter_map(|line| {
                let (id, name) = line.split_once('\t')?;
                Some(WearablePeer {
                    id: id.to_owned(),
                    name: name.to_owned(),
                })
            })
            .collect())
    }

    fn send_message(&self, peer: &str, path: &str, data: &[u8]) -> Result<(), WearableError> {
        self.call(|env, activity, class| {
            let peer = JObject::from(env.new_string(peer)?);
            let path = JObject::from(env.new_string(path)?);
            let data = JObject::from(env.byte_array_from_slice(data)?);
            env.call_static_method(
                class,
                jni_str!("sendMessage"),
                jni_sig!("(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;[B)V"),
                &[
                    JValue::Object(activity),
                    JValue::Object(&peer),
                    JValue::Object(&path),
                    JValue::Object(&data),
                ],
            )
            .map(|_| ())
        })
    }

    fn open_stream(&self, peer: &str, path: &str) -> Result<Box<dyn Write + Send>, WearableError> {
        let (stream, buffer) = self.call(|env, activity, class| {
            let peer = JObject::from(env.new_string(peer)?);
            let path = JObject::from(env.new_string(path)?);
            let stream = env
                .call_static_method(
                    class,
                    jni_str!("openStream"),
                    jni_sig!(
                        "(Landroid/content/Context;Ljava/lang/String;Ljava/lang/String;)Ljava/io/OutputStream;"
                    ),
                    &[JValue::Object(activity), JValue::Object(&peer), JValue::Object(&path)],
                )
                .and_then(jni::JValueOwned::l)?;
            let buffer = env.new_byte_array(CHUNK)?;
            Ok((env.new_global_ref(stream)?, env.new_global_ref(buffer)?))
        })?;
        Ok(Box::new(JavaOutputStream {
            stream,
            buffer,
            scratch: vec![0; CHUNK],
        }))
    }
}

fn java_vm() -> io::Result<JavaVM> {
    JavaVM::singleton().map_err(io::Error::other)
}

fn call_stream(
    stream: &Global<JObject<'static>>,
    name: &'static jni::strings::JNIStr,
) -> io::Result<()> {
    java_vm()?
        .attach_current_thread(|env| -> jni::errors::Result<()> {
            env.call_method(stream.as_obj(), name, jni_sig!("()V"), &[])
                .map(|_| ())
                .inspect_err(|_| clear_pending_android_jni_exception(env))
        })
        .map_err(io::Error::other)
}

struct JavaOutputStream {
    stream: Global<JObject<'static>>,
    buffer: Global<JByteArray<'static>>,
    scratch: Vec<jbyte>,
}

impl Write for JavaOutputStream {
    fn write(&mut self, data: &[u8]) -> io::Result<usize> {
        let count = data.len().min(CHUNK);
        for (slot, &byte) in self.scratch.iter_mut().zip(&data[..count]) {
            *slot = byte as jbyte;
        }
        let (stream, buffer, scratch) = (&self.stream, &self.buffer, &self.scratch);
        java_vm()?
            .attach_current_thread(|env| -> jni::errors::Result<()> {
                buffer.set_region(env, 0, &scratch[..count])?;
                env.call_method(
                    stream.as_obj(),
                    jni_str!("write"),
                    jni_sig!("([BII)V"),
                    &[
                        JValue::Object(buffer.as_obj()),
                        JValue::Int(0),
                        JValue::Int(count as i32),
                    ],
                )
                .map(|_| ())
                .inspect_err(|_| clear_pending_android_jni_exception(env))
            })
            .map_err(io::Error::other)?;
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        call_stream(&self.stream, jni_str!("flush"))
    }
}

impl Drop for JavaOutputStream {
    fn drop(&mut self) {
        let _ = call_stream(&self.stream, jni_str!("close"));
    }
}

struct JavaInputStream {
    stream: Global<JObject<'static>>,
    buffer: Global<JByteArray<'static>>,
    scratch: Vec<jbyte>,
}

impl Read for JavaInputStream {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        let wanted = out.len().min(CHUNK);
        let (stream, buffer, scratch) = (&self.stream, &self.buffer, &mut self.scratch);
        let count = java_vm()?
            .attach_current_thread(|env| -> jni::errors::Result<usize> {
                let read = env
                    .call_method(
                        stream.as_obj(),
                        jni_str!("read"),
                        jni_sig!("([BII)I"),
                        &[
                            JValue::Object(buffer.as_obj()),
                            JValue::Int(0),
                            JValue::Int(wanted as i32),
                        ],
                    )
                    .and_then(jni::JValueOwned::i)
                    .inspect_err(|_| clear_pending_android_jni_exception(env))?;
                let count = usize::try_from(read).unwrap_or(0);
                buffer.get_region(env, 0, &mut scratch[..count])?;
                Ok(count)
            })
            .map_err(io::Error::other)?;
        for (slot, &byte) in out.iter_mut().zip(&self.scratch[..count]) {
            *slot = byte as u8;
        }
        Ok(count)
    }
}

impl Drop for JavaInputStream {
    fn drop(&mut self) {
        let _ = call_stream(&self.stream, jni_str!("close"));
    }
}

/// A message from the paired device.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeWearableService_nativeOnMessage<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    peer: JString<'local>,
    path: JString<'local>,
    data: JByteArray<'local>,
) {
    let decoded = env.with_env(|env| -> jni::errors::Result<_> {
        Ok((
            peer.try_to_string(env)?,
            path.try_to_string(env)?,
            env.convert_byte_array(&data)?,
        ))
    });
    if let Outcome::Ok((peer, path, data)) = decoded.into_outcome() {
        publish_wearable_event(WearableEvent::Message { peer, path, data });
    }
}

/// A stream the paired device opened; Rust reads it until it ends.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "system" fn Java_dev_cranpose_android_CranposeWearableService_nativeOnStream<'local>(
    mut env: EnvUnowned<'local>,
    _class: JClass<'local>,
    peer: JString<'local>,
    path: JString<'local>,
    stream: JObject<'local>,
) {
    let decoded = env.with_env(|env| -> jni::errors::Result<_> {
        let buffer = env.new_byte_array(CHUNK)?;
        Ok((
            peer.try_to_string(env)?,
            path.try_to_string(env)?,
            env.new_global_ref(stream)?,
            env.new_global_ref(buffer)?,
        ))
    });
    if let Outcome::Ok((peer, path, stream, buffer)) = decoded.into_outcome() {
        let reader = JavaInputStream {
            stream,
            buffer,
            scratch: vec![0; CHUNK],
        };
        publish_wearable_event(WearableEvent::Stream(WearableStream::new(
            peer,
            path,
            Box::new(reader),
        )));
    }
}
