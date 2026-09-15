use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Mirrors `DEFAULT_EDGE_OFFSET` in overlayPreferences.ts.
const DEFAULT_EDGE_OFFSET: f64 = 25.0;
const FORMS: &[&str] = &["pill", "bead"];
const SIZES: &[&str] = &["s", "m", "l"];
const PALETTES: &[&str] = &["copper", "graphite", "lagoon", "violet", "custom"];
const ANCHORS: &[&str] = &[
    "top-left",
    "top-center",
    "top-right",
    "center-left",
    "center",
    "center-right",
    "bottom-left",
    "bottom-center",
    "bottom-right",
];

#[derive(Clone, Debug)]
pub struct OverlayPreferences {
    pub form: &'static str,
    pub size: &'static str,
    pub anchor: &'static str,
    pub edge_offset: f64,
}

fn choice(
    value: &Value,
    key: &str,
    choices: &[&'static str],
    default: &'static str,
) -> &'static str {
    choices
        .iter()
        .copied()
        .find(|item| Some(*item) == value[key].as_str())
        .unwrap_or(default)
}

impl OverlayPreferences {
    pub fn from_config(config: &Value) -> Self {
        let value = &config["overlay"];
        Self {
            form: choice(value, "form", FORMS, "pill"),
            size: choice(value, "size", SIZES, "m"),
            anchor: choice(value, "anchor", ANCHORS, "bottom-center"),
            edge_offset: value["edge_offset"]
                .as_f64()
                .filter(|n| (0.0..=512.0).contains(n) && n.fract() == 0.0)
                .unwrap_or(DEFAULT_EDGE_OFFSET),
        }
    }

    pub fn layout(&self, streaming: bool, needs_text: bool) -> Layout {
        if needs_text {
            Layout::Pill
        } else if self.form == "bead" {
            Layout::Bead
        } else if streaming {
            Layout::Streaming
        } else {
            Layout::Pill
        }
    }

    pub fn window_size(&self, layout: Layout) -> (f64, f64) {
        match (layout, self.size) {
            (Layout::Pill, "s") => (280.0, 60.0),
            (Layout::Pill, "l") => (360.0, 72.0),
            (Layout::Pill, _) => (308.0, 64.0),
            (Layout::Bead, "s") => (64.0, 64.0),
            (Layout::Bead, "l") => (80.0, 80.0),
            (Layout::Bead, _) => (72.0, 72.0),
            (Layout::Streaming, "s") => (520.0, 138.0),
            (Layout::Streaming, "l") => (680.0, 174.0),
            (Layout::Streaming, _) => (600.0, 150.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Layout {
    Pill,
    Bead,
    Streaming,
}

// Loading repairs known retired values in memory; only a successful save writes them.
pub fn migrate(config: &mut Value) {
    if let Some(palette) = config
        .get_mut("overlay")
        .and_then(|overlay| overlay.get_mut("palette"))
    {
        if matches!(palette.as_str(), Some("accent" | "coal" | "amber")) {
            *palette = Value::String("copper".into());
        }
    }
}

pub fn validate(config: &Value) -> Result<(), String> {
    let Some(value) = config.get("overlay") else {
        return Ok(());
    };
    if !value.is_object() {
        return Err("overlay must be an object".into());
    }
    for (key, values) in [
        ("form", FORMS),
        ("size", SIZES),
        ("palette", PALETTES),
        ("anchor", ANCHORS),
    ] {
        if let Some(raw) = value.get(key) {
            if !raw.as_str().is_some_and(|s| values.contains(&s)) {
                return Err(format!("Invalid overlay.{key}"));
            }
        }
    }
    for (key, max, exclusive, integer) in [
        ("palette_hue", 360.0, true, false),
        ("palette_chroma", 0.2, false, false),
        ("edge_offset", 512.0, false, true),
    ] {
        if let Some(raw) = value.get(key) {
            if !raw.as_f64().is_some_and(|n| {
                n.is_finite()
                    && n >= 0.0
                    && (if exclusive { n < max } else { n <= max })
                    && (!integer || n.fract() == 0.0)
            }) {
                return Err(format!("Invalid overlay.{key}"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn old_or_invalid_geometry_uses_defaults() {
        for value in [
            json!({}),
            json!({"overlay": false}),
            json!({"overlay": {"form":"wrong", "size":3,"anchor":"bad","edge_offset":-1}}),
        ] {
            let p = OverlayPreferences::from_config(&value);
            assert_eq!(p.layout(false, false), Layout::Pill);
            assert_eq!(p.window_size(Layout::Pill), (308.0, 64.0));
            assert_eq!(p.anchor, "bottom-center");
            assert_eq!(p.edge_offset, 25.0);
        }
    }

    #[test]
    fn explicit_offset_keeps_its_saved_value() {
        let p = OverlayPreferences::from_config(&json!({"overlay":{"edge_offset":20}}));
        assert_eq!(p.edge_offset, 20.0);
    }

    #[test]
    fn bead_hides_streaming_but_reveals_errors() {
        let p = OverlayPreferences::from_config(&json!({"overlay":{"form":"bead"}}));
        assert_eq!(p.layout(true, false), Layout::Bead);
        assert_eq!(p.layout(true, true), Layout::Pill);
        assert_eq!(p.layout(false, false), Layout::Bead);
    }

    #[test]
    fn rejects_invalid_preferences() {
        for patch in [
            json!(null),
            json!({"form":"circle"}),
            json!({"palette":"bad"}),
            json!({"size":"xl"}),
            json!({"anchor":"left"}),
            json!({"palette_hue":360}),
            json!({"palette_hue":-1}),
            json!({"palette_chroma":0.21}),
            json!({"edge_offset":513}),
            json!({"edge_offset":0.5}),
        ] {
            assert!(validate(&json!({"overlay":patch})).is_err());
        }
        assert!(validate(&json!({"overlay":{"palette":"custom","palette_hue":359.9,"palette_chroma":0.2,"edge_offset":512}})).is_ok());
    }
}
