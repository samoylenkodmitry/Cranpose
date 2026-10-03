use std::{
    collections::BTreeMap,
    sync::{
        LazyLock, Mutex,
        atomic::{AtomicI64, Ordering},
    },
};

use android_activity::{AndroidApp, AndroidAppWaker};
use cranpose_ui::Rect;
use jni::{
    Env, jni_sig, jni_str,
    objects::{JClass, JString},
    sys::jlong,
};

use crate::{
    native_view::NativeViewHost,
    webview_host::{BrowserView, Events, WebViews},
};

struct Route {
    id: u64,
    events: Events,
    wake: AndroidAppWaker,
}
static ROUTES: LazyLock<Mutex<BTreeMap<i64, Route>>> = LazyLock::new(Mutex::default);
static NEXT_ROUTE: AtomicI64 = AtomicI64::new(1);

#[expect(
    deprecated,
    reason = "jni 0.22.4 generates AtomicBool::fetch_update for its ABI check"
)]
const _: jni::NativeMethod = jni::native_method! {
    java_type = "dev.cranpose.android.CranposeWebViews",
    static extern fn native_event(route: jlong, event: JString),
};

fn native_event<'local>(
    env: &mut Env<'local>,
    _class: JClass<'local>,
    route: jlong,
    event: JString<'local>,
) -> jni::errors::Result<()> {
    let event = event.try_to_string(env)?;
    if let Some(route) = ROUTES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(&route)
    {
        let _ = route.events.send((route.id, event));
        route.wake.wake();
    }
    Ok(())
}

pub(crate) struct AndroidWebView {
    app: AndroidApp,
    route: i64,
}

impl AndroidWebView {
    fn update(&self, url: Option<&str>, bounds: Option<Rect>) -> Result<(), String> {
        crate::android_jni::with_android_activity_env(&self.app, |env, activity| {
            let url = url
                .map(|url| env.new_string(url))
                .transpose()
                .map_err(|error| error.to_string())?;
            let null = jni::objects::JObject::null();
            let text = url
                .as_ref()
                .map_or(jni::objects::JValue::Object(&null), Into::into);
            let rect = bounds.unwrap_or(Rect {
                x: 0.0,
                y: 0.0,
                width: 0.0,
                height: 0.0,
            });
            env.call_method(
                &activity,
                jni_str!("cranposeUpdateWebView"),
                jni_sig!("(JLjava/lang/String;ZFFFF)V"),
                &[
                    self.route.into(),
                    text,
                    bounds.is_some().into(),
                    rect.x.into(),
                    rect.y.into(),
                    rect.width.into(),
                    rect.height.into(),
                ],
            )
            .map_err(|error| {
                crate::android_jni::clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        })
    }
}

impl BrowserView for AndroidWebView {
    fn navigate(&mut self, url: &str) -> Result<(), String> {
        self.update(Some(url), None)
    }
    fn place(&mut self, bounds: Rect) -> Result<(), String> {
        self.update(None, Some(bounds))
    }
}

impl Drop for AndroidWebView {
    fn drop(&mut self) {
        ROUTES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .remove(&self.route);
        let result = crate::android_jni::with_android_activity_env(&self.app, |env, activity| {
            env.call_method(
                &activity,
                jni_str!("cranposeRemoveWebView"),
                jni_sig!("(J)V"),
                &[self.route.into()],
            )
            .map_err(|error| {
                crate::android_jni::clear_pending_android_jni_exception(env);
                error.to_string()
            })?;
            Ok(())
        });
        if let Err(error) = result {
            log::warn!("could not remove Android WebView: {error}");
        }
    }
}

pub(crate) fn sync<R: cranpose_render_common::Renderer>(
    views: &mut WebViews<AndroidWebView>,
    host: &NativeViewHost,
    shell: &mut cranpose_app_shell::AppShell<R>,
    app: &AndroidApp,
) where
    R::Error: std::fmt::Debug,
{
    if host.is_empty() && views.is_empty() {
        return;
    }
    views.sync(host, shell.layout_tree(), |id, url, bounds, events| {
        let route = NEXT_ROUTE.fetch_add(1, Ordering::Relaxed);
        ROUTES
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .insert(
                route,
                Route {
                    id,
                    events: events.clone(),
                    wake: app.create_waker(),
                },
            );
        let view = AndroidWebView {
            app: app.clone(),
            route,
        };
        view.update(Some(url), Some(bounds))?;
        Ok(view)
    });
}
