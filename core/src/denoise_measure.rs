//! Development measurements, not features: the AI models on one RAW with ONNX
//! Runtime's CPU and with CoreML on each of its plans (the GPU, the Neural
//! Engine, both), timed, and how far each answer is from the CPU's. RAWmakase
//! measured CPU and CoreML's GPU equal for AI Denoise on a Mac (its
//! docs/rendering-quality.md); these measure the iPad before a plan is chosen
//! there.

use rawmakase_ai::ai_denoise::{Command, Loader, command, load_model_with};
use rawmakase_ai::model_install::Pack;
use rawmakase_cancel::Cancel;
use rawmakase_inference::{
    CoreMLFormat, CoreMLPlan, CoreMLUnits, Coverage, LoadOptions, Provider, RgbImage, Sky, Subject,
};
use rawmakase_model::camera_data::Decode;
use rawmakase_model::model::recipe::Recipe;
use std::io::{BufReader, Read};
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

/// The CoreML plans measured: the GPU (AI Denoise's choice so far), the Neural
/// Engine, and both, in each of the two graph formats.
const PLANS: [CoreMLPlan; 6] = {
    use CoreMLFormat::{MLProgram, NeuralNetwork};
    use CoreMLUnits::{All, Gpu, NeuralEngine};
    const fn plan(format: CoreMLFormat, units: CoreMLUnits) -> CoreMLPlan {
        CoreMLPlan { format, units }
    }
    [
        plan(MLProgram, Gpu),
        plan(MLProgram, NeuralEngine),
        plan(MLProgram, All),
        plan(NeuralNetwork, Gpu),
        plan(NeuralNetwork, NeuralEngine),
        plan(NeuralNetwork, All),
    ]
};

/// Runs AI Denoise on `raw` on the CPU and on each CoreML plan (the model in
/// `model`, or the installed one), its output dumped in `scratch`; reports each
/// run's time and its difference from the CPU's output, as JSON.
#[uniffi::export]
pub fn measure_denoise_providers(raw: String, model: Option<String>, scratch: String) -> String {
    report(measure_denoise(
        Path::new(&raw),
        model.as_deref().map(Path::new),
        Path::new(&scratch),
    ))
}

/// Runs Select Subject and Select Sky on `raw` on the CPU and on each CoreML plan,
/// with the installed models (downloading a missing one); reports each run's time and its mask's agreement with
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

fn measure_denoise(
    raw: &Path,
    model: Option<&Path>,
    scratch: &Path,
) -> anyhow::Result<serde_json::Value> {
    std::fs::create_dir_all(scratch)?;
    let run = |provider, coreml: Option<CoreMLPlan>, dump: &Path| -> anyhow::Result<f64> {
        let started = Instant::now();
        command(Command {
            input: raw.to_path_buf(),
            model: model.map(Path::to_path_buf),
            provider,
            loader: Some(match coreml {
                Some(plan) => {
                    Arc::new(move |dir: &Path, p| load_model_with(dir, p, Some(plan))) as Loader
                }
                None => crate::neural_denoise::loader(),
            }),
            amount: 1.,
            output: None,
            plain_output: None,
            max_edge: 0,
            dump: Some(dump.to_path_buf()),
            cache: None,
            bench_fit: None,
            camera_dump: None,
            camera_crop: 0,
            nr_luma: vec![],
            bench_plain: false,
        })?;
        Ok(started.elapsed().as_secs_f64())
    };
    // The CPU's output is the reference each plan is compared with, read back from
    // its dump a piece at a time: a 45 MP photo's output is half a gigabyte.
    let reference = scratch.join("CPU.f32");
    let cpu_s = run(Provider::Cpu, None, &reference)?;
    println!("DENOISE CPU took {cpu_s:.1} s");
    let mut runs = vec![serde_json::json!({ "plan": "CPU", "seconds": cpu_s })];
    // The app's own run first: the bundled model on the Neural Engine, through the
    // same pipeline (tiling, gain fit). `SAFELIGHT_MEASURE=:NeuralEngine` measures
    // only the runs whose names contain that part.
    let only = std::env::var("SAFELIGHT_MEASURE").unwrap_or_default();
    let only = only
        .split_once(':')
        .map_or(only.as_str(), |(_, plan)| plan)
        .to_string();
    let candidates = std::iter::once(("NeuralEngine".to_string(), Provider::NeuralEngine, None))
        .chain(PLANS.map(|plan| (plan.name(), Provider::CoreML, Some(plan))))
        .filter(|(name, ..)| name.contains(&only));
    for (name, provider, plan) in candidates {
        let dump = scratch.join("accelerated.f32");
        let result = run(provider, plan, &dump)
            .and_then(|seconds| Ok((seconds, difference(&reference, &dump)?)));
        let _ = std::fs::remove_file(&dump);
        let entry = match result {
            Ok((seconds, d)) => {
                println!(
                    "DENOISE {} took {seconds:.1} s, max diff {:e}, PSNR {:.1} dB",
                    name, d.max_abs, d.psnr_db
                );
                serde_json::json!({
                    "plan": name,
                    "seconds": seconds,
                    "max_abs_diff": d.max_abs,
                    "mean_abs_diff": d.mean_abs,
                    "psnr_db": d.psnr_db,
                    "non_finite": d.non_finite,
                })
            }
            Err(e) => {
                println!("DENOISE {name} failed: {e:#}");
                serde_json::json!({ "plan": name, "error": format!("{e:#}") })
            }
        };
        runs.push(entry);
    }
    let _ = std::fs::remove_file(&reference);
    Ok(serde_json::json!({ "file": raw, "denoise": runs }))
}

/// How far one dump of little-endian `f32`s is from another of the same length.
struct Difference {
    max_abs: f32,
    mean_abs: f64,
    /// Against the reference's largest magnitude.
    psnr_db: f64,
    non_finite: u64,
}

fn difference(reference: &Path, other: &Path) -> anyhow::Result<Difference> {
    let (a, b) = (std::fs::File::open(reference)?, std::fs::File::open(other)?);
    anyhow::ensure!(
        a.metadata()?.len() == b.metadata()?.len(),
        "the outputs differ in size"
    );
    let (mut a, mut b) = (BufReader::new(a), BufReader::new(b));
    let (mut abuf, mut bbuf) = (vec![0u8; 1 << 20], vec![0u8; 1 << 20]);
    let (mut max_abs, mut peak, mut sum, mut squares, mut count, mut non_finite) =
        (0f32, 0f32, 0f64, 0f64, 0u64, 0u64);
    loop {
        let n = read_full(&mut a, &mut abuf)?;
        anyhow::ensure!(read_full(&mut b, &mut bbuf)? == n, "an output ended early");
        if n == 0 {
            break;
        }
        for (x, y) in abuf[..n]
            .as_chunks::<4>()
            .0
            .iter()
            .zip(bbuf[..n].as_chunks::<4>().0)
        {
            let (x, y) = (f32::from_le_bytes(*x), f32::from_le_bytes(*y));
            if !y.is_finite() {
                non_finite += 1;
                continue;
            }
            let d = (x - y).abs();
            max_abs = max_abs.max(d);
            peak = peak.max(x.abs());
            sum += d as f64;
            squares += (d as f64).powi(2);
            count += 1;
        }
    }
    let count = count.max(1) as f64;
    Ok(Difference {
        max_abs,
        mean_abs: sum / count,
        psnr_db: 10. * ((peak as f64).powi(2) / (squares / count).max(1e-30)).log10(),
        non_finite,
    })
}

/// Fills `buf` unless the file ends first; how many bytes were read.
fn read_full(r: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match r.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn measure_selection(raw: &Path) -> anyhow::Result<serde_json::Value> {
    // What the app feeds the models: the photo at its default settings, developed for
    // the selection at the selection's size.
    let image = rawmakase_decode::photo::open(raw)?.develop(Decode::Half, Cancel::NEVER)?;
    let recipe = Recipe::for_metadata(&image.metadata);
    let input = rawmakase_engine::develop::masks::selection_input(
        &image,
        &recipe,
        rawmakase_ai::selection::INPUT_EDGE,
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
    // `SAFELIGHT_MEASURE=sky:NeuralNetwork` measures only the runs whose model and
    // plan names contain those parts, for repeating one.
    let only = std::env::var("SAFELIGHT_MEASURE").unwrap_or_default();
    let (only_model, only_plan) = only.split_once(':').unwrap_or((&only, ""));
    for (name, pack) in [("subject", Pack::Subject), ("sky", Pack::Sky)] {
        if !name.contains(only_model) {
            continue;
        }
        // As Masks > Select Subject's download does, when the model is missing.
        if !pack.is_installed() {
            println!("SELECT installing the {name} model");
            rawmakase_ai::model_install::install(pack, None, &Default::default())
                .map_err(|e| anyhow::anyhow!("installing the {name} model: {e:?}"))?;
        }
        let dir = pack.installed_dir();
        // Loads the model and selects twice: the first run on CoreML pays for
        // compiling the graph, the second is what each later photo costs.
        let select = |options: &LoadOptions| -> anyhow::Result<(f64, f64, f64, Coverage)> {
            let started = Instant::now();
            let mut run: Box<dyn FnMut() -> anyhow::Result<Coverage> + '_> = if pack == Pack::Sky {
                let mut sky = Sky::load_with(&dir, options)?;
                Box::new(move || Ok(sky.select(rgb, Cancel::NEVER)?))
            } else {
                let mut subject = Subject::load_with(&dir, options)?;
                Box::new(move || Ok(subject.analyze(rgb, Cancel::NEVER)?.subject()?))
            };
            let load = started.elapsed().as_secs_f64();
            let started = Instant::now();
            run()?;
            let first = started.elapsed().as_secs_f64();
            let started = Instant::now();
            let coverage = run()?;
            Ok((load, first, started.elapsed().as_secs_f64(), coverage))
        };
        let options = |provider, coreml| LoadOptions {
            provider: Some(provider),
            coreml,
            ..rawmakase_ai::selection::load_options()
        };
        let (load, first, again, reference) = select(&options(Provider::Cpu, None))?;
        println!("SELECT {name} CPU: load {load:.1} s, {first:.2} s, again {again:.2} s");
        let mut runs = vec![serde_json::json!({
            "plan": "CPU", "load_s": load, "first_s": first, "again_s": again,
            "covered": covered(&reference),
        })];
        for plan in PLANS.into_iter().filter(|p| p.name().contains(only_plan)) {
            let entry = match select(&options(Provider::CoreML, Some(plan))) {
                Ok((load, first, again, coverage)) => {
                    let iou = iou(&reference, &coverage);
                    println!(
                        "SELECT {name} {}: load {load:.1} s, {first:.2} s, again {again:.2} s, IoU {iou:.4}",
                        plan.name()
                    );
                    serde_json::json!({
                        "plan": plan.name(), "load_s": load, "first_s": first, "again_s": again,
                        "iou": iou, "covered": covered(&coverage),
                        "max_coverage_diff": max_diff(&reference, &coverage),
                    })
                }
                Err(e) => {
                    println!("SELECT {name} {} failed: {e:#}", plan.name());
                    serde_json::json!({ "plan": plan.name(), "error": format!("{e:#}") })
                }
            };
            runs.push(entry);
        }
        // Subject: the app's own encoder and DETR with Core ML, with the CPU's
        // decoder, as Select Subject runs them (`subject:bundled` alone, with the CPU).
        if pack == Pack::Subject && (only_plan.is_empty() || only_plan == "bundled") {
            let entry = match crate::subject_coreml::CoreMLSubject::bundled() {
                None => serde_json::json!({ "plan": "NeuralEngine", "error": "no bundled model" }),
                Some(mut neural) => {
                    let mut session = Subject::load_with(&dir, &options(Provider::Cpu, None))?;
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
        // Sky: the app's own model on the Neural Engine, as Select Sky runs it
        // (`SAFELIGHT_MEASURE=sky:bundled` measures it alone, with the CPU).
        if pack == Pack::Sky && (only_plan.is_empty() || only_plan == "bundled") {
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
    use rawmakase_inference::Coverage;

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

    #[test]
    fn dumps_differ_by_their_values() {
        let dir = tempfile::tempdir().unwrap();
        let write = |name: &str, values: &[f32]| {
            let path = dir.path().join(name);
            let bytes: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
            std::fs::write(&path, bytes).unwrap();
            path
        };
        let a = write("a", &[1., 0.5, 0.25]);
        let b = write("b", &[1., 0.75, f32::NAN]);
        let d = super::difference(&a, &b).unwrap();
        assert_eq!(d.max_abs, 0.25);
        assert_eq!(d.non_finite, 1);
    }

    /// Needs `SAFELIGHT_DENOISE_RAW` (a Bayer RAW) and the model installed
    /// where RAWmakase installs it.
    #[test]
    #[ignore = "needs SAFELIGHT_DENOISE_RAW and the AI Denoise model"]
    fn cpu_and_coreml_agree() {
        let raw = std::env::var("SAFELIGHT_DENOISE_RAW").unwrap();
        let dir = tempfile::tempdir().unwrap();
        let report = super::measure_denoise_providers(raw, None, dir.path().display().to_string());
        println!("{report}");
        assert!(!report.contains("\"error\""), "{report}");
    }
}
