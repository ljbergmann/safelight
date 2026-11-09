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

## Sky-ANE.mlpackage (made at build time)

Select Sky's network, UperNet with a ConvNeXt-base backbone (openmmlab/upernet-convnext-base,
MIT; [Sky-ANE.NOTICE](Sky-ANE.NOTICE) has the credit, the licence and what was changed). At
233 MB it is too large for the repository, so
[`scripts/convert-sky-ane.py`](../../scripts/convert-sky-ane.py) makes it into
`build/Sky-ANE.mlpackage`, and Xcode bundles it from there. A build without it still works:
Select Sky then runs on the CPU with the downloaded model, as before.

- **Source:** RAWmakase's `upernet-convnext-base-sky-fp16.onnx` (SHA-256 `ed75b2ad…b891`),
  downloaded from its pinned mirror when no path is given. The PyTorch model is rebuilt
  around its weights; it answers within 1e-7 of ONNX Runtime.
- **What was changed:** converted to a half-precision ML Program with one function per
  input shape, sharing one copy of the weights: the long edge at 1024 px and the short one
  for 3:2, 4:3 and 16:9 photos, both ways round, and square ones. Any other shape stays on
  the CPU. Eight-bit weights halve the size but lose too much (mask IoU 0.96–0.98, and
  broken on the GPU), so the weights stay in half precision.
- **Checked:** every function on the Mac's Neural Engine against ONNX Runtime's CPU on 12
  of macOS's desktop pictures. The sky's probabilities, which the app's soft mask is made of,
  may differ by 0.05 at most and 0.01 on average; measured 0.033 and 0.006. The masks cut at
  0.5 agree at IoU 0.99 or better on every photograph; on an illustration whose sky the model
  is unsure of (as many pixels near 0.5 as sky), a few of them crossing 0.5 bring it to 0.73.
- **Measured:** on an M1 iPad Pro, 390 ms per photo against about 7 s on the CPU, worst
  mask IoU 0.997 ([MEASUREMENTS.md](../../MEASUREMENTS.md)).
