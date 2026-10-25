use std::sync::Mutex;

use super::*;

/// Records what the platform was asked to show.
#[derive(Default)]
struct Shown(Mutex<Vec<Option<BackgroundWork>>>);

impl BackgroundActivity for Shown {
    fn show(&self, work: Option<&BackgroundWork>) {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(work.cloned());
    }
}

impl Shown {
    fn take(&self) -> Vec<Option<BackgroundWork>> {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn work(title: &str, progress: Option<f32>) -> Option<BackgroundWork> {
    Some(BackgroundWork {
        title: title.to_owned(),
        progress,
    })
}

#[test]
fn the_latest_lease_is_shown_until_the_last_one_ends() {
    let _services = crate::registry::test_service_guard();
    let shown = Arc::new(Shown::default());
    set_platform_background_activity(Arc::<Shown>::clone(&shown));

    let save = acquire_background_work("Saving changes");
    assert!(background_active());
    let download = acquire_background_work("Downloading the models");
    drop(download);
    assert!(background_active());
    drop(save);
    assert!(!background_active());

    assert_eq!(
        shown.take(),
        [
            work("Saving changes", None),
            work("Downloading the models", None),
            work("Saving changes", None),
            None,
        ]
    );
    clear_platform_background_activity();
}

#[test]
fn progress_is_shown_once_per_percent() {
    let _services = crate::registry::test_service_guard();
    let shown = Arc::new(Shown::default());
    set_platform_background_activity(Arc::<Shown>::clone(&shown));

    let download = acquire_background_work("Downloading the models");
    for chunk in 0..=40 {
        download.set_progress(Some(chunk as f32 / 2000.0));
    }
    download.set_title("Checking the models");
    drop(download);

    assert_eq!(
        shown.take(),
        [
            work("Downloading the models", None),
            work("Downloading the models", Some(0.0)),
            work("Downloading the models", Some(0.01)),
            work("Downloading the models", Some(0.02)),
            work("Checking the models", Some(0.02)),
            None,
        ]
    );
    clear_platform_background_activity();
}

#[test]
fn a_platform_installed_during_work_shows_it_at_once() {
    let _services = crate::registry::test_service_guard();
    clear_platform_background_activity();
    let download = acquire_background_work("Downloading the models");
    let shown = Arc::new(Shown::default());
    set_platform_background_activity(Arc::<Shown>::clone(&shown));
    drop(download);

    assert_eq!(shown.take(), [work("Downloading the models", None), None]);
    clear_platform_background_activity();
}
