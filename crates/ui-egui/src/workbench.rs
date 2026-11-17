//! The UI-only commands Workbench sends when PdfCraft runs in its side panel: `view.workbench`
//! (Workbench's look) and `view.handsOff` (only the pages, while an agent does the work).
//! They take parameters, so they run through the control channel's `ui.command` like `ui.panel`.

use serde_json::{Value, json};

use crate::PdfCraftApp;
use crate::theme;

/// Workbench's look: on or off, with an accent.
pub(crate) const WORKBENCH: &str = "view.workbench";
/// Hands off: only the page view, no window chrome.
pub(crate) const HANDS_OFF: &str = "view.handsOff";

/// Workbench's default accent when it names none.
const DEFAULT_ACCENT: egui::Color32 = egui::Color32::from_rgb(0xFF, 0x60, 0x00);

/// Whether `id` is one of these commands.
pub(crate) fn handles(id: &str) -> bool {
    matches!(id, WORKBENCH | HANDS_OFF)
}

/// The `ui.commands` entries for these commands.
pub(crate) fn listing() -> [Value; 2] {
    [
        json!({"id": WORKBENCH, "label": "Workbench Look", "enabled": true, "params": "{on?: bool, accent?: \"#rrggbb\"}"}),
        json!({"id": HANDS_OFF, "label": "Hands Off", "enabled": true, "params": "{on?: bool}: only the pages, no menus, toolbars or panels"}),
    ]
}

/// Run `id` with `params` (`null` or `{}`: toggle).
pub(crate) fn command(app: &mut PdfCraftApp, id: &str, params: &Value) -> Result<Value, String> {
    let on = match params.get("on") {
        None | Some(Value::Null) => None,
        Some(Value::Bool(b)) => Some(*b),
        Some(_) => return Err(format!("{id}: on must be true or false")),
    };
    match id {
        WORKBENCH => {
            let accent = match params.get("accent") {
                None | Some(Value::Null) => None,
                Some(v) => Some(v.as_str().and_then(parse_hex).ok_or_else(|| format!("{id}: accent must be a colour like \"#ff6000\""))?),
            };
            let on = on.unwrap_or(theme::workbench_accent().is_none());
            theme::set_workbench(on.then(|| accent.or_else(theme::workbench_accent).unwrap_or(DEFAULT_ACCENT)));
            if let Some(ctx) = app.ctx.clone() {
                theme::apply(&ctx, app.theme);
                ctx.request_repaint();
            }
            Ok(json!({"on": on}))
        }
        HANDS_OFF => {
            app.hands_off = on.unwrap_or(!app.hands_off);
            if let Some(ctx) = &app.ctx {
                ctx.request_repaint();
            }
            Ok(json!({"on": app.hands_off}))
        }
        _ => Err(format!("unknown command {id:?} (see ui.commands)")),
    }
}

/// `#rrggbb` to a colour.
fn parse_hex(s: &str) -> Option<egui::Color32> {
    let h = s.strip_prefix('#')?;
    if h.len() != 6 || !h.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(h, 16).ok()?;
    let [_, r, g, b] = n.to_be_bytes();
    Some(egui::Color32::from_rgb(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_colours_parse_and_bad_ones_do_not() {
        assert_eq!(parse_hex("#FF6000"), Some(egui::Color32::from_rgb(0xFF, 0x60, 0x00)));
        assert_eq!(parse_hex("#0a0b0c"), Some(egui::Color32::from_rgb(0x0A, 0x0B, 0x0C)));
        for bad in ["FF6000", "#FFF", "#+F6000", "#GG0000", "#FF60000", ""] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn hands_off_toggles_and_takes_on() {
        let mut app = PdfCraftApp::new();
        assert_eq!(command(&mut app, HANDS_OFF, &Value::Null), Ok(json!({"on": true})));
        assert_eq!(command(&mut app, HANDS_OFF, &json!({})), Ok(json!({"on": false})));
        assert_eq!(command(&mut app, HANDS_OFF, &json!({"on": false})), Ok(json!({"on": false})));
        assert!(command(&mut app, HANDS_OFF, &json!({"on": "yes"})).is_err());
    }

    #[test]
    fn the_workbench_palette_is_square_and_uses_the_accent() {
        let accent = egui::Color32::from_rgb(1, 2, 3);
        let t = theme::Tokens::workbench(accent);
        assert_eq!(
            (t.radius, t.accent, t.chrome, t.pasteboard),
            (0, accent, egui::Color32::from_rgb(0x12, 0x13, 0x16), egui::Color32::from_rgb(0x06, 0x06, 0x07))
        );
        assert!(t.dark());
    }
}
