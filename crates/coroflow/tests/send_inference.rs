use std::{rc::Rc, sync::Arc, time::Duration};

use coroflow::{
    BoxFlow, CoroutineScope, Dispatchers, Flow, FlowExt, MainScope, MutableStateFlow,
    SharingStarted, TestScheduler, flow, flow_of,
};

fn assert_send<T: Send>(_: &T) {}

fn assert_send_flow<F>(flow: &F)
where
    F: Flow + Send,
    F::Run: Send,
{
    assert_send(flow);
    assert_send(&flow.open());
}

trait Repository: Send + Sync {
    fn observe(&self) -> BoxFlow<Vec<u32>>;
}

struct InMemory {
    rows: MutableStateFlow<Vec<u32>>,
}

impl Repository for InMemory {
    fn observe(&self) -> BoxFlow<Vec<u32>> {
        self.rows.as_state_flow().boxed()
    }
}

#[test]
fn a_chain_over_plain_data_is_send_without_any_annotation() {
    let query = MutableStateFlow::new(String::new());
    let rows = MutableStateFlow::new(vec![1_u32, 2, 3]);
    let chain = query
        .as_state_flow()
        .debounce(Duration::from_millis(300))
        .distinct_until_changed()
        .flat_map_latest(move |needle: String| {
            let length = needle.len() as u32;
            rows.as_state_flow().map(move |all| {
                all.into_iter()
                    .filter(|row| *row > length)
                    .collect::<Vec<_>>()
            })
        })
        .combine(flow_of(vec![true]), |rows, online| (rows.clone(), *online))
        .flow_on(Dispatchers::default_pool());
    assert_send_flow(&chain);
    assert_send_flow(&chain.boxed());
}

#[test]
fn a_repository_trait_returns_boxed_flows_that_background_scopes_can_share() {
    let scheduler = TestScheduler::new();
    let repository: Rc<dyn Repository> = Rc::new(InMemory {
        rows: MutableStateFlow::new(vec![4, 5]),
    });
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let shared =
        repository
            .observe()
            .map(|rows| rows.len())
            .state_in(&scope, SharingStarted::Eagerly, 0);
    scheduler.run_current();
    assert_eq!(shared.value(), 2);
}

#[test]
fn a_flow_holding_rc_is_accepted_by_the_main_scope_only() {
    let scheduler = TestScheduler::new();
    let scope = MainScope::new(scheduler.main_dispatcher());
    let local = Rc::new(7);
    let local_flow = flow(move |emitter| {
        let local = Rc::clone(&local);
        async move {
            emitter.emit(*local).await;
        }
    });
    let state = local_flow.state_in(&scope, SharingStarted::Eagerly, 0);
    scheduler.run_current();
    assert_eq!(state.value(), 7);
}

struct Store;

impl Store {
    async fn load(&self, id: u32) -> u32 {
        id * 2
    }
}

#[test]
fn suspending_lambdas_capture_state_without_clones_and_stay_send() {
    let scheduler = TestScheduler::new();
    let store = Arc::new(Store);
    let chain = flow_of(vec![1_u32, 2, 3])
        .map_async(async move |id| store.load(id).await)
        .filter_async(async |value| value > 2)
        .map_latest(async |value| value + 1);
    assert_send_flow(&chain);
    let scope = CoroutineScope::new(scheduler.dispatcher());
    let loaded = chain.state_in(&scope, SharingStarted::Eagerly, 0);
    scheduler.run_current();
    assert_eq!(loaded.value(), 7);
    let main = MainScope::new(scheduler.main_dispatcher());
    let local = Rc::new(10_u32);
    let on_main = flow_of(vec![1_u32]).map_async(async move |value| value + *local);
    let state = on_main.state_in(&main, SharingStarted::Eagerly, 0);
    scheduler.run_current();
    assert_eq!(state.value(), 11);
}
