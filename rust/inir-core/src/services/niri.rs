use crate::events::Event;
use inir_types::desktop::{NiriState, NiriWindow, NiriWorkspace};
use niri_ipc::{
    state::{EventStreamState, EventStreamStatePart},
    Action, Reply, Request,
};
use std::{collections::HashMap, path::Path, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixStream,
    sync::{mpsc, watch},
};
use tokio_util::sync::CancellationToken;

/// Sentinel used by sortWindowsByLayout for windows/outputs without a position,
/// kept identical to the QML implementation so ordering matches exactly.
const NO_POS: i64 = 999_999;

#[derive(Default)]
pub struct Engine {
    state: EventStreamState,
    projected: Arc<NiriState>,
    window_positions: HashMap<u64, usize>,
    workspace_positions: HashMap<u64, usize>,
    focused_id: Option<u64>,
    pub projected_window_updates: u64,
    pub layout_sorts: u64,
    focus_serial: u64,
    history: HashMap<u64, u64>,
    // Output name -> logical (x, y). Outputs are not part of the event stream,
    // so they are queried separately and refreshed when workspaces change.
    outputs: HashMap<String, (i32, i32)>,
    output_data: std::collections::BTreeMap<String, inir_types::desktop::IpcValue>,
    config_generation: u64,
    config_failed: bool,
    windows_initialized: bool,
    workspaces_initialized: bool,
}

impl Engine {
    pub fn set_outputs(&mut self, outputs: HashMap<String, niri_ipc::Output>) {
        let data: std::collections::BTreeMap<_, _> = outputs
            .iter()
            .map(|(name, output)| {
                (
                    name.clone(),
                    serde_json::to_value(output)
                        .expect("serializable Niri output")
                        .into(),
                )
            })
            .collect();
        let positions = outputs
            .into_iter()
            .filter_map(|(name, output)| output.logical.map(|l| (name, (l.x, l.y))))
            .collect();
        let moved = self.outputs != positions;
        self.outputs = positions;
        if self.output_data != data {
            self.output_data = data;
            Arc::make_mut(&mut self.projected).outputs = Arc::new(self.output_data.clone());
        }
        if moved {
            self.sort_windows();
        }
    }

    /// Layout-aware sort key, ported from NiriService.sortWindowsByLayout:
    /// output X, output Y, workspace index, column, row, then window id.
    fn layout_key(&self, window: &niri_ipc::Window) -> (i64, i64, i64, i64, i64, u64) {
        let workspace = window
            .workspace_id
            .and_then(|id| self.state.workspaces.workspaces.get(&id));
        let (ox, oy, ws_idx) = match workspace {
            Some(ws) => {
                let (ox, oy) = ws
                    .output
                    .as_ref()
                    .and_then(|name| self.outputs.get(name))
                    .map(|&(x, y)| (x as i64, y as i64))
                    .unwrap_or((NO_POS, NO_POS));
                (ox, oy, ws.idx as i64)
            }
            None => return (NO_POS, NO_POS, NO_POS, NO_POS, NO_POS, window.id),
        };
        let (col, row) = window
            .layout
            .pos_in_scrolling_layout
            .map(|(c, r)| (c as i64, r as i64))
            .unwrap_or((NO_POS, NO_POS));
        (ox, oy, ws_idx, col, row, window.id)
    }
}

impl Engine {
    pub fn apply(&mut self, mut event: niri_ipc::Event) {
        // A stale or reordered workspace update must not panic the worker.
        match &event {
            niri_ipc::Event::WindowsChanged { windows } => {
                self.windows_initialized = true;
                if self.history.is_empty() {
                    // Recover MRU on reconnect from the compositor's monotonic
                    // timestamps, then keep tracking focus events locally.
                    let mut history: Vec<_> = windows
                        .iter()
                        .filter_map(|w| w.focus_timestamp.map(|t| ((t.secs, t.nanos, w.id), w.id)))
                        .collect();
                    history.sort_by_key(|(timestamp, _)| *timestamp);
                    for (_, id) in history {
                        self.focus_serial += 1;
                        self.history.insert(id, self.focus_serial);
                    }
                }
            }
            niri_ipc::Event::WorkspacesChanged { .. } => self.workspaces_initialized = true,
            niri_ipc::Event::ConfigLoaded { failed } => {
                self.config_generation += 1;
                self.config_failed = *failed;
            }
            niri_ipc::Event::WorkspaceUrgencyChanged { id, .. }
                if !self.state.workspaces.workspaces.contains_key(id) =>
            {
                return
            }
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
        use niri_ipc::Event as N;
        let before_focus = self.focused_id;
        let all_windows = matches!(&event, N::WindowsChanged { .. });
        let all_workspaces = matches!(&event, N::WorkspacesChanged { .. });
        let mut window_ids = match &event {
            N::WindowsChanged { windows } => windows.iter().map(|w| w.id).collect(),
            N::WindowOpenedOrChanged { window } => vec![window.id],
            N::WindowClosed { id }
            | N::WindowUrgencyChanged { id, .. }
            | N::WindowFocusTimestampChanged { id, .. } => vec![*id],
            N::WindowLayoutsChanged { changes } => changes.iter().map(|(id, _)| *id).collect(),
            N::WindowFocusChanged { id } => id.iter().copied().collect(),
            _ => Vec::new(),
        };
        let workspace_ids: Vec<_> = match &event {
            N::WorkspaceUrgencyChanged { id, .. } => vec![*id],
            N::WorkspaceActiveWindowChanged { workspace_id, .. } => vec![*workspace_id],
            N::WorkspaceActivated { id, focused } => {
                let output = self
                    .state
                    .workspaces
                    .workspaces
                    .get(id)
                    .and_then(|w| w.output.as_ref());
                self.state
                    .workspaces
                    .workspaces
                    .values()
                    .filter(|w| w.output.as_ref() == output || (*focused && w.is_focused))
                    .map(|w| w.id)
                    .collect()
            }
            N::WorkspacesChanged { workspaces } => workspaces.iter().map(|w| w.id).collect(),
            _ => Vec::new(),
        };
        if matches!(&event, N::WindowFocusChanged { .. })
            || matches!(&event, N::WindowOpenedOrChanged { window } if window.is_focused)
        {
            window_ids.extend(before_focus);
        }
        let before_keys: HashMap<_, _> = window_ids
            .iter()
            .map(|id| {
                (
                    *id,
                    self.state
                        .windows
                        .windows
                        .get(id)
                        .map(|w| self.layout_key(w)),
                )
            })
            .collect();
        let keyboard_changed = matches!(
            &event,
            N::KeyboardLayoutsChanged { .. } | N::KeyboardLayoutSwitched { .. }
        );
        let overview_changed = matches!(&event, N::OverviewOpenedOrClosed { .. });
        let config_changed = matches!(&event, N::ConfigLoaded { .. });
        let closing = matches!(&event, N::WindowClosed { .. });
        // Focus paths touch only the old and new window, rather than scanning
        // every window or reconstructing its strings on a title event.
        match event {
            N::WindowFocusChanged { id } => {
                if let Some(old) = before_focus {
                    if let Some(window) = self.state.windows.windows.get_mut(&old) {
                        window.is_focused = false;
                    }
                }
                self.focused_id = id.filter(|id| self.state.windows.windows.contains_key(id));
                if let Some(id) = self.focused_id {
                    self.state.windows.windows.get_mut(&id).unwrap().is_focused = true;
                }
            }
            N::WindowOpenedOrChanged { window } => {
                let id = window.id;
                if window.is_focused {
                    if let Some(old) = before_focus.filter(|old| *old != id) {
                        if let Some(w) = self.state.windows.windows.get_mut(&old) {
                            w.is_focused = false;
                        }
                    }
                    self.focused_id = Some(id);
                } else if before_focus == Some(id) {
                    self.focused_id = None;
                }
                self.state.windows.windows.insert(id, window);
            }
            event => {
                self.state.apply(event);
                if all_windows {
                    self.focused_id = self
                        .state
                        .windows
                        .windows
                        .values()
                        .find(|w| w.is_focused)
                        .map(|w| w.id);
                } else if closing
                    && before_focus.is_some_and(|id| !self.state.windows.windows.contains_key(&id))
                {
                    self.focused_id = None;
                }
            }
        }
        let focus_changed = self.focused_id != before_focus;
        if focus_changed {
            if let Some(id) = self.focused_id {
                self.focus_serial += 1;
                self.history.insert(id, self.focus_serial);
            }
            window_ids.extend(before_focus);
            window_ids.extend(self.focused_id);
        }
        if all_windows || closing {
            self.history
                .retain(|id, _| self.state.windows.windows.contains_key(id));
        }
        window_ids.sort_unstable();
        window_ids.dedup();
        let order_changed = all_windows
            || all_workspaces
            || window_ids.iter().any(|id| {
                before_keys.get(id).copied().flatten()
                    != self
                        .state
                        .windows
                        .windows
                        .get(id)
                        .map(|w| self.layout_key(w))
            });
        self.project_windows(&window_ids, all_windows);
        self.project_workspaces(&workspace_ids, all_workspaces);
        if order_changed {
            self.sort_windows();
        }
        if focus_changed || all_windows || closing {
            let mut history: Vec<_> = self.history.iter().collect();
            history.sort_by_key(|(_, serial)| std::cmp::Reverse(**serial));
            let ids: Vec<_> = history.into_iter().map(|(id, _)| *id).collect();
            if self.projected.mru_window_ids.as_ref() != &ids {
                Arc::make_mut(&mut self.projected).mru_window_ids = Arc::new(ids);
            }
        }
        if keyboard_changed {
            if let Some(layouts) = &self.state.keyboard_layouts.keyboard_layouts {
                let state = Arc::make_mut(&mut self.projected);
                if state.keyboard_layouts.as_ref() != &layouts.names {
                    state.keyboard_layouts = Arc::new(layouts.names.clone());
                }
                state.keyboard_layout_index = layouts.current_idx;
            }
        }
        if overview_changed {
            Arc::make_mut(&mut self.projected).overview_open = self.state.overview.is_open;
        }
        if config_changed {
            let state = Arc::make_mut(&mut self.projected);
            state.config_generation = self.config_generation;
            state.config_failed = self.config_failed;
        }
        let ready = self.windows_initialized && self.workspaces_initialized;
        if self.projected.status.ready != ready {
            Arc::make_mut(&mut self.projected).status.ready = ready;
        }
    }

    fn window_record(&self, w: &niri_ipc::Window) -> NiriWindow {
        let (col, row) = w
            .layout
            .pos_in_scrolling_layout
            .map(|(c, r)| (c as i32, r as i32))
            .unwrap_or((-1, -1));
        let (tile_pos_x, tile_pos_y, tile_pos_valid) = w
            .layout
            .tile_pos_in_workspace_view
            .map(|(x, y)| (x, y, true))
            .unwrap_or((0.0, 0.0, false));
        NiriWindow {
            id: w.id.to_string(),
            title: w.title.clone().unwrap_or_default(),
            app_id: w.app_id.clone().unwrap_or_default(),
            pid: w.pid.unwrap_or(-1),
            title_present: w.title.is_some(),
            app_id_present: w.app_id.is_some(),
            workspace_id: w.workspace_id.map(|id| id.to_string()).unwrap_or_default(),
            focused: w.is_focused,
            floating: w.is_floating,
            urgent: w.is_urgent,
            focus_serial: self.history.get(&w.id).copied().unwrap_or_default(),
            column: col,
            row,
            tile_width: w.layout.tile_size.0,
            tile_height: w.layout.tile_size.1,
            window_width: w.layout.window_size.0,
            window_height: w.layout.window_size.1,
            tile_pos_x,
            tile_pos_y,
            tile_pos_valid,
            window_offset_x: w.layout.window_offset_in_tile.0,
            window_offset_y: w.layout.window_offset_in_tile.1,
            focus_timestamp: w.focus_timestamp.map(|t| (t.secs, t.nanos)),
        }
    }
    fn workspace_record(&self, w: &niri_ipc::Workspace) -> NiriWorkspace {
        NiriWorkspace {
            id: w.id.to_string(),
            index: w.idx,
            name: w.name.clone().unwrap_or_default(),
            name_present: w.name.is_some(),
            output: w.output.clone().unwrap_or_default(),
            active: w.is_active,
            focused: w.is_focused,
            active_window_id: w
                .active_window_id
                .map(|id| id.to_string())
                .unwrap_or_default(),
            urgent: w.is_urgent,
        }
    }
    fn project_windows(&mut self, ids: &[u64], replace: bool) {
        let mut updates = Vec::new();
        for id in ids {
            if let Some(window) = self.state.windows.windows.get(id) {
                let record = self.window_record(window);
                self.projected_window_updates += 1;
                let unchanged = self
                    .window_positions
                    .get(id)
                    .is_some_and(|index| self.projected.windows[*index].as_ref() == &record);
                if !unchanged {
                    updates.push((*id, Arc::new(record)));
                }
            }
        }
        let removed: Vec<_> = self
            .window_positions
            .keys()
            .copied()
            .filter(|id| {
                (replace || ids.contains(id)) && !self.state.windows.windows.contains_key(id)
            })
            .collect();
        if updates.is_empty() && removed.is_empty() {
            return;
        }
        let rows = Arc::make_mut(&mut Arc::make_mut(&mut self.projected).windows);
        if !removed.is_empty() {
            rows.retain(|row| !removed.iter().any(|id| row.id == id.to_string()));
        }
        if !removed.is_empty() {
            self.window_positions = rows
                .iter()
                .enumerate()
                .map(|(i, w)| (w.id.parse().unwrap(), i))
                .collect();
        }
        for (id, record) in updates {
            if let Some(index) = self.window_positions.get(&id) {
                rows[*index] = record;
            } else {
                self.window_positions.insert(id, rows.len());
                rows.push(record);
            }
        }
    }
    fn project_workspaces(&mut self, ids: &[u64], replace: bool) {
        let mut updates = Vec::new();
        for id in ids {
            if let Some(workspace) = self.state.workspaces.workspaces.get(id) {
                let record = self.workspace_record(workspace);
                let unchanged = self
                    .workspace_positions
                    .get(id)
                    .is_some_and(|index| self.projected.workspaces[*index].as_ref() == &record);
                if !unchanged {
                    updates.push((*id, Arc::new(record)));
                }
            }
        }
        let removed: Vec<_> = self
            .workspace_positions
            .keys()
            .copied()
            .filter(|id| {
                (replace || ids.contains(id)) && !self.state.workspaces.workspaces.contains_key(id)
            })
            .collect();
        if updates.is_empty() && removed.is_empty() {
            return;
        }
        let rows = Arc::make_mut(&mut Arc::make_mut(&mut self.projected).workspaces);
        if !removed.is_empty() {
            rows.retain(|row| !removed.iter().any(|id| row.id == id.to_string()));
        }
        self.workspace_positions = rows
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id.parse().unwrap(), i))
            .collect();
        for (id, record) in updates {
            if let Some(index) = self.workspace_positions.get(&id) {
                rows[*index] = record;
            } else {
                self.workspace_positions.insert(id, rows.len());
                rows.push(record);
            }
        }
        if replace {
            rows.sort_by_key(|w| (w.index, w.id.parse::<u64>().unwrap()));
        }
        self.workspace_positions = rows
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id.parse().unwrap(), i))
            .collect();
    }
    fn sort_windows(&mut self) {
        self.layout_sorts += 1;
        let mut order: Vec<_> = self
            .state
            .windows
            .windows
            .values()
            .map(|w| (self.layout_key(w), w.id))
            .collect();
        order.sort_unstable();
        if self
            .projected
            .windows
            .iter()
            .map(|w| w.id.parse::<u64>().unwrap())
            .eq(order.iter().map(|(_, id)| *id))
        {
            return;
        }
        let positions = &self.window_positions;
        let rows = Arc::make_mut(&mut Arc::make_mut(&mut self.projected).windows);
        let next = order
            .into_iter()
            .map(|(_, id)| rows[positions[&id]].clone())
            .collect();
        *rows = next;
        self.window_positions = rows
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id.parse().unwrap(), i))
            .collect();
    }
    /// Cheap persistent state handle; unchanged collections and rows share storage.
    pub fn frame(&self) -> Arc<NiriState> {
        self.projected.clone()
    }
    /// Explicit diagnostic/test view, not the publication path.
    pub fn snapshot(&self) -> NiriState {
        (*self.projected).clone()
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

/// FIFO owner of the action connection. Taking the stream out before awaiting
/// also discards it on timeout/cancellation, so a late reply cannot satisfy the
/// next command. Never replay an action after an ambiguous transport failure.
#[derive(Default)]
pub struct ActionClient {
    stream: Option<BufReader<UnixStream>>,
}

impl ActionClient {
    pub async fn action(&mut self, path: &Path, value: serde_json::Value) -> Result<(), String> {
        let action: Action = serde_json::from_value(value).map_err(|e| e.to_string())?;
        let mut stream = match self.stream.take() {
            Some(stream) => stream,
            None => BufReader::new(UnixStream::connect(path).await.map_err(|e| e.to_string())?),
        };
        let message = format!(
            "{}\n",
            serde_json::to_string(&Request::Action(action)).unwrap()
        );
        stream
            .get_mut()
            .write_all(message.as_bytes())
            .await
            .map_err(|e| e.to_string())?;
        let mut line = String::new();
        if stream
            .read_line(&mut line)
            .await
            .map_err(|e| e.to_string())?
            == 0
        {
            return Err("Niri action socket disconnected".into());
        }
        let reply = serde_json::from_str::<Reply>(&line).map_err(|e| e.to_string())?;
        self.stream = Some(stream);
        reply.map(|_| ())
    }
}

pub async fn stream(
    path: &Path,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
) -> Result<(), String> {
    let (_refresh, receiver) = watch::channel(0);
    stream_with_refresh(path, events, stop, receiver).await
}

pub async fn stream_with_refresh(
    path: &Path,
    events: &mpsc::Sender<Event>,
    stop: &CancellationToken,
    mut refresh: watch::Receiver<u64>,
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
    refresh_outputs(&mut engine, path).await;
    let mut lines = stream.lines();
    loop {
        let line = tokio::select! {
            biased;
            _ = stop.cancelled() => return Ok(()),
            Ok(()) = refresh.changed() => {
                refresh_outputs(&mut engine, path).await;
                events.send(Event::NiriFrame(engine.frame())).await.map_err(|e| e.to_string())?;
                continue;
            }
            r = lines.next_line() => r.map_err(|e| e.to_string())?
        };
        let line = line.ok_or("Niri event stream disconnected")?;
        // Ignore future events instead of disconnecting from a newer compositor.
        match serde_json::from_str::<niri_ipc::Event>(&line) {
            Ok(event) => {
                // Output geometry is not part of the event stream; refresh it when
                // workspaces change, which also covers output hotplug/reposition.
                if matches!(
                    event,
                    niri_ipc::Event::WorkspacesChanged { .. }
                        | niri_ipc::Event::ConfigLoaded { failed: false }
                ) {
                    refresh_outputs(&mut engine, path).await;
                }
                engine.apply(event);
                events
                    .send(Event::NiriFrame(engine.frame()))
                    .await
                    .map_err(|e| e.to_string())?;
            }
            Err(e) => tracing::debug!(%e, "unrecognized Niri event"),
        }
    }
}

/// Output geometry is auxiliary: if the query fails, keep the last good geometry
/// instead of tearing down the event stream and publishing an error state.
async fn refresh_outputs(engine: &mut Engine, path: &Path) {
    match fetch_outputs(path).await {
        Ok(outputs) => engine.set_outputs(outputs),
        Err(error) => tracing::warn!(%error, "Niri outputs refresh failed; keeping last geometry"),
    }
}

async fn fetch_outputs(path: &Path) -> Result<HashMap<String, niri_ipc::Output>, String> {
    match tokio::time::timeout(
        std::time::Duration::from_secs(3),
        request(path, Request::Outputs),
    )
    .await
    {
        Ok(Ok(niri_ipc::Response::Outputs(outputs))) => Ok(outputs),
        Ok(Err(error)) => Err(error),
        Ok(_) => Err("Unexpected Niri outputs reply".into()),
        Err(_) => Err("Niri outputs request timed out".into()),
    }
}
