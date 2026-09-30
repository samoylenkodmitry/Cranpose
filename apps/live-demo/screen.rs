fn Screen() {
    Column(|| {
        Text("Edit apps/live-demo/screen.rs and save".into());
        key("count", || CounterLabel(counter.count().collectAsState().get()));
        key("increment", || Button("Add one".into(), || counter.add(1)));
        key("reset", || Button("Reset".into(), || counter.reset()));
    });
}
