#!/usr/bin/env -S uv run --script
# /// script
# requires-python = "==3.12.*"
# dependencies = [
#     "coremltools==9.0",
#     "torch==2.7.0",
#     "transformers==4.57.1",
#     "timm==1.0.20",
#     "onnx==1.19.1",
#     "onnxruntime==1.23.2",
#     "numpy==2.2.6",
#     "pillow==11.3.0",
# ]
# ///
"""Writes build/Subject-CoreML.mlpackage: Select Subject's two large networks for Core ML.

Select Subject (and Background) runs three networks: SAM 2.1 small's image encoder, DETR's
panoptic segmentation (which finds the people and animals) and SAM 2's prompt encoder and
mask decoder, once for each of them. The first two take nearly all the time; they are
converted here. The decoder, about 10 ms a call, stays on the CPU with ONNX Runtime.

Both are rebuilt in PyTorch around the weights of RAWmakase's ONNX files (the ones the app
downloads), as transformers defines them: `Sam2Model.get_image_embeddings` (SAM 2.1 small:
blocks [1, 2, 11, 2], global attention in blocks 7, 10 and 13) and `DetrForSegmentation`
(ResNet-50 from timm, 250 classes). The export renamed some weights: linear layers became
MatMuls, BatchNorms were folded into Mul and Add constants or into a convolution's bias,
GroupNorms into InstanceNormalization with an affine, and SAM 2's position embedding into
one constant, which the backbone then uses as it is. Each is found by its node's name.

One package: `encoder` (1024 x 1024, half precision) and `detr_<width>x<height>` (single
precision: in half, a few pixels of an animal's outline cross DETR's thresholds) for 3:2,
4:3 and 16:9 photos both ways round and square ones, the long edge at 800 px as Select
Subject feeds DETR. Its short side is the photo's own, rounded, so each proportion gets the
rounded side and one either side of it: cameras' sensors are a hair off 3:2 (an 8191 x
5463 photo gives 534, a 6000 x 4000 one 533), and resizing the photo by that pixel moves
DETR's segments across its threshold. The DETR functions share their weights. Any other
shape runs on the CPU as before.

The app runs them on the GPU: on an M1 iPad, loading either network for the Neural Engine
takes the app down (and keeps its compiler busy for minutes after).

The script then checks both on this Mac's GPU against ONNX Runtime's CPU, on
macOS's desktop pictures: SAM 2's mask for a box prompt, through the CPU's decoder, within
0.05 at most and 0.01 on average of the CPU's probabilities; DETR finding the same
confident people and animals, the only segments Select Subject keeps. Its class
probabilities are reported too: its CPU reference computes in half precision as well, and a
segment of another class right at the threshold may cross it.

    scripts/convert-subject-coreml.py [folder with RAWmakase's sam2.1-hiera-small-v1 files] [--check]

Without a folder, the files are downloaded from RAWmakase's pinned mirror into build/.
`--check` checks the model already in build/ without converting it again.
"""

import gc
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
from transformers import (DetrConfig, DetrForSegmentation, Sam2Config, Sam2HieraDetConfig, Sam2Model,
                          Sam2VisionConfig)

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "build" / "Subject-CoreML.mlpackage"
MIRROR = "https://huggingface.co/pch/rawmakase-models/resolve/5748347a4c11d29b1c747859a4c24883a01215ac/"
FILES = {
    "vision_encoder.onnx": ("sam2.1-hiera-small/", "aacf1f7137bb6fffcf6bf166abcfabe28f57a76059254f3fb611c4a64a208119"),
    "vision_encoder.onnx_data": ("sam2.1-hiera-small/", "260fd1f0a34e72a3dc79a739e563b4facc0ba75504818b433a1f808e66637456"),
    "prompt_encoder_mask_decoder.onnx": ("sam2.1-hiera-small/", "079c59b261f723ff5c6a125e69b0170a957b21c58738c28d2b0394ecd0587d7f"),
    "prompt_encoder_mask_decoder.onnx_data": ("sam2.1-hiera-small/", "f9e59a584ab8ced21fa812c211bc01084204db1c9e92a5ef4fb3a49972b4e864"),
    "detr-panoptic-fp16.onnx": ("detr-resnet-50-panoptic/", "afd9f02d864302d690356fd4bfcb2feed2397a1190bf46a7306cb430464d734a"),
}
MEAN = np.array([0.485, 0.456, 0.406], np.float32)
STD = np.array([0.229, 0.224, 0.225], np.float32)
SAM_SIZE = 1024
DETR_LONG_EDGE = 800
RATIOS = [(3, 2), (4, 3), (16, 9), (1, 1)]
MOST_DIFFERENCE = 0.05
MOST_MEAN_DIFFERENCE = 0.01
CONFIDENT = 0.85  # RAWmakase's panoptic decoding keeps a segment from this class probability
SUBJECT_CLASSES = {1} | set(range(16, 26))  # people and animals (rawmakase_inference::PANOPTIC)


def files(argv) -> Path:
    folder = Path(argv[1]) if len(argv) > 1 else ROOT / "build" / "subject-onnx"
    folder.mkdir(parents=True, exist_ok=True)
    for name, (path, sha256) in FILES.items():
        target = folder / name
        if not target.exists():
            print(f"downloading {name}", flush=True)
            urllib.request.urlretrieve(MIRROR + path + name, target)
        digest = hashlib.sha256(target.read_bytes()).hexdigest()
        if digest != sha256:
            sys.exit(f"{target} is not RAWmakase's (SHA-256 {digest})")
    return folder


def weights(path: Path) -> tuple[onnx.ModelProto, dict]:
    graph = onnx.load(str(path))
    return graph, {i.name.removesuffix("_fp16"): numpy_helper.to_array(i).astype(np.float32)
                   for i in graph.graph.initializer}


def module_path(node) -> str:
    """A node's module, from its name: `/a/b.0/c/MatMul` -> `a.b.0.c`."""
    path = node.name.removeprefix("/model/").lstrip("/").rsplit("/", 1)[0]
    return re.sub(r"(\w+)/\1\.(\d+)", r"\1.\2", path).replace("/", ".")


class Encoder(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, pixel_values):
        embeddings = self.model.get_image_embeddings(pixel_values)
        return embeddings[0], embeddings[1], embeddings[2]


def sam_encoder(folder: Path) -> nn.Module:
    graph, w = weights(folder / "vision_encoder.onnx")
    backbone = Sam2HieraDetConfig(blocks_per_stage=[1, 2, 11, 2], global_attention_blocks=[7, 10, 13])
    model = Sam2Model(Sam2Config(vision_config=Sam2VisionConfig(backbone_config=backbone))).eval()
    state = model.state_dict()
    found = {k: w["model." + k] for k in state if "model." + k in w}
    for node in graph.graph.node:
        if node.op_type == "MatMul" and len(node.input) > 1 and node.input[1].startswith("onnx::"):
            found[module_path(node) + ".weight"] = w[node.input[1]].T
    wanted = [k for k in state if k.startswith("vision_encoder.") or k.startswith("mask_decoder.conv_s")
              or "no_memory_embedding" in k]
    missing = [k for k in wanted if k not in found and "pos_embed" not in k]
    if missing:
        sys.exit(f"SAM 2's encoder: weights not found in the ONNX file: {missing[:5]}")
    model.load_state_dict({k: torch.from_numpy(np.ascontiguousarray(v)) for k, v in found.items()}, strict=False)
    # The position embedding, interpolated and with the window's tiled in, as the export
    # folded it.
    backbone_module = model.vision_encoder.backbone
    backbone_module.register_buffer("folded_pos_embed", torch.from_numpy(w["/vision_encoder/backbone/Transpose_output_0"]))
    backbone_module._get_pos_embed = lambda hw: backbone_module.folded_pos_embed
    return Encoder(model).eval()


class Panoptic(nn.Module):
    def __init__(self, model):
        super().__init__()
        self.model = model

    def forward(self, pixel_values, pixel_mask):
        out = self.model(pixel_values=pixel_values, pixel_mask=pixel_mask, return_dict=True)
        return out.logits, out.pred_masks


def detr(folder: Path) -> nn.Module:
    graph, w = weights(folder / "detr-panoptic-fp16.onnx")
    config = DetrConfig(use_timm_backbone=True, backbone="resnet50", use_pretrained_backbone=False,
                        num_labels=250, masks=True)
    model = DetrForSegmentation(config).eval()
    state = model.state_dict()
    found = {k: w[k] for k in state if k in w}

    def key(node, leaf):
        path = module_path(node)
        for prefix in ("detr.model.", "detr.", ""):
            if f"{prefix}{path}.{leaf}" in state:
                return f"{prefix}{path}.{leaf}"
        return None

    def frozen_norm(base, scale, shift):
        """A frozen BatchNorm that multiplies by `scale` and adds `shift`: its eps is 1e-5."""
        found[base + ".weight"], found[base + ".bias"] = scale, shift
        found[base + ".running_mean"] = np.zeros_like(scale)
        found[base + ".running_var"] = np.full_like(scale, 1 - 1e-5)

    scales = {}
    for node in graph.graph.node:
        constants = [i for i in node.input if i in w and i not in state]
        if not constants:
            continue
        if node.op_type == "MatMul":
            if k := key(node, "weight"):
                found[k] = w[constants[0]].T
        elif node.op_type == "Conv" and node.name == "/bbox_attention/Conv":
            # k_linear, applied as a 1x1 convolution over the encoder's output.
            found["bbox_attention.k_linear.weight"] = w[node.input[1]].reshape(256, 256)
        elif node.op_type == "Conv":
            k = key(node, "weight")
            if k and k not in found:
                found[k] = w[node.input[1]]
            if k and len(node.input) > 2 and node.input[2] in w:
                bias = key(node, "bias")
                if bias and bias not in found:
                    found[bias] = w[node.input[2]]
                elif "downsample.0" in k:  # its BatchNorm, folded into the bias
                    shift = w[node.input[2]]
                    frozen_norm(k.replace("downsample.0.weight", "downsample.1"), np.ones_like(shift), shift)
        elif node.op_type in ("Mul", "Add") and re.search(r"/mask_head/gn\d/", node.name):
            if k := key(node, "weight"):  # GroupNorm's affine
                found[k.rsplit(".", 1)[0] + (".weight" if node.op_type == "Mul" else ".bias")] = \
                    w[constants[0]].reshape(-1)
        elif node.op_type in ("Mul", "Add") and re.search(r"/bn\d/", node.name):
            if k := key(node, "weight"):
                base = k.rsplit(".", 1)[0]
                if node.op_type == "Mul":
                    scales[base] = w[constants[0]].reshape(-1)
                else:
                    frozen_norm(base, scales[base], w[constants[0]].reshape(-1))
    query = "detr.model.query_position_embeddings.weight"
    expanded = [k for k, v in w.items() if v.size == state[query].numel() and k not in state]
    if len(expanded) == 1:  # expanded to the batch at export
        found[query] = w[expanded[0]].reshape(tuple(state[query].shape))
    missing = [k for k in state if k not in found and not k.endswith("num_batches_tracked")]
    if missing:
        sys.exit(f"DETR: weights not found in the ONNX file: {missing[:5]}")
    model.load_state_dict({k: torch.from_numpy(np.ascontiguousarray(v)) for k, v in found.items()}, strict=False)
    return Panoptic(model).eval()


def detr_shapes() -> list[tuple[int, int]]:
    """(width, height) of each DETR function: landscape, portrait, then square."""
    found = []
    for long, short in RATIOS:
        middle = round(DETR_LONG_EDGE * short / long)
        for side in (middle - 1, middle, middle + 1):
            if side > DETR_LONG_EDGE:
                continue
            for shape in ((DETR_LONG_EDGE, side), (side, DETR_LONG_EDGE)):
                if shape not in found:
                    found.append(shape)
    return found


def convert(folder: Path) -> None:
    descriptor = ct.utils.MultiFunctionDescriptor()
    with tempfile.TemporaryDirectory() as tmp:
        def add(traced, inputs, outputs, name, precision):
            converted = ct.convert(traced, inputs=inputs, outputs=outputs,
                                   compute_precision=precision,
                                   minimum_deployment_target=ct.target.iOS18, convert_to="mlprogram")
            part = Path(tmp) / f"{name}.mlpackage"
            converted.save(str(part))
            descriptor.add_function(str(part), src_function_name="main", target_function_name=name)
            print(f"converted {name}", flush=True)

        encoder = sam_encoder(folder)
        with torch.no_grad():
            traced = torch.jit.trace(encoder, torch.zeros(1, 3, SAM_SIZE, SAM_SIZE))
        add(traced, [ct.TensorType(name="pixel_values", shape=(1, 3, SAM_SIZE, SAM_SIZE))],
            [ct.TensorType(name=f"image_embeddings_{i}") for i in range(3)], "encoder", ct.precision.FLOAT16)
        panoptic = detr(folder)
        for width, height in detr_shapes():
            example = (torch.zeros(1, 3, height, width), torch.ones(1, 64, 64, dtype=torch.int64))
            with torch.no_grad():
                traced = torch.jit.trace(panoptic, example)
            add(traced, [ct.TensorType(name="pixel_values", shape=(1, 3, height, width)),
                         ct.TensorType(name="pixel_mask", shape=(1, 64, 64), dtype=np.int32)],
                [ct.TensorType(name="logits"), ct.TensorType(name="pred_masks")], f"detr_{width}x{height}",
                ct.precision.FLOAT32)
        descriptor.default_function_name = "encoder"
        joined = Path(tmp) / "joined.mlpackage"
        ct.utils.save_multifunction(descriptor, str(joined))
        merged = ct.models.MLModel(str(joined), skip_model_load=True)
        merged.author = "Meta AI (SAM 2.1, DETR); converted by Safelight"
        merged.license = "Apache-2.0 (SAM 2.1: facebookresearch/sam2; DETR: facebook/detr-resnet-50-panoptic)"
        merged.short_description = (
            "Select Subject's SAM 2.1 small image encoder (half precision) and DETR panoptic "
            "segmentation (single precision). Inputs and outputs as RAWmakase's ONNX files have them.")
        merged.user_defined_metadata.update({
            "encoder_sha256": FILES["vision_encoder.onnx_data"][1],
            "detr_sha256": FILES["detr-panoptic-fp16.onnx"][1],
            "detr_functions": ",".join(f"{w}x{h}" for w, h in detr_shapes()),
        })
        if OUT.exists():
            subprocess.run(["rm", "-rf", str(OUT)], check=True)
        merged.save(str(OUT))


def pictures() -> list[np.ndarray]:
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
    """`picture` resized to `width` x `height` (squeezed, as SAM 2 takes it) and normalized."""
    image = Image.fromarray((picture * 255).astype(np.uint8)).resize((width, height), Image.BILINEAR)
    return ((np.asarray(image, np.float32) / 255 - MEAN) / STD).transpose(2, 0, 1)[None].astype(np.float32)


def check(folder: Path) -> None:
    options = ort.SessionOptions()
    encoder = ort.InferenceSession(str(folder / "vision_encoder.onnx"), options, providers=["CPUExecutionProvider"])
    decoder = ort.InferenceSession(str(folder / "prompt_encoder_mask_decoder.onnx"), options,
                                   providers=["CPUExecutionProvider"])
    basic = ort.SessionOptions()
    basic.graph_optimization_level = ort.GraphOptimizationLevel.ORT_ENABLE_BASIC  # as RAWmakase loads it
    panoptic = ort.InferenceSession(str(folder / "detr-panoptic-fp16.onnx"), basic, providers=["CPUExecutionProvider"])
    samples = pictures()

    def mask(embeddings):
        feed = {"input_points": np.array([[[[512, 560]]]], np.float32), "input_labels": np.ones((1, 1, 1), np.int64),
                "input_boxes": np.array([[[256, 160, 768, 1000]]], np.float32)}
        feed.update({f"image_embeddings.{i}": e for i, e in enumerate(embeddings)})
        scores, masks, _ = decoder.run(None, feed)
        best = int(np.argmax(scores[0, 0]))
        return 1 / (1 + np.exp(-masks[0, 0, best]))

    sam = ct.models.MLModel(str(OUT), compute_units=ct.ComputeUnit.CPU_AND_GPU, function_name="encoder")
    farthest = mean = 0.0
    for picture in samples:
        x = tensor(picture, SAM_SIZE, SAM_SIZE)
        got = sam.predict({"pixel_values": x})
        mine = [got[f"image_embeddings_{i}"].astype(np.float32) for i in range(3)]
        if not all(np.isfinite(m).all() for m in mine):
            sys.exit("SAM 2's encoder answered with values that are not finite")
        difference = np.abs(mask(mine) - mask(encoder.run(None, {"pixel_values": x})))
        farthest, mean = max(farthest, float(difference.max())), max(mean, float(difference.mean()))
    del sam
    gc.collect()
    print(f"checked encoder: mask probabilities within {farthest:.3f}, mean {mean:.4f}", flush=True)
    if farthest > MOST_DIFFERENCE or mean > MOST_MEAN_DIFFERENCE:
        sys.exit("SAM 2's masks are too far from the CPU's")

    def softmax(logits):
        e = np.exp(logits - logits.max(-1, keepdims=True))
        return e / e.sum(-1, keepdims=True)

    worst = 0.0
    for width, height in detr_shapes():
        model = ct.models.MLModel(str(OUT), compute_units=ct.ComputeUnit.CPU_AND_GPU,
                                  function_name=f"detr_{width}x{height}")
        for picture in samples:
            x = tensor(picture, width, height)
            mask_in = np.ones((1, 64, 64), np.int64)
            want = softmax(panoptic.run(["logits"], {"pixel_values": x, "pixel_mask": mask_in})[0][0])
            got = model.predict({"pixel_values": x, "pixel_mask": mask_in.astype(np.int32)})
            if not (np.isfinite(got["logits"]).all() and np.isfinite(got["pred_masks"]).all()):
                sys.exit(f"DETR {width}x{height} answered with values that are not finite")
            have = softmax(got["logits"][0].astype(np.float64))
            worst = max(worst, float(np.abs(have - want).max()))
            def subjects(p):
                return {(q, int(p[q, :-1].argmax())) for q in range(len(p))
                        if p[q, :-1].max() > CONFIDENT and int(p[q, :-1].argmax()) in SUBJECT_CLASSES}
            if subjects(want) != subjects(have):
                sys.exit(f"DETR {width}x{height} finds other people or animals than the CPU: "
                         f"{subjects(want) ^ subjects(have)}")
        print(f"checked detr_{width}x{height}: same people and animals; class probabilities within "
              f"{worst:.3f} so far", flush=True)
        # Core ML crashes loading the eighth function into one process while the others
        # are still alive: let each go before the next.
        del model
        gc.collect()
    print(f"{OUT.relative_to(ROOT)} checked over {len(samples)} pictures")


def main() -> None:
    check_only = "--check" in sys.argv
    folder = files([a for a in sys.argv if a != "--check"])
    if not check_only:
        convert(folder)
    check(folder)


if __name__ == "__main__":
    main()
