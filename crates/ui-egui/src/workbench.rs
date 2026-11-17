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

/// The comment commands that, given where (`page` and `rect`, `quads` or `at`), add the comment rather than pick the
/// tool: Claude marks up a PDF by command.
const ANNOTATE: [&str; 7] =
    ["comment.highlight", "comment.underline", "comment.strikeout", "comment.squiggly", "comment.note", "comment.square", "comment.freetext"];

/// Whether `id` (with `params`) is one of these commands.
pub(crate) fn handles(id: &str, params: Option<&Value>) -> bool {
    matches!(id, WORKBENCH | HANDS_OFF) || (ANNOTATE.contains(&id) && params.is_some_and(placed))
}

/// Whether comment params say where the comment goes.
fn placed(params: &Value) -> bool {
    ["rect", "rects", "quads", "at"].iter().any(|k| params.get(*k).is_some())
}

/// The `ui.commands` entries for these commands.
pub(crate) fn listing() -> [Value; 3] {
    [
        json!({"id": WORKBENCH, "label": "Workbench Look", "enabled": true, "params": "{on?: bool, accent?: \"#rrggbb\"}"}),
        json!({"id": HANDS_OFF, "label": "Hands Off", "enabled": true, "params": "{on?: bool}: only the pages, no menus, toolbars or panels"}),
        json!({"id": "comment.*", "label": "Add a comment by command", "enabled": true, "params": ANNOTATE_PARAMS}),
    ]
}

const ANNOTATE_PARAMS: &str = "comment.highlight | comment.underline | comment.strikeout | comment.squiggly {page?, quads: [[x1,y1,x2,y2,x3,y3,x4,y4]] (top-left, top-right, bottom-left, bottom-right) or rect: [x0,y0,x1,y1] or rects: [[x0,y0,x1,y1]], text?, color?, opacity?}; comment.note {page?, at: [x,y] (its top-left) or rect, text, color?}; comment.square {page?, rect, color?, fill?, width?, text?}; comment.freetext {page?, rect, text, fontSize? (12), color?}. PDF points from the page's bottom-left; page is 1-based (default: the current page); color \"#rrggbb\" or [r,g,b] 0..1. Without a place these pick the tool instead";

/// Run `id` with `params` (`null` or `{}`: toggle).
pub(crate) fn command(app: &mut PdfCraftApp, id: &str, params: &Value) -> Result<Value, String> {
    if ANNOTATE.contains(&id) {
        return annotate(app, id, params);
    }
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

/// Add the comment `id` describes with `params` to the active document (one undo step, as drawn by hand).
fn annotate(app: &mut PdfCraftApp, id: &str, params: &Value) -> Result<Value, String> {
    use crate::tools::comments::CommentTool;
    use pdfcraft_engine::{Edit, Markup, NewAnnotation, NoteIcon, Shape};

    let Some((view, doc_id)) = app.active_ids() else { return Err(format!("{id}: no document is open")) };
    let pages = app.session.get(doc_id).map_or(0, |d| d.info.pages.len());
    let page = match params.get("page") {
        None | Some(Value::Null) => app.views[view].current,
        Some(v) => match v.as_u64() {
            Some(n) if n >= 1 && (n as usize) <= pages => n as usize - 1,
            _ => return Err(format!("{id}: page must be 1..{pages}")),
        },
    };
    let numbers = |v: &Value, n: usize, what: &str| -> Result<Vec<f64>, String> {
        let a: Option<Vec<f64>> = v.as_array().filter(|a| a.len() == n).and_then(|a| a.iter().map(Value::as_f64).collect());
        a.filter(|a| a.iter().all(|x| x.is_finite())).ok_or_else(|| format!("{id}: {what} must be {n} numbers"))
    };
    let rect = |key: &str| -> Result<Option<[f64; 4]>, String> {
        params.get(key).map(|v| numbers(v, 4, "rect").map(|r| [r[0], r[1], r[2], r[3]])).transpose()
    };
    let colour = |key: &str| -> Result<Option<[f64; 3]>, String> {
        match params.get(key) {
            None | Some(Value::Null) => Ok(None),
            Some(Value::String(s)) => {
                let c = parse_hex(s).ok_or_else(|| format!("{id}: {key} must be \"#rrggbb\" or [r, g, b]"))?;
                Ok(Some([c.r(), c.g(), c.b()].map(|x| f64::from(x) / 255.0)))
            }
            Some(v) => numbers(v, 3, key).map(|c| Some([c[0], c[1], c[2]])),
        }
    };
    let text = params.get("text").or_else(|| params.get("contents")).and_then(Value::as_str).unwrap_or("").to_string();
    // a box as a quad: top-left, top-right, bottom-left, bottom-right
    let quad = |r: [f64; 4]| {
        let [x0, y0, x1, y1] = [r[0].min(r[2]), r[1].min(r[3]), r[0].max(r[2]), r[1].max(r[3])];
        [x0, y1, x1, y1, x0, y0, x1, y0]
    };
    let need_rect = || rect("rect")?.ok_or_else(|| format!("{id}: rect [x0, y0, x1, y1] is required"));

    let markup = match id {
        "comment.highlight" => Some((Markup::Highlight, CommentTool::Highlight)),
        "comment.underline" => Some((Markup::Underline, CommentTool::Underline)),
        "comment.strikeout" => Some((Markup::StrikeOut, CommentTool::StrikeOut)),
        "comment.squiggly" => Some((Markup::Squiggly, CommentTool::Squiggly)),
        _ => None,
    };
    let (shape, tool) = if let Some((kind, tool)) = markup {
        let mut quads: Vec<[f64; 8]> = Vec::new();
        if let Some(q) = params.get("quads") {
            for q in q.as_array().ok_or_else(|| format!("{id}: quads must be a list of 8-number quads"))? {
                let q = numbers(q, 8, "each quad")?;
                quads.push([q[0], q[1], q[2], q[3], q[4], q[5], q[6], q[7]]);
            }
        }
        if let Some(r) = rect("rect")? {
            quads.push(quad(r));
        }
        if let Some(rs) = params.get("rects") {
            for r in rs.as_array().ok_or_else(|| format!("{id}: rects must be a list of [x0, y0, x1, y1]"))? {
                let r = numbers(r, 4, "each rect")?;
                quads.push(quad([r[0], r[1], r[2], r[3]]));
            }
        }
        if quads.is_empty() {
            return Err(format!("{id}: give quads, rect or rects"));
        }
        (Shape::TextMarkup { kind, quads }, tool)
    } else {
        match id {
            "comment.note" => {
                let at = match (params.get("at"), rect("rect")?) {
                    (Some(v), _) => numbers(v, 2, "at").map(|a| [a[0], a[1]])?,
                    (None, Some(r)) => [r[0].min(r[2]), r[1].max(r[3])],
                    (None, None) => return Err(format!("{id}: at [x, y] or rect is required")),
                };
                (Shape::Note { at, icon: NoteIcon::default() }, CommentTool::Note)
            }
            "comment.square" => (Shape::Rectangle { rect: need_rect()? }, CommentTool::Rectangle),
            _ => {
                let font_size = params.get("fontSize").and_then(Value::as_f64).filter(|s| s.is_finite() && *s > 0.0).unwrap_or(12.0);
                if text.is_empty() {
                    return Err(format!("{id}: text is required"));
                }
                (Shape::TextBox { rect: need_rect()?, font_size }, CommentTool::TextBox)
            }
        }
    };
    let mut style = app.comment_prefs.style(tool);
    if let Some(c) = colour("color")? {
        style.color = c;
    }
    if let Some(c) = colour("fill")? {
        style.fill = Some(c);
    }
    if let Some(o) = params.get("opacity").and_then(Value::as_f64) {
        style.opacity = o.clamp(0.0, 1.0);
    }
    if let Some(w) = params.get("width").and_then(Value::as_f64).filter(|w| w.is_finite() && *w >= 0.0) {
        style.width = w;
    }
    let edit = Edit::AddAnnotation(NewAnnotation { page, shape, style, contents: text, author: app.comment_prefs.author.clone() });
    if !app.apply_edit(edit) {
        return Err(app.toast.as_ref().map_or_else(|| format!("{id}: the comment could not be added"), |t| t.0.clone()));
    }
    let comments = app.session.get(doc_id).map_or(0, |d| d.info.annotations.len());
    if let Some(ctx) = &app.ctx {
        ctx.request_repaint();
    }
    Ok(json!({"added": id, "page": page + 1, "comments": comments}))
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

    /// Workbench: a highlight, a note, a rectangle and a text box added by command, with their params.
    #[test]
    fn comments_are_added_by_command_with_their_params() {
        let mut app = PdfCraftApp::new();
        app.create_blank();
        assert!(handles("comment.highlight", Some(&json!({"rect": [72, 700, 300, 714]}))));
        assert!(!handles("comment.highlight", Some(&json!({}))), "without a place it picks the tool");
        let count = |app: &PdfCraftApp| app.active_ids().and_then(|(_, id)| app.session.get(id)).map_or(0, |d| d.info.annotations.len());
        let runs = [
            ("comment.highlight", json!({"page": 1, "rect": [72, 700, 300, 714], "color": "#ffee00", "text": "Check this"})),
            ("comment.underline", json!({"quads": [[72, 690, 300, 690, 72, 680, 300, 680]]})),
            ("comment.note", json!({"page": 1, "at": [400, 700], "text": "A note"})),
            ("comment.square", json!({"rect": [72, 500, 200, 600], "color": [1, 0, 0]})),
            ("comment.freetext", json!({"rect": [72, 400, 300, 440], "text": "Hello", "fontSize": 14})),
        ];
        for (n, (id, params)) in runs.iter().enumerate() {
            let out = command(&mut app, id, params).unwrap_or_else(|e| panic!("{id}: {e}"));
            assert_eq!(out["page"], 1, "{id}");
            assert_eq!(count(&app), n + 1, "{id} added one comment");
        }
        assert!(command(&mut app, "comment.highlight", &json!({"page": 9, "rect": [0, 0, 10, 10]})).unwrap_err().contains("page must be 1..1"));
        assert!(command(&mut app, "comment.freetext", &json!({"rect": [72, 400, 300, 440]})).unwrap_err().contains("text is required"));
        assert!(command(&mut app, "comment.square", &json!({"rect": [1, 2]})).unwrap_err().contains("rect must be 4 numbers"));
        assert_eq!(count(&app), runs.len());
    }
}
