#!/usr/bin/env -S uv run --script
# /// script
# requires-python = "==3.12.*"
# dependencies = [
#     "coremltools==9.0",
#     "torch==2.7.0",
#     "transformers==4.57.1",
#     "onnx==1.19.1",
#     "onnxruntime==1.23.2",
#     "numpy==2.2.6",
#     "pillow==11.3.0",
# ]
# ///
"""Writes build/Sky-ANE.mlpackage, the Select Sky model the app runs on the Neural Engine.

Select Sky's network is UperNet with a ConvNeXt-base backbone (openmmlab/upernet-convnext-base,
MIT), exported by RAWmakase to ONNX (`upernet-convnext-base-sky-fp16.onnx`). The weights here
are read from that file, the one the app would otherwise download, so they are the same
values. The PyTorch model is rebuilt around them: ONNX keeps PyTorch's parameter names,
except for the linear layers (exported as MatMuls) and each convolution with its BatchNorm
folded in, which are found by their nodes' names. The rebuilt model answers within 1e-7 of
ONNX Runtime.

It is converted to a half-precision ML Program with one function per input shape, sharing
one copy of the weights (about 233 MB): the long edge at 1024 px, as Select Sky feeds it,
and the short one for 3:2, 4:3 and 16:9 photos, both ways round, and square ones. A photo of
any other proportion is selected on the CPU as before. The function names, the source's
SHA-256 and the long edge are in the model's metadata.

The script then checks every shape against ONNX Runtime's CPU on macOS's desktop pictures:
no probability may differ by more than 0.05, nor by more than 0.01 on average (about 13 and 2.5
of 255 levels). The masks' overlap at 0.5 is reported too, but not required: on a picture
whose sky the model is unsure of (an illustration with as many pixels near 0.5 as sky), a few
of those pixels crossing 0.5 swing it while nothing moves by more than 0.02.

    scripts/convert-sky-ane.py [upernet-convnext-base-sky-fp16.onnx] [--check]

`--check` checks the model already in build/ without converting it again.

Without a path, the file is downloaded from RAWmakase's pinned mirror into build/.
This takes a few minutes and needs about 8 GB of memory.
"""

import hashlib
import re
import subprocess
import sys
import tempfile
import urllib.request
from pathlib import Path

import coremltools as ct
import numpy as np
import onnx
import onnxruntime as ort
import torch
from onnx import numpy_helper
from PIL import Image
from torch import nn
from transformers import ConvNextConfig, UperNetConfig, UperNetForSemanticSegmentation

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "build" / "Sky-ANE.mlpackage"
SOURCE_URL = ("https://huggingface.co/pch/rawmakase-models/resolve/"
              "b727de8d854f7d5c50396323abe1a3b12e743954/upernet-convnext-base/"
              "upernet-convnext-base-sky-fp16.onnx")
SOURCE_SHA256 = "ed75b2ad81ad209949c9580071ed486a72a423037822a49faa4e98991a43b891"
SKY = 2  # ADE20K's class
LONG_EDGE = 1024
SHORT_EDGES = [683, 768, 576, 1024]  # 3:2, 4:3, 16:9, 1:1
MEAN = np.array([0.485, 0.456, 0.406], np.float32)
STD = np.array([0.229, 0.224, 0.225], np.float32)
# The app's soft mask is made of the probabilities, so they are what is compared: none may
# move by more than this (so a pixel the CPU is that sure about stays on its side of 0.5)...
MOST_DIFFERENCE = 0.05
# ...and on average not by more than this.
MOST_MEAN_DIFFERENCE = 0.01


class Sky(nn.Module):
    """RAWmakase's export: the probability of ADE20K's sky at each pixel."""

    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, pixel_values):
        return self.model(pixel_values=pixel_values, return_dict=True).logits.softmax(1)[:, SKY]


def source(argv) -> Path:
    path = Path(argv[1]) if len(argv) > 1 else ROOT / "build" / "upernet-convnext-base-sky-fp16.onnx"
    if not path.exists():
        path.parent.mkdir(parents=True, exist_ok=True)
        print(f"downloading {SOURCE_URL}")
        urllib.request.urlretrieve(SOURCE_URL, path)
    digest = hashlib.sha256(path.read_bytes()).hexdigest()
    if digest != SOURCE_SHA256:
        sys.exit(f"{path} is not RAWmakase's Sky model (SHA-256 {digest})")
    return path


def rebuild(path: Path) -> nn.Module:
    """The PyTorch model with the ONNX file's weights."""
    graph = onnx.load(str(path))
    weights = {i.name.removesuffix("_fp16"): numpy_helper.to_array(i).astype(np.float32)
               for i in graph.graph.initializer}
    config = UperNetConfig(
        backbone_config=ConvNextConfig(depths=[3, 3, 27, 3], hidden_sizes=[128, 256, 512, 1024],
                                       out_features=["stage1", "stage2", "stage3", "stage4"]),
        num_labels=150, hidden_size=512, use_auxiliary_head=False)
    model = UperNetForSemanticSegmentation(config).eval()
    modules = dict(model.named_modules())
    state = {}
    for name in model.state_dict():
        for candidate in ("model." + name, name):
            if candidate in weights:
                state[name] = weights[candidate]

    def module(node):
        path = node.name.removeprefix("/model/").rsplit("/", 1)[0].replace("/", ".")
        # The export names pyramid pooling's blocks after their containers twice.
        return re.sub(r"psp_modules\.psp_modules\.(\d+)\.psp_modules\.\d+\.(\d+)", r"psp_modules.\1.\2", path)

    for node in graph.graph.node:
        if len(node.input) < 2 or not node.input[1].startswith("onnx::"):
            continue
        if node.op_type == "MatMul":  # a linear layer, its weight transposed
            state[module(node) + ".weight"] = weights[node.input[1]].T
        elif node.op_type == "Conv":  # a convolution with its BatchNorm folded in
            conv = module(node)
            norm = conv.rsplit(".", 1)[0] + ".batch_norm"
            channels = weights[node.input[1]].shape[0]
            eps = modules[norm].eps
            state[conv + ".weight"] = weights[node.input[1]]
            state[norm + ".weight"] = np.ones(channels, np.float32)
            state[norm + ".bias"] = weights[node.input[2]]
            state[norm + ".running_mean"] = np.zeros(channels, np.float32)
            state[norm + ".running_var"] = np.full(channels, 1 - eps, np.float32)
    missing = [k for k in model.state_dict() if k not in state and not k.endswith("num_batches_tracked")]
    if missing:
        sys.exit(f"weights not found in the ONNX file: {missing[:5]}")
    model.load_state_dict({k: torch.from_numpy(np.ascontiguousarray(v)) for k, v in state.items()}, strict=False)
    return Sky(model).eval()


def shapes() -> list[tuple[int, int]]:
    """(width, height) of each function: landscape, portrait, then square."""
    found = []
    for short in SHORT_EDGES:
        for shape in ((LONG_EDGE, short), (short, LONG_EDGE)):
            if shape not in found:
                found.append(shape)
    return found


def pictures() -> list[np.ndarray]:
    """macOS's desktop pictures as RGB in 0..1, for the check."""
    folder = Path("/System/Library/Desktop Pictures/.thumbnails")
    found = []
    with tempfile.TemporaryDirectory() as tmp:
        for picture in sorted(folder.glob("*.heic"))[:12]:
            png = Path(tmp) / (picture.stem + ".png")
            if subprocess.run(["sips", "-s", "format", "png", str(picture), "--out", str(png)],
                              capture_output=True).returncode == 0:
                found.append(np.asarray(Image.open(png).convert("RGB"), np.float32) / 255)
    if not found:
        sys.exit("no desktop pictures to check the model with")
    return found


def tensor(picture: np.ndarray, width: int, height: int) -> np.ndarray:
    """`picture` cropped to the shape's proportions, resized and normalized as Select Sky does."""
    h, w = picture.shape[:2]
    if w / h > width / height:
        crop = round(h * width / height)
        picture = picture[:, (w - crop) // 2:(w - crop) // 2 + crop]
    else:
        crop = round(w * height / width)
        picture = picture[(h - crop) // 2:(h - crop) // 2 + crop]
    image = Image.fromarray((picture * 255).astype(np.uint8)).resize((width, height), Image.BILINEAR)
    pixels = np.asarray(image, np.float32) / 255
    return ((pixels - MEAN) / STD).transpose(2, 0, 1)[None].astype(np.float32)


def main() -> None:
    check_only = "--check" in sys.argv
    path = source([a for a in sys.argv if a != "--check"])
    if not check_only:
        convert(path)
    check(path)


def convert(path: Path) -> None:
    model = rebuild(path)
    descriptor = ct.utils.MultiFunctionDescriptor()
    with tempfile.TemporaryDirectory() as tmp:
        for width, height in shapes():
            example = torch.zeros(1, 3, height, width)
            with torch.no_grad():
                traced = torch.jit.trace(model, example)
            converted = ct.convert(
                traced,
                inputs=[ct.TensorType(name="pixel_values", shape=(1, 3, height, width))],
                outputs=[ct.TensorType(name="sky")],
                compute_precision=ct.precision.FLOAT16,
                minimum_deployment_target=ct.target.iOS18,
                convert_to="mlprogram",
            )
            part = Path(tmp) / f"{width}x{height}.mlpackage"
            converted.save(str(part))
            descriptor.add_function(str(part), src_function_name="main",
                                    target_function_name=f"sky_{width}x{height}")
            print(f"converted {width}x{height}", flush=True)
        descriptor.default_function_name = f"sky_{LONG_EDGE}x{SHORT_EDGES[0]}"
        joined = Path(tmp) / "joined.mlpackage"
        ct.utils.save_multifunction(descriptor, str(joined))
        merged = ct.models.MLModel(str(joined), skip_model_load=True)
        merged.author = "OpenMMLab (UperNet, ConvNeXt); converted by Safelight"
        merged.license = "MIT (openmmlab/upernet-convnext-base's card)"
        merged.short_description = (
            "Select Sky's UperNet ConvNeXt-base, half precision. Input: pixel_values, ImageNet-"
            "normalized RGB; output: sky, the probability of sky at each pixel.")
        merged.user_defined_metadata.update({
            "source_sha256": SOURCE_SHA256,
            "long_edge": str(LONG_EDGE),
            "functions": ",".join(f"{w}x{h}" for w, h in shapes()),
        })
        if OUT.exists():
            subprocess.run(["rm", "-rf", str(OUT)], check=True)
        merged.save(str(OUT))


def check(path: Path) -> None:
    """Each shape on this Mac's Neural Engine against ONNX Runtime's CPU."""
    reference = ort.InferenceSession(str(path), providers=["CPUExecutionProvider"])
    samples = pictures()
    worst, farthest, mean = 1.0, 0.0, 0.0
    for width, height in shapes():
        engine = ct.models.MLModel(str(OUT), compute_units=ct.ComputeUnit.CPU_AND_NE,
                                   function_name=f"sky_{width}x{height}")
        for picture in samples:
            x = tensor(picture, width, height)
            probability = reference.run(None, {"pixel_values": x})[0]
            got = engine.predict({"pixel_values": x})["sky"]
            if not np.isfinite(got).all():
                sys.exit(f"{width}x{height}: the Neural Engine answered with values that are not finite")
            difference = np.abs(got - probability)
            farthest = max(farthest, float(difference.max()))
            mean = max(mean, float(difference.mean()))
            want, have = probability > 0.5, got > 0.5
            if (want | have).any():
                worst = min(worst, float((want & have).sum() / (want | have).sum()))
        print(f"checked {width}x{height}: largest difference {farthest:.3f}, mean {mean:.4f}, "
              f"worst IoU {worst:.4f} so far", flush=True)
    if farthest > MOST_DIFFERENCE or mean > MOST_MEAN_DIFFERENCE:
        sys.exit(f"the Neural Engine's answers are too far from the CPU's: largest difference "
                 f"{farthest:.3f}, mean {mean:.4f}")
    print(f"{OUT.relative_to(ROOT)} checked: largest difference {farthest:.3f}, mean {mean:.4f}, "
          f"worst IoU {worst:.4f} over {len(samples)} pictures")


if __name__ == "__main__":
    main()
