//! PipeWire's native registry, metadata and SPA Props protocol. No wpctl/pactl.
use crate::events::Event;
use inir_types::desktop::{AudioNode, AudioState, Command};
use pipewire::{
    self as pw,
    spa::{
        self,
        pod::{
            deserialize::PodDeserializer, serialize::PodSerializer, Object, Pod, Property, Value,
            ValueArray,
        },
    },
};
use std::{cell::RefCell, collections::HashMap, io::Cursor, rc::Rc};
use tokio::sync::{mpsc, oneshot, watch};

pub enum Message {
    Stop,
    Command(Command, oneshot::Sender<Result<(), String>>),
}

pub struct Driver {
    pub sender: pw::channel::Sender<Message>,
    thread: Option<std::thread::JoinHandle<()>>,
    forwarder: tokio::task::JoinHandle<()>,
    pub terminated: oneshot::Receiver<()>,
}
impl Driver {
    pub fn start(events: mpsc::Sender<Event>) -> std::io::Result<Self> {
        let (sender, receiver) = pw::channel::channel();
        let (snapshots, mut states) = watch::channel(AudioState::default());
        let forwarder = tokio::spawn(async move {
            while states.changed().await.is_ok() {
                let state = states.borrow_and_update().clone();
                if events.send(Event::Audio(state)).await.is_err() {
                    break;
                }
            }
        });
        let (done, terminated) = oneshot::channel();
        let thread = std::thread::Builder::new()
            .name("inir-pipewire".into())
            .spawn(move || {
                if let Err(error) = run(receiver, snapshots.clone()) {
                    let mut state = AudioState::default();
                    state.status.error = error;
                    snapshots.send_replace(state);
                }
                let _ = done.send(());
            })?;
        Ok(Self {
            sender,
            thread: Some(thread),
            forwarder,
            terminated,
        })
    }
    pub async fn shutdown(&mut self) {
        let _ = self.sender.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = tokio::task::spawn_blocking(move || thread.join()).await;
        }
    }
}
impl Drop for Driver {
    fn drop(&mut self) {
        let _ = self.sender.send(Message::Stop);
        self.forwarder.abort();
    }
}

pub fn apply_props(node: &mut AudioNode, value: Value) {
    let Value::Object(object) = value else { return };
    for property in object.properties {
        match (property.key, property.value) {
            (spa::sys::SPA_PROP_mute, Value::Bool(value)) => node.muted = Some(value),
            (spa::sys::SPA_PROP_channelVolumes, Value::ValueArray(ValueArray::Float(values)))
                if values.iter().all(|v| v.is_finite() && *v >= 0.0) =>
            {
                node.volume = values
                    .iter()
                    .copied()
                    .reduce(f32::max)
                    .map(|v| f64::from(v).cbrt());
                node.channels = values;
            }
            _ => {}
        }
    }
}

fn publish(state: &Rc<RefCell<AudioState>>, events: &watch::Sender<AudioState>) {
    // Coalesce bursts without dropping the final authoritative snapshot or
    // blocking the PipeWire loop on Tokio's bounded event queue.
    events.send_replace(state.borrow().clone());
}

fn run(
    receiver: pw::channel::Receiver<Message>,
    events: watch::Sender<AudioState>,
) -> Result<(), String> {
    pw::init(); // Process-wide init; never call deinit while Qt also uses PipeWire.
    let mainloop = pw::main_loop::MainLoopRc::new(None).map_err(|e| e.to_string())?;
    let context = pw::context::ContextRc::new(&mainloop, None).map_err(|e| e.to_string())?;
    let core = context.connect_rc(None).map_err(|e| e.to_string())?;
    let registry = Rc::new(core.get_registry_rc().map_err(|e| e.to_string())?);
    let state = Rc::new(RefCell::new(AudioState::default()));
    let nodes = Rc::new(RefCell::new(HashMap::<
        u32,
        (pw::node::Node, pw::node::NodeListener),
    >::new()));
    let metadata = Rc::new(RefCell::new(HashMap::<
        u32,
        (pw::metadata::Metadata, pw::metadata::MetadataListener),
    >::new()));

    let _commands = receiver.attach(mainloop.loop_(), {
        let mainloop = mainloop.clone();
        let nodes = nodes.clone();
        let state = state.clone();
        move |message| match message {
            Message::Stop => mainloop.quit(),
            Message::Command(command, reply) => {
                let result = (|| {
                    let (id, property) = match command {
                        Command::AudioMute { node, muted } => (
                            node,
                            Property::new(spa::sys::SPA_PROP_mute, Value::Bool(muted)),
                        ),
                        Command::AudioVolume { node, value } => {
                            if !value.is_finite() || !(0.0..=2.0).contains(&value) {
                                return Err("invalid volume".into());
                            }
                            let snapshot = state.borrow();
                            let record = snapshot
                                .nodes
                                .iter()
                                .find(|n| n.id == node)
                                .ok_or("audio node disappeared")?;
                            if record.channels.is_empty() {
                                return Err("channel volumes unavailable".into());
                            }
                            let old = record
                                .channels
                                .iter()
                                .copied()
                                .reduce(f32::max)
                                .unwrap_or_default();
                            let level = value.powi(3) as f32;
                            let values = record
                                .channels
                                .iter()
                                .map(|v| if old > 0.0 { v / old * level } else { level })
                                .collect();
                            (
                                node,
                                Property::new(
                                    spa::sys::SPA_PROP_channelVolumes,
                                    Value::ValueArray(ValueArray::Float(values)),
                                ),
                            )
                        }
                        _ => return Err("invalid audio command".into()),
                    };
                    let id: u32 = id.parse().map_err(|_| "invalid node id")?;
                    let nodes = nodes.borrow();
                    let (node, _) = nodes.get(&id).ok_or("audio node disappeared")?;
                    let value = Value::Object(Object {
                        type_: spa::utils::SpaTypes::ObjectParamProps.as_raw(),
                        id: spa::param::ParamType::Props.as_raw(),
                        properties: vec![property],
                    });
                    let (buffer, _) = PodSerializer::serialize(Cursor::new(Vec::new()), &value)
                        .map_err(|e| format!("{e:?}"))?;
                    let buffer = buffer.into_inner();
                    let pod = Pod::from_bytes(&buffer).ok_or("invalid SPA pod")?;
                    node.set_param(spa::param::ParamType::Props, 0, pod);
                    // Completion means submitted to PipeWire, not acknowledged;
                    // authoritative state arrives through the Props subscription.
                    Ok(())
                })();
                let _ = reply.send(result);
            }
        }
    });
    let _core = core
        .add_listener_local()
        .done({
            let state = state.clone();
            let events = events.clone();
            move |_, _| {
                state.borrow_mut().status.ready = true;
                publish(&state, &events);
            }
        })
        .error({
            let state = state.clone();
            let events = events.clone();
            let mainloop = mainloop.clone();
            move |id, _, _, error| {
                if id == pw::core::PW_ID_CORE {
                    state.borrow_mut().status.ready = false;
                    state.borrow_mut().status.error = error.into();
                    publish(&state, &events);
                    mainloop.quit();
                }
            }
        })
        .register();
    let _registry = registry
        .add_listener_local()
        .global({
            let registry = registry.clone();
            let state = state.clone();
            let events = events.clone();
            let nodes = nodes.clone();
            let metadata = metadata.clone();
            move |global| {
                let Some(props) = global.props else { return };
                match global.type_ {
                    pw::types::ObjectType::Node
                        if props
                            .get("media.class")
                            .is_some_and(|c| c.contains("Audio")) =>
                    {
                        let Ok(node) = registry.bind::<pw::node::Node, _>(global) else {
                            return;
                        };
                        let id = global.id.to_string();
                        state.borrow_mut().nodes.push(AudioNode {
                            id: id.clone(),
                            serial: props.get("object.serial").unwrap_or_default().into(),
                            name: props.get("node.name").unwrap_or_default().into(),
                            description: props
                                .get("node.description")
                                .or_else(|| props.get("node.nick"))
                                .unwrap_or_default()
                                .into(),
                            media_class: props.get("media.class").unwrap_or_default().into(),
                            ..Default::default()
                        });
                        state.borrow_mut().nodes.sort_by(|a, b| a.id.cmp(&b.id));
                        let listener = node
                            .add_listener_local()
                            .param({
                                let state = state.clone();
                                let events = events.clone();
                                move |_, _, _, _, pod| {
                                    if let Some(pod) = pod {
                                        if let Ok((_, value)) =
                                            PodDeserializer::deserialize_from::<Value>(
                                                pod.as_bytes(),
                                            )
                                        {
                                            if let Some(node) = state
                                                .borrow_mut()
                                                .nodes
                                                .iter_mut()
                                                .find(|n| n.id == id)
                                            {
                                                apply_props(node, value);
                                            }
                                            publish(&state, &events);
                                        }
                                    }
                                }
                            })
                            .register();
                        node.subscribe_params(&[spa::param::ParamType::Props]);
                        nodes.borrow_mut().insert(global.id, (node, listener));
                        publish(&state, &events);
                    }
                    pw::types::ObjectType::Metadata
                        if props.get("metadata.name") == Some("default") =>
                    {
                        let Ok(object) = registry.bind::<pw::metadata::Metadata, _>(global) else {
                            return;
                        };
                        let listener = object
                            .add_listener_local()
                            .property({
                                let state = state.clone();
                                let events = events.clone();
                                move |_, key, _, value| {
                                    let name = value
                                        .and_then(|v| {
                                            serde_json::from_str::<serde_json::Value>(v).ok()
                                        })
                                        .and_then(|v| {
                                            v.get("name")
                                                .and_then(|n| n.as_str())
                                                .map(str::to_owned)
                                        })
                                        .unwrap_or_default();
                                    match key {
                                        Some("default.audio.sink") => {
                                            state.borrow_mut().default_sink = name
                                        }
                                        Some("default.audio.source") => {
                                            state.borrow_mut().default_source = name
                                        }
                                        _ => return 0,
                                    }
                                    publish(&state, &events);
                                    0
                                }
                            })
                            .register();
                        metadata.borrow_mut().insert(global.id, (object, listener));
                    }
                    _ => {}
                }
            }
        })
        .global_remove({
            let state = state.clone();
            let nodes = nodes.clone();
            let metadata = metadata.clone();
            let events = events.clone();
            move |id| {
                nodes.borrow_mut().remove(&id);
                metadata.borrow_mut().remove(&id);
                state.borrow_mut().nodes.retain(|n| n.id != id.to_string());
                publish(&state, &events);
            }
        })
        .register();
    core.sync(0).map_err(|e| e.to_string())?;
    mainloop.run();
    Ok(())
}
