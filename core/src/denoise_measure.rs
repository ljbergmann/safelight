//! A development measurement, not a feature: AI Denoise on one RAW with
//! ONNX Runtime's CPU and its CoreML provider, timed, and how far apart
//! their outputs are. RAWmakase measured the two equal on a Mac (its
//! docs/rendering-quality.md); this measures the iPad before CoreML is
//! chosen there. The run is RAWmakase's `rawmakase denoise` command.

use rawmakase_ai::ai_denoise::{Command, command};
use rawmakase_inference::runtime::Provider;
use std::path::Path;
use std::time::Instant;

/// Runs AI Denoise on `raw` with each provider (the model in `model`, or
/// the installed one), its output dumped in `scratch`; reports each run's
/// time and the outputs' difference, as JSON.
#[uniffi::export]
pub fn measure_denoise_providers(raw: String, model: Option<String>, scratch: String) -> String {
    match measure(Path::new(&raw), model.as_deref().map(Path::new), Path::new(&scratch)) {
        Ok(report) => report,
        Err(e) => format!("{{\"error\": {:?}}}", format!("{e:#}")),
    }
}

fn measure(raw: &Path, model: Option<&Path>, scratch: &Path) -> anyhow::Result<String> {
    std::fs::create_dir_all(scratch)?;
    let mut runs = vec![];
    // CoreML first: on an iPad the CPU's run takes many minutes.
    for provider in [Provider::CoreML, Provider::Cpu] {
        let dump = scratch.join(format!("{}.f32", provider.name()));
        let started = Instant::now();
        command(Command {
            input: raw.to_path_buf(),
            model: model.map(Path::to_path_buf),
            provider,
            amount: 1.,
            output: None,
            plain_output: None,
            max_edge: 0,
            dump: Some(dump.clone()),
            cache: None,
            bench_fit: None,
            camera_dump: None,
            camera_crop: 0,
            nr_luma: vec![],
            bench_plain: false,
        })?;
        let seconds = started.elapsed().as_secs_f64();
        let values: Vec<f32> = std::fs::read(&dump)?
            .chunks_exact(4)
            .map(|b| f32::from_le_bytes([b[0], b[1], b[2], b[3]]))
            .collect();
        println!("DENOISE {} took {seconds:.1} s", provider.name());
        runs.push((provider.name(), seconds, values));
    }
    let (coreml, cpu) = (&runs[0].2, &runs[1].2);
    anyhow::ensure!(cpu.len() == coreml.len(), "the outputs differ in size");
    let mut max = 0f32;
    let mut sum = 0f64;
    let mut peak = 0f32;
    for (a, b) in cpu.iter().zip(coreml) {
        let d = (a - b).abs();
        max = max.max(d);
        sum += d as f64;
        peak = peak.max(a.abs());
    }
    let mean = sum / cpu.len().max(1) as f64;
    let mse = cpu
        .iter()
        .zip(coreml)
        .map(|(a, b)| ((a - b) as f64).powi(2))
        .sum::<f64>()
        / cpu.len().max(1) as f64;
    let psnr = 10. * ((peak as f64).powi(2) / mse.max(1e-30)).log10();
    Ok(serde_json::json!({
        "file": raw,
        "values": cpu.len(),
        "coreml_s": runs[0].1,
        "cpu_s": runs[1].1,
        "max_abs_diff": max,
        "mean_abs_diff": mean,
        "psnr_db": psnr,
    })
    .to_string())
}

#[cfg(test)]
mod tests {
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
