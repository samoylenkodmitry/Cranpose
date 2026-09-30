use std::{
    cell::RefCell,
    io::{self, BufRead},
    path::PathBuf,
    rc::Rc,
    time::Instant,
};

use anyhow::{Context, Result};
use coroflow::{MutableStateFlow, StateFlow};
use cranpose::{AppLauncher, LaunchedEffectAsync};
use cranpose_coroflow::StateFlowCollect;
use cranpose_live::{LiveView, Registry, Request, Session, composable, live_api};
use cranpose_ui::{Column, ColumnSpec, Modifier, Text, TextStyle};
use notify::{RecursiveMode, Watcher};
use tokio::sync::mpsc;

struct Counter {
    count: MutableStateFlow<i64>,
}

#[live_api]
impl Counter {
    #[live(state)]
    fn count(&self) -> StateFlow<i64> {
        self.count.as_state_flow()
    }

    #[live(action)]
    fn add(&self, amount: i64) {
        self.count.set(self.count.value().saturating_add(amount));
    }

    #[live(action)]
    fn reset(&self) {
        self.count.set(0);
    }
}

#[composable]
fn CounterLabel(count: i64) {
    Text(
        format!("Count: {count}"),
        Modifier::empty(),
        TextStyle::default(),
    );
}

enum Input {
    Json(String),
    FileChanged,
}

fn watch(
    path: &std::path::Path,
    sender: mpsc::Sender<Input>,
) -> Result<notify::RecommendedWatcher> {
    let watched = path.to_owned();
    let mut watcher =
        notify::recommended_watcher(move |event: notify::Result<notify::Event>| match event {
            Ok(event)
                if (event.kind.is_modify() || event.kind.is_create() || event.kind.is_remove())
                    && event.paths.iter().any(|path| path == &watched) =>
            {
                let _ = sender.blocking_send(Input::FileChanged);
            }
            Err(error) => eprintln!("watcher: {error}"),
            _ => {}
        })?;
    watcher.watch(
        path.parent().context("source needs a parent directory")?,
        RecursiveMode::NonRecursive,
    )?;
    Ok(watcher)
}

fn source_request(path: &std::path::Path, session: &Session) -> Result<Request> {
    Ok(Request::Source {
        base_revision: session.revision(),
        source: std::fs::read_to_string(path)?,
        function: "Screen".into(),
    })
}

fn process(input: Input, path: &std::path::Path, session: &Session) -> Result<String> {
    let started = Instant::now();
    let request = match input {
        Input::Json(line) => serde_json::from_str(&line)?,
        Input::FileChanged => source_request(path, session)?,
    };
    let response = session.handle(request)?;
    println!("{}", serde_json::to_string(&response)?);
    Ok(format!(
        "Revision {} · applied in {:.2} ms",
        session.revision(),
        started.elapsed().as_secs_f64() * 1000.0
    ))
}

fn read_stdin(sender: mpsc::Sender<Input>) {
    for line in io::stdin().lock().lines() {
        match line {
            Ok(line) if line.trim().is_empty() => {}
            Ok(line) => {
                if sender.blocking_send(Input::Json(line)).is_err() {
                    break;
                }
            }
            Err(error) => {
                eprintln!("stdin: {error}");
                break;
            }
        }
    }
}

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .map_or_else(
            || PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/screen.rs")),
            PathBuf::from,
        )
        .canonicalize()
        .context("open live source file")?;
    let mut registry = Registry::discover()?;
    registry.bind(
        "counter",
        Rc::new(Counter {
            count: MutableStateFlow::new(0),
        }),
    )?;
    let program =
        cranpose_live::parse_source(&registry, &std::fs::read_to_string(&path)?, "Screen")?;
    let session = Session::new(registry, program)?;
    let (sender, receiver) = mpsc::channel(32);
    let _watcher = watch(&path, sender.clone())?;
    std::thread::spawn(move || read_stdin(sender));
    eprintln!(
        "Watching {}. Send JSON requests on stdin; responses use stdout.",
        path.display()
    );
    let receiver = Rc::new(RefCell::new(Some(receiver)));
    let status =
        MutableStateFlow::new("Ready · edit the source file or send a JSON command".to_owned());
    AppLauncher::new().with_title("Cranpose · live runtime").with_size(700, 420).try_run(move || {
        {
            let session = session.clone();
            let path = path.clone();
            let status = status.clone();
            let receiver = Rc::clone(&receiver);
            LaunchedEffectAsync((), move |_scope| Box::pin(async move {
                let Some(mut receiver) = receiver.borrow_mut().take() else { return; };
                while let Some(input) = receiver.recv().await {
                    match process(input, &path, &session) {
                        Ok(message) => status.set(message),
                        Err(error) => {
                            println!("{}", serde_json::json!({"error": error.to_string(), "revision": session.revision()}));
                            status.set(error.to_string());
                        }
                    }
                }
            }));
        }
        let status_text = status.as_state_flow().collectAsState().get();
        let view = session.clone();
        Column(Modifier::empty().padding(24.0), ColumnSpec::default(), move || {
            Text(status_text.clone(), Modifier::empty(), TextStyle::default());
            LiveView(view.clone());
        });
    })?;
    Ok(())
}
