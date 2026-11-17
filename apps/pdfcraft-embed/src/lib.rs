//! PdfCraft hosted inside Workbench's side panel: the same app as the desktop build, drawn into a view
//! Workbench owns, with Workbench's look and only the pages showing (Hands off) until the user asks for the
//! menus. Workbench (and Claude) drive it through its UI control channel. See `craft-embed`.
//!
//! PdfCraft's control channel is all `ui.*` methods, so the protocol's names map onto them as its web page
//! API does (`apps/pdfcraft-web/src/api.rs`): `engine.execute {command, params}` is `ui.command {id, params}`,
//! `engine.commands` is `ui.commands`, `document.inspect` is `ui.state`; `engine.execute file.open {path}`
//! opens the file (`ui.open`).

use std::sync::mpsc::{Receiver, TryRecvError};

use craft_embed::{EmbedApp, egui};
use pdfcraft_ui_egui::PdfCraftApp;
use pdfcraft_ui_egui::control::{ControlClient, Reply};
use serde_json::{Value, json};

struct Pdf {
    app: PdfCraftApp,
    client: ControlClient,
    /// eframe's `App` methods take a frame; nothing in PdfCraft's needs a real one.
    frame: eframe::Frame,
}

impl EmbedApp for Pdf {
    fn logic(&mut self, ctx: &egui::Context) {
        eframe::App::logic(&mut self.app, ctx, &mut self.frame);
    }
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        eframe::App::raw_input_hook(&mut self.app, ctx, raw);
    }
    fn ui(&mut self, ui: &mut egui::Ui) {
        eframe::App::ui(&mut self.app, ui, &mut self.frame);
    }
    fn control(&mut self, ctx: &egui::Context, request: &str) -> String {
        let req: Value = serde_json::from_str(request).unwrap_or(Value::Null);
        let method = req.get("method").and_then(Value::as_str).unwrap_or("");
        let params = req.get("params").cloned().unwrap_or(json!({}));
        let (method, params) = translate(method, params);
        let rx = self.client.send(&method, params);
        // PdfCraft answers control requests in `logic`; run it now (as eframe does while the window is
        // hidden) so a request it answers at once has its reply before this returns.
        eframe::App::logic(&mut self.app, ctx, &mut self.frame);
        reply(&rx).to_string()
    }
    fn exit(&mut self) {
        eframe::App::on_exit(&mut self.app, None);
    }
}

/// The protocol's method and params as PdfCraft's control channel names them.
fn translate(method: &str, params: Value) -> (String, Value) {
    match method {
        "engine.execute" => {
            let command = params.get("command").and_then(Value::as_str).unwrap_or("");
            let inner = params.get("params").cloned().unwrap_or(Value::Null);
            match inner.get("path").and_then(Value::as_str) {
                Some(path) if command == "file.open" => ("ui.open".into(), json!({"path": path})),
                _ if params.get("id").is_some() => ("ui.command".into(), params),
                _ => ("ui.command".into(), json!({"id": command, "params": inner})),
            }
        }
        "engine.commands" => ("ui.commands".into(), params),
        "document.inspect" => ("ui.state".into(), params),
        other => (other.into(), params),
    }
}

/// The reply as the protocol's `{ok, result}` / `{ok: false, error}`. A request that waits for frames
/// (clicks, typing, a file still opening) is answered later, so it reports `pending`.
fn reply(rx: &Receiver<Reply>) -> Value {
    match rx.try_recv() {
        Ok(Ok(result)) => json!({"ok": true, "result": result}),
        Ok(Err(error)) => json!({"ok": false, "error": error}),
        Err(TryRecvError::Empty) => json!({"ok": true, "result": {"pending": true}}),
        Err(TryRecvError::Disconnected) => json!({"ok": false, "error": "the app dropped the request"}),
    }
}

fn make(ctx: &egui::Context) -> Box<dyn EmbedApp> {
    let mut app = PdfCraftApp::new();
    let client = app.attach_control(ctx);
    // Workbench's look and Hands off from the first frames, through the control channel as Workbench sends
    // them; Workbench sets the mode's accent right after
    for id in ["view.workbench", "view.handsOff"] {
        let _ = client.send("ui.command", json!({"id": id, "params": {"on": true}}));
    }
    Box::new(Pdf { app, client, frame: eframe::Frame::_new_kittest() })
}

craft_embed::export_embed!(make);
