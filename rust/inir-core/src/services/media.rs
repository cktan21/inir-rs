use super::dbus::{self, read, Properties};
use inir_types::desktop::{MediaPlayer, MediaState, ServiceStatus};
use zbus::Connection;

pub async fn snapshot(bus: &Connection) -> zbus::Result<MediaState> {
    let names: Vec<String> = dbus::proxy(
        bus,
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
    )
    .await?
    .call("ListNames", &())
    .await?;
    let mut state = MediaState {
        status: ServiceStatus {
            ready: true,
            error: String::new(),
        },
        ..Default::default()
    };
    for name in names
        .into_iter()
        .filter(|n| n.starts_with("org.mpris.MediaPlayer2."))
    {
        let Ok(p) = dbus::properties(
            bus,
            &name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2.Player",
        )
        .await
        else {
            continue;
        };
        let identity = dbus::properties(
            bus,
            &name,
            "/org/mpris/MediaPlayer2",
            "org.mpris.MediaPlayer2",
        )
        .await?;
        let metadata: Properties = read(&p, "Metadata");
        state.players.push(MediaPlayer {
            id: name,
            identity: read(&identity, "Identity"),
            playback_status: read(&p, "PlaybackStatus"),
            title: read(&metadata, "xesam:title"),
            artist: read::<Vec<String>>(&metadata, "xesam:artist").join(", "),
            art_url: read(&metadata, "mpris:artUrl"),
            length: read(&metadata, "mpris:length"),
            can_control: read(&p, "CanControl"),
            can_seek: read(&p, "CanSeek"),
        });
    }
    state.players.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(state)
}

pub async fn execute(bus: &Connection, player: &str, method: &str) -> Result<(), String> {
    if !player.starts_with("org.mpris.MediaPlayer2.")
        || !matches!(
            method,
            "Play" | "Pause" | "PlayPause" | "Stop" | "Next" | "Previous"
        )
    {
        return Err("invalid media command".into());
    }
    dbus::proxy(
        bus,
        player,
        "/org/mpris/MediaPlayer2",
        "org.mpris.MediaPlayer2.Player",
    )
    .await
    .map_err(|e| e.to_string())?
    .call::<_, _, ()>(method, &())
    .await
    .map_err(|e| e.to_string())
}
