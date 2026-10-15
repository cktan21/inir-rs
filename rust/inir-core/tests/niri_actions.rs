use inir_core::services::niri::ActionClient;
use inir_types::desktop::IpcValue;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::UnixListener,
};

// The Qt bridge carries every QML number as IpcValue::Number(f64), including
// window ids, workspace references and integer action parameters.
fn qml_action(value: Value) -> Value {
    serde_json::to_value(IpcValue::from(value)).unwrap()
}

#[tokio::test]
async fn niri_actions_accept_qml_numbers_on_the_wire() {
    let actions = vec![
        json!({"FocusWindow": {"id": 4}}),
        json!({"CloseWindow": {"id": 5}}),
        json!({"FocusWorkspace": {"reference": {"Id": 7}}}),
        json!({"FocusWorkspace": {"reference": {"Index": 2}}}),
        json!({"MoveWindowToWorkspace": {
            "window_id": 4, "reference": {"Id": 7}, "focus": false
        }}),
        json!({"MoveWindowToWorkspace": {
            "window_id": 4, "reference": {"Index": 2}, "focus": true
        }}),
        json!({"SetColumnWidth": {"change": {"AdjustFixed": -12}}}),
        json!({"SetColumnWidth": {"change": {"SetProportion": 37.5}}}),
        json!({"MoveFloatingWindow": {
            "id": 4, "x": {"SetFixed": -12.5}, "y": {"SetFixed": 20.25}
        }}),
        json!({"FocusWorkspace": {"reference": {"Name": "2.0"}}}),
        json!({"CloseWindow": {"id": null}}),
    ];
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("niri.sock");
    let listener = UnixListener::bind(&path).unwrap();
    let expected = actions.clone();
    let server = tokio::spawn(async move {
        let (stream, _) = listener.accept().await.unwrap();
        let mut stream = BufReader::new(stream);
        for action in expected {
            let mut line = String::new();
            assert!(stream.read_line(&mut line).await.unwrap() > 0);
            let request: niri_ipc::Request = serde_json::from_str(&line).unwrap();
            assert_eq!(
                serde_json::to_value(request).unwrap(),
                json!({"Action": action})
            );
            stream
                .get_mut()
                .write_all(b"{\"Ok\":\"Handled\"}\n")
                .await
                .unwrap();
        }
    });

    tokio::time::timeout(Duration::from_secs(3), async {
        let mut client = ActionClient::default();
        for action in actions {
            client.action(&path, qml_action(action)).await.unwrap();
        }
        server.await.unwrap();
    })
    .await
    .unwrap();
}

#[tokio::test]
async fn niri_actions_reject_invalid_integer_parameters_before_connecting() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("missing.sock");
    let mut client = ActionClient::default();
    for action in [
        json!({"FocusWindow": {"id": 4.5}}),
        json!({"FocusWindow": {"id": -1.0}}),
        json!({"FocusWindow": {"id": 18_446_744_073_709_551_616.0}}),
        json!({"FocusWorkspace": {"reference": {"Index": 256.0}}}),
        json!({"SetColumnWidth": {"change": {"AdjustFixed": -12.5}}}),
        json!({"SetColumnWidth": {"change": {"AdjustFixed": -2_147_483_649.0}}}),
    ] {
        let error = client.action(&path, action).await.unwrap_err();
        assert!(
            error.contains("invalid type") || error.contains("invalid value"),
            "{error}"
        );
    }
}
