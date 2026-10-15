use crate::model::{Row, text, number};
use cxx_qt_lib::QVariant;
use inir_types::desktop::*;
impl Row for NiriWindow {
 fn role_names() -> &'static [&'static str] { &["windowId","title","appId","pid","workspaceId","focused","floating","urgent","focusSerial","column","row","tileWidth","tileHeight","windowWidth","windowHeight","tilePosX","tilePosY","tilePosValid","windowOffsetX","windowOffsetY","focusTimestamp","titlePresent","appIdPresent"] }
 fn data(&self, role:i32) -> QVariant { match role - 257 {
0 => number(&self.id),
1 => text(&self.title),
2 => text(&self.app_id),
3 => QVariant::from(&self.pid),
4 => number(&self.workspace_id),
5 => QVariant::from(&self.focused),
6 => QVariant::from(&self.floating),
7 => QVariant::from(&self.urgent),
8 => QVariant::from(&(self.focus_serial as f64)),
9 => QVariant::from(&self.column),
10 => QVariant::from(&self.row),
11 => QVariant::from(&self.tile_width),
12 => QVariant::from(&self.tile_height),
13 => QVariant::from(&self.window_width),
14 => QVariant::from(&self.window_height),
15 => QVariant::from(&self.tile_pos_x),
16 => QVariant::from(&self.tile_pos_y),
17 => QVariant::from(&self.tile_pos_valid),
18 => QVariant::from(&self.window_offset_x),
19 => QVariant::from(&self.window_offset_y),
20 => self.focus_timestamp.map(|(secs,nanos)| text(&format!("{secs}:{nanos}"))).unwrap_or_else(||text("")),
21 => QVariant::from(&self.title_present),
22 => QVariant::from(&self.app_id_present),
_ => QVariant::default(),
} }
}
impl Row for NiriWorkspace {
 fn role_names() -> &'static [&'static str] { &["workspaceId","idx","name","output","active","focused","activeWindowId","urgent","namePresent"] }
 fn data(&self, role:i32) -> QVariant { match role - 257 {
0 => number(&self.id),
1 => QVariant::from(&(self.index as i32)),
2 => text(&self.name),
3 => text(&self.output),
4 => QVariant::from(&self.active),
5 => QVariant::from(&self.focused),
6 => number(&self.active_window_id),
7 => QVariant::from(&self.urgent),
8 => QVariant::from(&self.name_present),
_ => QVariant::default(),
} }
}
impl Row for AccessPoint {
 fn role_names() -> &'static [&'static str] { &["apId","device","ssid","bssid","strength","frequency","rate","security","active"] }
 fn data(&self, role:i32) -> QVariant { match role - 257 {
0 => text(&self.id),
1 => text(&self.device),
2 => text(&self.ssid),
3 => text(&self.bssid),
4 => QVariant::from(&(self.strength as i32)),
5 => QVariant::from(&self.frequency),
6 => QVariant::from(&self.rate),
7 => text(&self.security),
8 => QVariant::from(&self.active),
_ => QVariant::default(),
} }
}
impl Row for Backlight {
 fn role_names() -> &'static [&'static str] { &["deviceId","kind","raw","maximum","value"] }
 fn data(&self, role:i32) -> QVariant { match role - 257 {
0 => text(&self.id),
1 => text(&self.kind),
2 => QVariant::from(&self.raw),
3 => QVariant::from(&self.maximum),
4 => QVariant::from(&self.value),
_ => QVariant::default(),
} }
}
