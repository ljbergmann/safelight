//! What Export names and marks photos with beyond its presets: the user's
//! own filename templates (Lightroom's Filename Template Editor) and
//! watermark presets, made here and kept where RAWmakase keeps them in the
//! data folder (`filename_templates.json`, `watermarks/`), in its formats.
//! Parsing, checking and saving are RAWmakase's ([`TokenTemplate`],
//! [`SavedTemplates`], [`watermark::save`]).

use crate::render::RenderError;
use rawmakase_export::watermark::{self, Anchor, Shadow, Size, Style, Watermark};
use rawmakase_interop::export_settings::{Menu, SavedTemplates, TokenTemplate, menu};

fn failed(message: impl ToString) -> RenderError {
    RenderError::Failed {
        message: message.to_string(),
    }
}

/// A menu of the Filename Template Editor: its name and its tokens' labels.
#[derive(uniffi::Record, Clone, Debug)]
pub struct TokenMenu {
    pub name: String,
    /// Each as the editor's text shows it: `{Filename}`.
    pub tokens: Vec<String>,
}

/// The editor's token menus, as the desktop's.
#[uniffi::export]
pub fn filename_token_menus() -> Vec<TokenMenu> {
    [
        (Menu::ImageName, "Image Name"),
        (Menu::Numbering, "Numbering"),
        (Menu::Date, "Date"),
        (Menu::Metadata, "Metadata"),
        (Menu::Exif, "EXIF"),
        (Menu::Custom, "Custom"),
    ]
    .into_iter()
    .map(|(m, name)| TokenMenu {
        name: name.into(),
        tokens: menu(m).map(|d| format!("{{{}}}", d.label)).collect(),
    })
    .collect()
}

/// A saved filename template, in the editor's form.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct FilenameTemplate {
    pub name: String,
    pub template: String,
}

/// The saved filename templates, by name.
#[uniffi::export]
pub fn filename_templates() -> Result<Vec<FilenameTemplate>, RenderError> {
    Ok(SavedTemplates::load()
        .map_err(failed)?
        .list()
        .into_iter()
        .map(|t| FilenameTemplate {
            name: t.name,
            template: TokenTemplate::parse(&t.tokens).to_display(),
        })
        .collect())
}

/// What is wrong with the editor's `template`, if anything.
#[uniffi::export]
pub fn filename_template_problem(template: String) -> Option<String> {
    if template.trim().is_empty() {
        return Some("Type a template, or insert a token".into());
    }
    TokenTemplate::from_display(&template)
        .err()
        .and_then(|errors| errors.into_iter().next())
        .map(|e| e.message)
}

/// Saves `template` as `name`, replacing the template of that name.
#[uniffi::export]
pub fn save_filename_template(name: String, template: String) -> Result<(), RenderError> {
    let parsed = TokenTemplate::from_display(&template)
        .map_err(|e| failed(e.first().map_or("Not a template", |e| e.message.as_str())))?;
    let mut saved = SavedTemplates::load().map_err(failed)?;
    let name = name.trim();
    if saved.get(name).is_some() {
        saved.update(name, &parsed.to_lightroom()).map_err(failed)
    } else {
        if let Some(problem) = saved.name_problem(name, None) {
            return Err(failed(problem));
        }
        saved.add(name, &parsed.to_lightroom()).map_err(failed)
    }
}

#[uniffi::export]
pub fn delete_filename_template(name: String) -> Result<(), RenderError> {
    SavedTemplates::load()
        .map_err(failed)?
        .delete(&name)
        .map_err(failed)
}

/// A watermark preset as the iPad edits it. Text marks use Inter, the font
/// RAWmakase ships; a preset made on the desktop keeps its own font.
#[derive(uniffi::Record, Clone, Debug, PartialEq)]
pub struct WatermarkPreset {
    pub name: String,
    /// A graphic (an image) rather than text.
    pub graphic: bool,
    pub text: String,
    /// sRGB, 0–1.
    pub color: Vec<f32>,
    pub shadow: bool,
    pub opacity: f32,
    /// The mark's width, a fraction of the photo's.
    pub size: f32,
    /// One of nine points: column and row, each 0 start, 1 centre, 2 end.
    pub anchor_x: u8,
    pub anchor_y: u8,
    /// Insets, fractions of the photo's width and height.
    pub inset: f32,
    /// Quarter turns counter-clockwise.
    pub rotation: u8,
    /// Whether it has an image of its own (a graphic).
    pub has_image: bool,
}

impl WatermarkPreset {
    fn of(w: &Watermark) -> Self {
        Self {
            name: w.name.clone(),
            graphic: w.style == Style::Graphic,
            text: w.text.clone(),
            color: w.color.to_vec(),
            shadow: w.shadow.enabled,
            opacity: w.opacity,
            size: match w.size {
                Size::Proportional(f) => f,
                Size::Fit | Size::Fill => 1.,
            },
            anchor_x: w.anchor.0,
            anchor_y: w.anchor.1,
            inset: w.inset[0],
            rotation: w.rotation,
            has_image: w.image.is_some(),
        }
    }

    /// Over `base` (the preset as saved, so what the iPad doesn't edit stays).
    fn onto(&self, mut base: Watermark) -> Watermark {
        base.name = self.name.trim().to_string();
        base.style = if self.graphic {
            Style::Graphic
        } else {
            Style::Text
        };
        base.text = self.text.clone();
        if let [r, g, b] = self.color[..] {
            base.color = [r, g, b].map(|v| v.clamp(0., 1.));
        }
        base.shadow = Shadow {
            enabled: self.shadow,
            ..base.shadow
        };
        base.opacity = self.opacity.clamp(0., 1.);
        if !matches!(base.size, Size::Fit | Size::Fill) || self.size < 1. {
            base.size = Size::Proportional(self.size.clamp(0.01, 1.));
        }
        base.anchor = Anchor(self.anchor_x.min(2), self.anchor_y.min(2));
        let inset = self.inset.clamp(0., 0.5);
        base.inset = [inset, inset];
        base.rotation = self.rotation % 4;
        base
    }
}

/// A new watermark's settings: Lightroom's defaults.
#[uniffi::export]
pub fn new_watermark() -> WatermarkPreset {
    WatermarkPreset::of(&Watermark::default())
}

/// The saved watermark presets, by name.
#[uniffi::export]
pub fn watermark_presets() -> Vec<WatermarkPreset> {
    watermark::presets()
        .iter()
        .map(WatermarkPreset::of)
        .collect()
}

/// Saves `preset`, in place of the preset `replacing` (which it renames
/// when the name differs); a graphic takes its own copy of `image`, a PNG or
/// JPEG, or keeps the one it has. Everything is checked before any file
/// changes.
#[uniffi::export]
pub fn save_watermark(
    preset: WatermarkPreset,
    replacing: Option<String>,
    image: Option<String>,
) -> Result<WatermarkPreset, RenderError> {
    let base = replacing
        .as_deref()
        .and_then(|r| watermark::presets().into_iter().find(|w| w.name == r))
        .unwrap_or_default();
    let saved = watermark::save(
        &preset.onto(base),
        replacing.as_deref(),
        image.as_deref().map(std::path::Path::new),
    )
    .map_err(|e| failed(format!("{e:#}")))?;
    Ok(WatermarkPreset::of(&saved))
}

#[uniffi::export]
pub fn delete_watermark(name: String) -> Result<(), RenderError> {
    let Some(w) = watermark::presets().into_iter().find(|w| w.name == name) else {
        return Ok(());
    };
    watermark::delete(&w).map_err(|e| failed(format!("{e:#}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The editor's form reads back as Lightroom's and the menus offer
    /// Filename; a broken template says why.
    #[test]
    fn templates_read_in_the_editors_form() {
        let menus = filename_token_menus();
        assert!(menus[0].tokens.contains(&"{Filename}".to_string()));
        assert_eq!(filename_template_problem("{Filename}-x".into()), None);
        assert!(filename_template_problem("{Filename".into()).is_some());
        let unknown = filename_template_problem("{Nothing Like This}".into()).unwrap();
        assert!(unknown.contains("Nothing Like This"), "{unknown}");
        assert!(filename_template_problem(" ".into()).is_some());
    }

    /// What the iPad edits round-trips, and what it doesn't (the font) is
    /// kept from the preset it replaces.
    #[test]
    fn a_preset_keeps_what_the_ipad_doesnt_edit() {
        let base = Watermark {
            family: "Helvetica".into(),
            face: "Bold".into(),
            ..Watermark::default()
        };
        let mut edit = WatermarkPreset::of(&base);
        edit.name = "Mine".into();
        edit.text = "© Me".into();
        edit.anchor_x = 2;
        edit.size = 0.3;
        let w = edit.onto(base);
        assert_eq!((w.family.as_str(), w.face.as_str()), ("Helvetica", "Bold"));
        assert_eq!(w.anchor, Anchor(2, 2));
        assert_eq!(w.size, Size::Proportional(0.3));
        assert_eq!(WatermarkPreset::of(&w), edit);
    }
}
