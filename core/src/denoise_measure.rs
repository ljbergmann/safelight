//! Development measurements, not features: Select Subject and Select Sky on one
//! RAW with ONNX Runtime's CPU and with the app's own Neural Engine models, timed,
//! and how far each mask is from the CPU's. MEASUREMENTS.md has what was measured,
//! AI Denoise's providers and CoreML's plans among it (those tools went with
//! RAWmakase 0.8, whose AI Denoise benchmark runs RAWmakase's own loader only).

use crate::models::Pack;
use rawmakase_cancel::Cancel;
use rawmakase_inference::{LoadOptions, Provider};
use rawmakase_model::camera_data::Decode;
use rawmakase_model::model::recipe::Recipe;
use rawmakase_selection::{Coverage, RgbImage, Sky, Subject};
use std::path::Path;
use std::time::Instant;

/// Runs Select Subject and Select Sky on `raw` on the CPU and with the app's
/// Neural Engine models, with the installed models (downloading a missing one); reports each run's time and its mask's agreement with
/// the CPU's (intersection over union), as JSON.
#[uniffi::export]
pub fn measure_selection_providers(raw: String) -> String {
    report(measure_selection(Path::new(&raw)))
}

fn report(result: anyhow::Result<serde_json::Value>) -> String {
    match result {
        Ok(report) => report.to_string(),
        Err(e) => serde_json::json!({ "error": format!("{e:#}") }).to_string(),
    }
}

fn measure_selection(raw: &Path) -> anyhow::Result<serde_json::Value> {
    // What the app feeds the models: the photo at its default settings, developed for
    // the selection at the selection's size.
    let image = rawmakase_decode::photo::open(raw)?.develop(Decode::Half, Cancel::NEVER)?;
    let recipe = Recipe::for_metadata(&image.metadata);
    let input = rawmakase_engine::develop::masks::selection_input(
        &image,
        &recipe,
        crate::from_desktop::selection::INPUT_EDGE,
        Cancel::NEVER,
    )?;
    let rgb = RgbImage {
        width: input.width as usize,
        height: input.height as usize,
        data: input.rgb8(),
    };
    let rgb = &rgb;
    let mut report = serde_json::Map::new();
    report.insert("file".into(), serde_json::json!(raw));
    // `SAFELIGHT_MEASURE=sky` measures only the model whose name contains it.
    let only = std::env::var("SAFELIGHT_MEASURE").unwrap_or_default();
    let only_model = only
        .split_once(':')
        .map_or(only.as_str(), |(model, _)| model);
    for (name, pack) in [("subject", Pack::Subject), ("sky", Pack::Sky)] {
        if !name.contains(only_model) {
            continue;
        }
        // As Masks > Select Subject's download does, when the model is missing.
        if !pack.is_installed() {
            println!("SELECT installing the {name} model");
            crate::models::install(pack, None, &Default::default())
                .map_err(|e| anyhow::anyhow!("installing the {name} model: {e:?}"))?;
        }
        let dir = pack.installed_dir();
        // Loads the model and selects twice: the first run on CoreML pays for
        // compiling the graph, the second is what each later photo costs.
        let select = |options: &LoadOptions| -> anyhow::Result<(f64, f64, f64, Coverage)> {
            let started = Instant::now();
            let loaded: anyhow::Result<Box<dyn FnMut() -> anyhow::Result<Coverage> + '_>> = if pack
                == Pack::Sky
            {
                Sky::load_with(&dir, options).map(|mut sky| {
                    Box::new(move || Ok(sky.select(rgb, Cancel::NEVER)?)) as Box<dyn FnMut() -> _>
                })
            } else {
                Subject::load_with(&dir, options).map(|mut subject| {
                    Box::new(move || Ok(subject.analyze(rgb, Cancel::NEVER)?.subject()?))
                        as Box<dyn FnMut() -> _>
                })
            }
            .map_err(Into::into);
            let mut run = loaded?;
            let load = started.elapsed().as_secs_f64();
            let started = Instant::now();
            run()?;
            let first = started.elapsed().as_secs_f64();
            let started = Instant::now();
            let coverage = run()?;
            Ok((load, first, started.elapsed().as_secs_f64(), coverage))
        };
        let options = |provider| LoadOptions {
            provider: Some(provider),
            ..crate::from_desktop::selection::load_options()
        };
        let (load, first, again, reference) = select(&options(Provider::Cpu))?;
        println!("SELECT {name} CPU: load {load:.1} s, {first:.2} s, again {again:.2} s");
        let mut runs = vec![serde_json::json!({
            "plan": "CPU", "load_s": load, "first_s": first, "again_s": again,
            "covered": covered(&reference),
        })];
        // Subject: the app's own encoder and DETR with Core ML, with the CPU's
        // decoder, as Select Subject runs them.
        if pack == Pack::Subject {
            let entry = match crate::subject_coreml::CoreMLSubject::bundled() {
                None => serde_json::json!({ "plan": "NeuralEngine", "error": "no bundled model" }),
                Some(mut neural) => {
                    let mut session = Subject::load_with(&dir, &options(Provider::Cpu))?;
                    let mut timed = || -> anyhow::Result<(f64, Option<Coverage>)> {
                        let started = Instant::now();
                        let Some((embedding, labels)) = neural.inputs(rgb)? else {
                            return Ok((0., None));
                        };
                        let analysis = session.analyze_from(embedding, labels, Cancel::NEVER)?;
                        Ok((started.elapsed().as_secs_f64(), Some(analysis.subject()?)))
                    };
                    match (timed(), timed()) {
                        (Ok((first, _)), Ok((again, Some(coverage)))) => {
                            let iou = iou(&reference, &coverage);
                            println!(
                                "SELECT subject Core ML (bundled): {first:.2} s, again {again:.2} s, IoU {iou:.4}"
                            );
                            serde_json::json!({
                                "plan": "NeuralEngine", "first_s": first, "again_s": again,
                                "iou": iou, "covered": covered(&coverage),
                                "max_coverage_diff": max_diff(&reference, &coverage),
                            })
                        }
                        (Ok(_), Ok((_, None))) => serde_json::json!({
                            "plan": "NeuralEngine", "error": "the photo's shape has no DETR function",
                        }),
                        (Err(e), _) | (_, Err(e)) => {
                            serde_json::json!({ "plan": "NeuralEngine", "error": format!("{e:#}") })
                        }
                    }
                }
            };
            runs.push(entry);
        }
        // Sky: the app's own model on the Neural Engine, as Select Sky runs it.
        if pack == Pack::Sky {
            let entry = match crate::neural_sky::NeuralSky::bundled() {
                None => serde_json::json!({ "plan": "NeuralEngine", "error": "no bundled model" }),
                Some(mut neural) => {
                    let mut timed = || -> anyhow::Result<(f64, Option<Coverage>)> {
                        let started = Instant::now();
                        let coverage = neural.select(rgb)?;
                        Ok((started.elapsed().as_secs_f64(), coverage))
                    };
                    match (timed(), timed()) {
                        (Ok((first, _)), Ok((again, Some(coverage)))) => {
                            let iou = iou(&reference, &coverage);
                            println!(
                                "SELECT sky NeuralEngine (bundled): {first:.2} s, again {again:.2} s, IoU {iou:.4}"
                            );
                            serde_json::json!({
                                "plan": "NeuralEngine", "first_s": first, "again_s": again,
                                "iou": iou, "covered": covered(&coverage),
                                "max_coverage_diff": max_diff(&reference, &coverage),
                            })
                        }
                        (Ok(_), Ok((_, None))) => serde_json::json!({
                            "plan": "NeuralEngine", "error": "the photo's shape has no function",
                        }),
                        (Err(e), _) | (_, Err(e)) => {
                            serde_json::json!({ "plan": "NeuralEngine", "error": format!("{e:#}") })
                        }
                    }
                }
            };
            runs.push(entry);
        }
        report.insert(name.into(), runs.into());
    }
    Ok(report.into())
}

/// The share of the frame a mask selects (coverage at least half).
fn covered(c: &Coverage) -> f64 {
    c.data.iter().filter(|v| **v >= 128).count() as f64 / c.data.len().max(1) as f64
}

/// Intersection over union of two masks, each selecting where its coverage is at
/// least half; 1 when both select nothing.
fn iou(a: &Coverage, b: &Coverage) -> f64 {
    if (a.width, a.height) != (b.width, b.height) {
        return 0.;
    }
    let (mut both, mut either) = (0usize, 0usize);
    for (x, y) in a.data.iter().zip(&b.data) {
        let (x, y) = (*x >= 128, *y >= 128);
        both += usize::from(x && y);
        either += usize::from(x || y);
    }
    if either == 0 {
        1.
    } else {
        both as f64 / either as f64
    }
}

fn max_diff(a: &Coverage, b: &Coverage) -> u8 {
    a.data
        .iter()
        .zip(&b.data)
        .map(|(x, y)| x.abs_diff(*y))
        .max()
        .unwrap_or(u8::MAX)
}

#[cfg(test)]
mod tests {
    use rawmakase_selection::Coverage;

    fn mask(data: &[u8]) -> Coverage {
        Coverage {
            width: data.len(),
            height: 1,
            data: data.to_vec(),
        }
    }

    #[test]
    fn masks_agree_by_what_each_selects() {
        assert_eq!(super::iou(&mask(&[0, 0]), &mask(&[0, 0])), 1.);
        assert_eq!(
            super::iou(&mask(&[255, 255, 0, 0]), &mask(&[255, 0, 0, 0])),
            0.5
        );
        assert_eq!(super::iou(&mask(&[200, 0]), &mask(&[0, 200])), 0.);
    }
}
