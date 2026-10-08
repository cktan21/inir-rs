use crate::events::Event;
use inir_types::desktop::{NiriState, NiriWindow, NiriWorkspace, ServiceStatus};
use niri_ipc::{
    state::{EventStreamState, EventStreamStatePart},
    Action, Reply, Request,
};
use std::{collections::HashMap, path::Path};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::mpsc,
};
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub struct Engine {
    state: EventStreamState,
    focus_serial: u64,
    history: HashMap<u64, u64>,
}

impl Engine {
    pub fn apply(&mut self, mut event: niri_ipc::Event) {
        // A stale or reordered workspace update must not panic the worker.
        match &event {
            niri_ipc::Event::WorkspaceActivated { id, .. }
                if !self.state.workspaces.workspaces.contains_key(id) =>
            {
                return
            }
            niri_ipc::Event::WorkspaceActiveWindowChanged { workspace_id, .. }
                if !self.state.workspaces.workspaces.contains_key(workspace_id) =>
            {
                return
            }
            _ => {}
        }
        match &mut event {
            niri_ipc::Event::WindowLayoutsChanged { changes } => {
                changes.retain(|(id, _)| self.state.windows.windows.contains_key(id))
            }
            niri_ipc::Event::WindowClosed { id }
                if !self.state.windows.windows.contains_key(id) =>
            {
                return
            }
            niri_ipc::Event::CastStopped { stream_id }
                if !self.state.casts.casts.contains_key(stream_id) =>
            {
                return
            }
            niri_ipc::Event::KeyboardLayoutSwitched { .. }
                if self.state.keyboard_layouts.keyboard_layouts.is_none() =>
            {
                return
            }
            _ => {}
        }
        let focused_before = self
            .state
            .windows
            .windows
            .values()
            .find(|w| w.is_focused)
            .map(|w| w.id);
        self.state.apply(event);
        let focused = self
            .state
            .windows
            .windows
            .values()
            .find(|w| w.is_focused)
            .map(|w| w.id);
        if focused != focused_before {
            if let Some(id) = focused {
                self.focus_serial += 1;
                self.history.insert(id, self.focus_serial);
            }
        }
        self.history
            .retain(|id, _| self.state.windows.windows.contains_key(id));
    }

    pub fn snapshot(&self) -> NiriState {
        let mut windows: Vec<_> = self
            .state
            .windows
            .windows
            .values()
            .map(|w| NiriWindow {
                id: w.id.to_string(),
                title: w.title.clone().unwrap_or_default(),
                app_id: w.app_id.clone().unwrap_or_default(),
                workspace_id: w.workspace_id.map(|id| id.to_string()).unwrap_or_default(),
                focused: w.is_focused,
                floating: w.is_floating,
                urgent: w.is_urgent,
                focus_serial: self.history.get(&w.id).copied().unwrap_or_default(),
            })
            .collect();
        windows.sort_by(|a, b| a.id.cmp(&b.id));
        let mut workspaces: Vec<_> = self
            .state
            .workspaces
            .workspaces
            .values()
            .map(|w| NiriWorkspace {
                id: w.id.to_string(),
                index: w.idx,
                name: w.name.clone().unwrap_or_default(),
                output: w.output.clone().unwrap_or_default(),
                active: w.is_active,
                focused: w.is_focused,
                active_window_id: w
                    .active_window_id
                    .map(|id| id.to_string())
                    .unwrap_or_default(),
                urgent: w.is_urgent,
            })
            .collect();
        workspaces.sort_by(|a, b| {
            a.output
                .cmp(&b.output)
                .then(a.index.cmp(&b.index))
                .then(a.id.cmp(&b.id))
        });
        let layouts = self.state.keyboard_layouts.keyboard_layouts.as_ref();
        NiriState {
            status: ServiceStatus {
                ready: true,
                error: String::new(),
            },
            windows,
            workspaces,
            overview_open: self.state.overview.is_open,
            keyboard_layouts: layouts.map(|l| l.names.clone()).unwrap_or_default(),
            keyboard_layout_index: layouts.map(|l| l.current_idx).unwrap_or_default(),
        }
    }
}

pub async fn request(path: &Path, request: Request) -> Result<niri_ipc::Response, String> {
    let stream = UnixStream::connect(path).await.map_err(|e| e.to_string())?;
    let mut stream = BufReader::new(stream);
    stream
        .get_mut()
        .write_all(
            format!(
                "{}\n",
                serde_json::to_string(&request).map_err(|e| e.to_string())?
            )
            .as_bytes(),
        )
        .await
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    stream
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str::<Reply>(&line).map_err(|e| e.to_string())?
}

pub async fn action(value: serde_json::Value) -> Result<(), String> {
    let path = std::env::var_os("NIRI_SOCKET").ok_or("NIRI_SOCKET is unset")?;
    let action: Action = serde_json::from_value(value).map_err(|e| e.to_string())?;
    request(Path::new(&path), Request::Action(action))
        .await
        .map(|_| ())
}

pub async fn stream(
    path: &Path,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> Result<(), String> {
    let stream = UnixStream::connect(path).await.map_err(|e| e.to_string())?;
    let mut stream = BufReader::new(stream);
    stream
        .get_mut()
        .write_all(b"\"EventStream\"\n")
        .await
        .map_err(|e| e.to_string())?;
    let mut line = String::new();
    stream
        .read_line(&mut line)
        .await
        .map_err(|e| e.to_string())?;
    serde_json::from_str::<Reply>(&line).map_err(|e| e.to_string())??;
    let mut engine = Engine::default();
    loop {
        line.clear();
        let count = tokio::select! { biased; _ = stop.cancelled() => return Ok(()), r = stream.read_line(&mut line) => r.map_err(|e| e.to_string())? };
        if count == 0 {
            return Err("Niri event stream disconnected".into());
        }
        // Ignore future events instead of disconnecting from a newer compositor.
        match serde_json::from_str::<niri_ipc::Event>(&line) {
            Ok(event) => {
                engine.apply(event);
                events
                    .send(Event::Niri(engine.snapshot()))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => tracing::debug!(%e, "unrecognized Niri event"),
        }
    }
}
