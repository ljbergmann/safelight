# Models bundled with the app

## RawNIND-ANE.mlpackage

AI Denoise's network, RawNIND UtNet2 (Bayer) by Benoit Brummer, converted for the
Neural Engine. The licence is CC BY 4.0 ([RawNIND-ANE.LICENSE](RawNIND-ANE.LICENSE));
the credit and what was changed are in [RawNIND-ANE.NOTICE](RawNIND-ANE.NOTICE). This
repository is where the converted file is published, beside the app that ships it.

- **Source:** RAWmakase's `model_bayer.onnx` (SHA-256 `5111f19a…6cc1`), the author's
  checkpoint converted to ONNX with the weights unchanged. The app still downloads that
  model as before: the CPU runs it when a tile fails on the Neural Engine.
- **What was changed:** the Neural Engine computes in half precision only, and the
  network's values reach 1.3 million while half precision stops at 65,504. Each
  convolution's output is multiplied by its own power of two: its weights per input
  channel and its bias. Every operation is positively homogeneous, so in full precision
  the result is exactly the same. The largest value seen in calibration then lands at 2^12. The
  weights are then rounded to half precision. The input is multiplied by
  `input_scale`, and the output comes out multiplied by `output_scale`. Both are in the
  model's metadata, with the source's SHA-256.
- **Made by:** [`scripts/convert-denoise-ane.py`](../../scripts/convert-denoise-ane.py)
  (pinned versions; calibrated on RAWmakase's colour-chart DNGs and synthetic tiles). It
  checks on the Mac's Neural Engine that no tile overflows and that the answer stays
  within 65 dB of ONNX Runtime's CPU.
- **Measured:** on a 45 MP Canon CR3, all 77 tiles finite, worst 73.3 dB against the
  CPU (M4 Pro). On an M1 iPad Pro, 13 ms per tile against about 135 ms for CoreML's GPU
  in single precision ([MEASUREMENTS.md](../../MEASUREMENTS.md)).
