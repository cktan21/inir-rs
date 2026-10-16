use inir_utils::{clipboard, Context, OPERATIONS, PROTOCOL_VERSION};
use serde_json::{json, Value};
use std::{
    io::{Read, Write},
    process::Stdio,
};
use tokio_util::sync::CancellationToken;

fn input() -> Result<Vec<u8>, String> {
    let mut bytes = Vec::new();
    std::io::stdin()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}
fn print_json(value: &Value) -> Result<(), String> {
    let mut output = std::io::stdout().lock();
    serde_json::to_writer(&mut output, value).map_err(|e| e.to_string())?;
    output.write_all(b"\n").map_err(|e| e.to_string())
}
async fn run() -> Result<i32, String> {
    let args: Vec<_> = std::env::args().skip(1).collect();
    let command: Vec<_> = args.iter().map(String::as_str).collect();
    match command.as_slice() {
        ["--version"] => {
            println!(
                "inir-helper {} (protocol {PROTOCOL_VERSION})",
                env!("CARGO_PKG_VERSION")
            );
            return Ok(0);
        }
        ["--protocol-version"] => {
            println!("{PROTOCOL_VERSION}");
            return Ok(0);
        }
        [] | ["--help"] | ["help"] => {
            println!("Usage: inir-helper job OPERATION < args.json\n       inir-helper clipboard filter|store\n       inir-helper devices snapshot [SYSFS_USB_ROOT]\n       inir-helper system sensors [SYSFS_ROOT]\n       inir-helper lyrics fetch TITLE ARTIST [ALBUM DURATION REQUEST_ID]\n\nOperations: {}", OPERATIONS.join(", "));
            return Ok(0);
        }
        ["clipboard", mode @ ("filter" | "store")] => {
            let Some(bytes) = clipboard::filter(&input()?) else {
                return Ok(0);
            };
            if *mode == "filter" {
                std::io::stdout()
                    .write_all(&bytes)
                    .map_err(|e| e.to_string())?;
                return Ok(0);
            }
            let mut child = tokio::process::Command::new("cliphist")
                .arg("store")
                .stdin(Stdio::piped())
                .kill_on_drop(true)
                .spawn()
                .map_err(|e| e.to_string())?;
            use tokio::io::AsyncWriteExt;
            child
                .stdin
                .take()
                .ok_or("Missing cliphist input")?
                .write_all(&bytes)
                .await
                .map_err(|e| e.to_string())?;
            return Ok(child
                .wait()
                .await
                .map_err(|e| e.to_string())?
                .code()
                .unwrap_or(1));
        }
        ["devices", "watch"] => {
            inir_utils::devices::watch(&CancellationToken::new(), |kind| {
                let _ = print_json(&json!({"kind": kind}));
            })
            .await?;
            return Ok(0);
        }
        ["dev", "reach", root] => {
            print_json(&inir_utils::dev_reload::reach_json(std::path::Path::new(
                root,
            )))?;
            return Ok(0);
        }
        ["dev", "watch", root, pid] => {
            let pid = pid
                .parse::<u32>()
                .ok()
                .filter(|pid| *pid > 0 && *pid <= i32::MAX as u32)
                .ok_or("Invalid shell process ID")?;
            inir_utils::dev_reload::watch(root.into(), pid, &CancellationToken::new()).await?;
            return Ok(0);
        }
        ["input", "locks", "--once"] => {
            let state = inir_utils::input_locks::probe();
            if !state.status.ready {
                return Err(state.status.error);
            }
            print_json(
                &json!({"type":"state", "caps":state.caps, "num":state.num, "devices":state.devices}),
            )?;
            return Ok(0);
        }
        ["input", "locks"] => {
            let stop = CancellationToken::new();
            let (sender, mut receiver) = tokio::sync::mpsc::channel(16);
            let worker = inir_utils::input_locks::stream(&sender, &stop);
            tokio::pin!(worker);
            loop {
                tokio::select! {
                    result = &mut worker => { result?; return Ok(0); },
                    Some(state) = receiver.recv() => {
                        if !state.status.ready { return Err(state.status.error); }
                        print_json(&json!({"type":"state", "caps":state.caps, "num":state.num, "devices":state.devices}))?;
                    }
                }
            }
        }
        _ => {}
    }
    let context = Context::default();
    let stop = CancellationToken::new();
    match command.as_slice() {
        ["job", operation] => {
            let args = serde_json::from_slice(&input()?)
                .map_err(|_| "Expected one JSON object on stdin")?;
            let result = context.execute(operation, args, &stop).await;
            // Jobs always return an envelope. Direct subcommands retain legacy stdout.
            let envelope = match result {
                Ok(value) => json!({"success":true,"result":value,"error":""}),
                Err(error) => json!({"success":false,"result":null,"error":error}),
            };
            print_json(&envelope)?;
        }
        ["devices", "snapshot", rest @ ..] if rest.len() <= 1 => print_json(
            &context
                .execute(
                    "devices.snapshot",
                    json!({"root":rest.first().copied().unwrap_or("/sys/bus/usb/devices")}),
                    &stop,
                )
                .await?,
        )?,
        ["system", "sensors", rest @ ..] if rest.len() <= 1 => {
            let result = context
                .execute(
                    "system.sensors",
                    json!({"root":rest.first().copied().unwrap_or("/sys")}),
                    &stop,
                )
                .await?;
            for (key, value) in result.as_object().unwrap() {
                println!("{key}:{}", value.as_str().unwrap());
            }
        }
        ["lyrics", "fetch", rest @ ..] if (2..=5).contains(&rest.len()) => {
            let args = json!({"title":rest[0],"artist":rest[1],"album":rest.get(2).copied().unwrap_or(""),"duration":rest.get(3).and_then(|v|v.parse::<f64>().ok()),"requestId":rest.get(4).copied().unwrap_or("")});
            print_json(&context.execute("lyrics.fetch", args, &stop).await?)?;
        }
        _ => return Err("Unknown command; run inir-helper --help".into()),
    }
    Ok(0)
}

fn main() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("utility runtime");
    let result = runtime.block_on(async {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .map_err(|error| error.to_string())?;
        inir_utils::attach_to_parent()?;
        tokio::select! {
            result = run() => result,
            _ = terminate.recv() => Ok(143),
            _ = tokio::signal::ctrl_c() => Ok(130),
        }
    });
    let code = match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("inir-helper: {error}");
            1
        }
    };
    // This is a command, not a service: never wait for unrelated blocking tasks.
    runtime.shutdown_background();
    std::process::exit(code);
}
