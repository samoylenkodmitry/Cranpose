use std::sync::Mutex;

use super::*;

#[test]
fn apply_observer_ids_do_not_use_process_global_counter() {
    let source = include_str!("../mod.rs");
    assert!(!source.contains(concat!("NEXT_", "OBSERVER_ID")));
    assert!(!source.contains(concat!("Atomic", "Usize")));
}

#[test]
fn test_apply_result_is_success() {
    assert!(SnapshotApplyResult::Success.is_success());
    assert!(!SnapshotApplyResult::Failure.is_success());
}

#[test]
fn test_apply_result_is_failure() {
    assert!(!SnapshotApplyResult::Success.is_failure());
    assert!(SnapshotApplyResult::Failure.is_failure());
}

#[test]
fn test_apply_result_check_success() {
    SnapshotApplyResult::Success.check();
}

#[test]
#[should_panic(expected = "Snapshot apply failed")]
fn test_apply_result_check_failure() {
    SnapshotApplyResult::Failure.check();
}

#[test]
fn snapshot_enter_restores_current_snapshot_after_panic() {
    let _guard = reset_runtime_for_tests();
    let snapshot = take_mutable_snapshot(None, None);

    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        snapshot.enter(|| panic!("snapshot body panic"));
    }));

    assert!(result.is_err());
    assert!(
        current_snapshot().is_none(),
        "snapshot enter must restore the previous current snapshot while unwinding"
    );
}

#[test]
fn test_merge_read_observers_both_none() {
    let result = merge_read_observers(None, None);
    assert!(result.is_none());
}

#[test]
fn test_merge_read_observers_one_some() {
    let observer = Arc::new(|_: &dyn StateObject| {});
    let result = merge_read_observers(Some(observer.clone()), None);
    assert!(result.is_some());

    let result = merge_read_observers(None, Some(observer));
    assert!(result.is_some());
}

#[test]
fn test_merge_write_observers_both_none() {
    let result = merge_write_observers(None, None);
    assert!(result.is_none());
}

#[test]
fn test_merge_write_observers_one_some() {
    let observer = Arc::new(|_: &dyn StateObject| {});
    let result = merge_write_observers(Some(observer.clone()), None);
    assert!(result.is_some());

    let result = merge_write_observers(None, Some(observer));
    assert!(result.is_some());
}

#[test]
fn test_current_snapshot_none_initially() {
    set_current_snapshot(None);
    assert!(current_snapshot().is_none());
}

struct TestStateObject {
    id: usize,
}

impl TestStateObject {
    fn new(id: usize) -> Arc<Self> {
        Arc::new(Self { id })
    }
}

impl StateObject for TestStateObject {
    fn object_id(&self) -> crate::state::ObjectId {
        crate::state::ObjectId(self.id)
    }

    fn first_record(&self) -> Rc<crate::state::StateRecord> {
        unimplemented!("Not needed for observer tests")
    }

    fn try_readable_record(
        &self,
        _snapshot_id: SnapshotId,
        _invalid: &SnapshotIdSet,
    ) -> Option<Rc<crate::state::StateRecord>> {
        None
    }

    fn readable_record(
        &self,
        _snapshot_id: SnapshotId,
        _invalid: &SnapshotIdSet,
    ) -> Rc<crate::state::StateRecord> {
        unimplemented!("Not needed for observer tests")
    }

    fn prepend_state_record(&self, _record: Rc<crate::state::StateRecord>) {
        unimplemented!("Not needed for observer tests")
    }

    fn promote_record(&self, _child_id: SnapshotId) -> Result<(), &'static str> {
        unimplemented!("Not needed for observer tests")
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[test]
fn test_apply_observer_receives_correct_modified_objects() {
    use std::sync::Mutex;

    let received_count = Arc::new(Mutex::new(0));
    let received_snapshot_id = Arc::new(Mutex::new(0));

    let received_count_clone = received_count.clone();
    let received_snapshot_id_clone = received_snapshot_id.clone();

    let _handle = register_apply_observer(Rc::new(move |modified, snapshot_id| {
        *received_snapshot_id_clone.lock().unwrap() = snapshot_id;
        *received_count_clone.lock().unwrap() = modified.len();
    }));

    let obj1: Arc<dyn StateObject> = TestStateObject::new(42);
    let obj2: Arc<dyn StateObject> = TestStateObject::new(99);
    let modified = vec![obj1, obj2];

    notify_apply_observers(&modified, 123);

    assert_eq!(*received_snapshot_id.lock().unwrap(), 123);
    assert_eq!(*received_count.lock().unwrap(), 2);
}

#[test]
fn test_apply_observer_receives_correct_snapshot_id() {
    use std::sync::Mutex;

    let received_id = Arc::new(Mutex::new(0));
    let received_id_clone = received_id.clone();

    let _handle = register_apply_observer(Rc::new(move |_, snapshot_id| {
        *received_id_clone.lock().unwrap() = snapshot_id;
    }));

    notify_apply_observers(&[], 456);

    assert_eq!(*received_id.lock().unwrap(), 456);
}

#[test]
fn test_multiple_apply_observers_all_called() {
    use std::sync::Mutex;

    let call_count1 = Arc::new(Mutex::new(0));
    let call_count2 = Arc::new(Mutex::new(0));
    let call_count3 = Arc::new(Mutex::new(0));

    let call_count1_clone = call_count1.clone();
    let call_count2_clone = call_count2.clone();
    let call_count3_clone = call_count3.clone();

    let _handle1 = register_apply_observer(Rc::new(move |_, _| {
        *call_count1_clone.lock().unwrap() += 1;
    }));

    let _handle2 = register_apply_observer(Rc::new(move |_, _| {
        *call_count2_clone.lock().unwrap() += 1;
    }));

    let _handle3 = register_apply_observer(Rc::new(move |_, _| {
        *call_count3_clone.lock().unwrap() += 1;
    }));

    notify_apply_observers(&[], 1);

    assert_eq!(*call_count1.lock().unwrap(), 1);
    assert_eq!(*call_count2.lock().unwrap(), 1);
    assert_eq!(*call_count3.lock().unwrap(), 1);

    notify_apply_observers(&[], 2);

    assert_eq!(*call_count1.lock().unwrap(), 2);
    assert_eq!(*call_count2.lock().unwrap(), 2);
    assert_eq!(*call_count3.lock().unwrap(), 2);
}

#[test]
fn test_apply_observer_not_called_for_empty_modifications() {
    use std::sync::Mutex;

    let call_count = Arc::new(Mutex::new(0));
    let call_count_clone = call_count.clone();

    let _handle = register_apply_observer(Rc::new(move |modified, _| {
        *call_count_clone.lock().unwrap() += 1;
        assert_eq!(modified.len(), 0);
    }));

    notify_apply_observers(&[], 1);

    assert_eq!(*call_count.lock().unwrap(), 1);
}

fn register_counting_observer(calls: &Arc<Mutex<Vec<i32>>>, tag: i32) -> ObserverHandle {
    let calls = calls.clone();
    register_apply_observer(Rc::new(move |_, _| {
        calls.lock().unwrap().push(tag);
    }))
}

#[test]
fn test_observer_handle_drop_removes_correct_observer() {
    let calls = Arc::new(Mutex::new(Vec::new()));

    let handle1 = register_counting_observer(&calls, 1);
    let handle2 = register_counting_observer(&calls, 2);
    let handle3 = register_counting_observer(&calls, 3);

    notify_apply_observers(&[], 1);
    let result = calls.lock().unwrap().clone();
    assert_eq!(result.len(), 3);
    assert!(result.contains(&1));
    assert!(result.contains(&2));
    assert!(result.contains(&3));
    calls.lock().unwrap().clear();

    drop(handle2);

    notify_apply_observers(&[], 2);
    let result = calls.lock().unwrap().clone();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&1));
    assert!(result.contains(&3));
    assert!(!result.contains(&2));
    calls.lock().unwrap().clear();

    drop(handle1);

    notify_apply_observers(&[], 3);
    let result = calls.lock().unwrap().clone();
    assert_eq!(result.len(), 1);
    assert!(result.contains(&3));
    calls.lock().unwrap().clear();

    drop(handle3);

    notify_apply_observers(&[], 4);
    assert_eq!(calls.lock().unwrap().len(), 0);
}

#[test]
fn test_observer_handle_drop_in_different_orders() {
    {
        let calls = Arc::new(Mutex::new(Vec::new()));

        let h1 = register_counting_observer(&calls, 1);
        let h2 = register_counting_observer(&calls, 2);
        let h3 = register_counting_observer(&calls, 3);

        drop(h3);
        notify_apply_observers(&[], 1);
        let result = calls.lock().unwrap().clone();
        assert!(result.contains(&1) && result.contains(&2) && !result.contains(&3));
        calls.lock().unwrap().clear();

        drop(h2);
        notify_apply_observers(&[], 2);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&1));
        calls.lock().unwrap().clear();

        drop(h1);
        notify_apply_observers(&[], 3);
        assert_eq!(calls.lock().unwrap().len(), 0);
    }

    {
        let calls = Arc::new(Mutex::new(Vec::new()));

        let h1 = register_counting_observer(&calls, 1);
        let h2 = register_counting_observer(&calls, 2);
        let h3 = register_counting_observer(&calls, 3);

        drop(h1);
        notify_apply_observers(&[], 1);
        let result = calls.lock().unwrap().clone();
        assert!(!result.contains(&1) && result.contains(&2) && result.contains(&3));
        calls.lock().unwrap().clear();

        drop(h2);
        notify_apply_observers(&[], 2);
        let result = calls.lock().unwrap().clone();
        assert_eq!(result.len(), 1);
        assert!(result.contains(&3));
        calls.lock().unwrap().clear();

        drop(h3);
        notify_apply_observers(&[], 3);
        assert_eq!(calls.lock().unwrap().len(), 0);
    }
}

#[test]
fn test_remaining_observers_still_work_after_drop() {
    use std::sync::Mutex;

    let calls = Arc::new(Mutex::new(Vec::new()));

    let calls1 = calls.clone();
    let handle1 = register_apply_observer(Rc::new(move |_, snapshot_id| {
        calls1.lock().unwrap().push((1, snapshot_id));
    }));

    let calls2 = calls.clone();
    let handle2 = register_apply_observer(Rc::new(move |_, snapshot_id| {
        calls2.lock().unwrap().push((2, snapshot_id));
    }));

    notify_apply_observers(&[], 100);
    assert_eq!(calls.lock().unwrap().len(), 2);
    calls.lock().unwrap().clear();

    drop(handle1);

    notify_apply_observers(&[], 200);
    assert_eq!(*calls.lock().unwrap(), vec![(2, 200)]);
    calls.lock().unwrap().clear();

    let calls3 = calls.clone();
    let _handle3 = register_apply_observer(Rc::new(move |_, snapshot_id| {
        calls3.lock().unwrap().push((3, snapshot_id));
    }));

    notify_apply_observers(&[], 300);
    let result = calls.lock().unwrap().clone();
    assert_eq!(result.len(), 2);
    assert!(result.contains(&(2, 300)));
    assert!(result.contains(&(3, 300)));

    drop(handle2);
}

#[test]
fn test_observer_ids_are_unique() {
    use std::sync::Mutex;

    let ids = Arc::new(Mutex::new(std::collections::HashSet::new()));

    let mut handles = Vec::new();

    for i in 0..100 {
        let ids_clone = ids.clone();
        let handle = register_apply_observer(Rc::new(move |_, _| {
            ids_clone.lock().unwrap().insert(i);
        }));
        handles.push(handle);
    }

    notify_apply_observers(&[], 1);
    assert_eq!(ids.lock().unwrap().len(), 100);

    for i in (0..100).step_by(2) {
        handles.remove(i / 2);
    }

    ids.lock().unwrap().clear();
    notify_apply_observers(&[], 2);
    assert_eq!(ids.lock().unwrap().len(), 50);
}

#[test]
fn test_state_object_storage_in_modified_set() {
    let state = SnapshotState::new(1, SnapshotIdSet::new(), None, None, false);

    let state_obj = TestStateObject::new(12345) as Arc<dyn StateObject>;

    state.record_write(state_obj.clone(), 1);

    let modified = state.modified.borrow();
    assert_eq!(modified.len(), 1);
    assert!(modified.contains_key(&12345));

    let (stored, writer_id) = modified.get(&12345).unwrap();
    assert_eq!(stored.object_id().as_usize(), 12345);
    assert_eq!(*writer_id, 1);
}

#[test]
fn test_multiple_writes_to_same_state_object() {
    let state = SnapshotState::new(1, SnapshotIdSet::new(), None, None, false);
    let state_obj = TestStateObject::new(99999) as Arc<dyn StateObject>;

    state.record_write(state_obj.clone(), 1);
    assert_eq!(state.modified.borrow().len(), 1);

    state.record_write(state_obj.clone(), 2);
    let modified = state.modified.borrow();
    assert_eq!(modified.len(), 1);
    assert!(modified.contains_key(&99999));
    let (_, writer_id) = modified.get(&99999).unwrap();
    assert_eq!(*writer_id, 2);
}
