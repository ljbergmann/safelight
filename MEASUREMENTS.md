# Measurements

## Stage 2: decode and render one RAW (2026-11-08)

Device: iPad Pro 12.9-inch (5th generation, M1, iPad13,8), Release build,
`-benchmark` launch option. Fitted render within 2048 px, the edit a new photo
starts with, no installed camera or lens profiles. Run one after another in the
order below, so each "before" is what the previous render left.

| Photo | Mode | Decode | Render | Total | Footprint after | Peak so far | Available after |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Canon EOS R5 CR3, 8191×5463 | GPU, full size | 1.2 s | 1.7 s | 3.1 s | 3.4 GB | 4.4 GB | 1.7 GB |
| | CPU, full size | 1.1 s | 1.0 s | 2.1 s | 3.7 GB | 4.4 GB | 1.4 GB |
| | GPU, half size | 0.5 s | 0.2 s | 0.7 s | 0.9 GB | 4.4 GB | 4.2 GB |
| Leica Q2 DNG, 8392×5632 | GPU, full size | 1.2 s | 1.9 s | 3.1 s | 3.1 GB | 4.8 GB | 2.0 GB |
| | CPU, full size | 1.1 s | 1.7 s | 2.8 s | 3.1 GB | 4.8 GB | 2.0 GB |
| | GPU, half size | 0.3 s | 0.3 s | 0.7 s | 1.6 GB | 4.8 GB | 3.5 GB |

Opening a photo takes 6–30 ms. The GPU's first render includes creating the
device (0.2 s).

- **Pixels.** The iPad's GPU render is within 1 of 255 of the Mac's CPU and GPU
  renders of the same photo (mean difference 0.0001), on both cameras. Compared
  with the Mac rendering with `HOME` pointing at an empty folder; with the
  Mac's installed profiles the renders differ, as they should.
- **The app's memory limit** on this iPad is about 5.1 GB (footprint plus
  `os_proc_available_memory`). A full-size decode peaked at 4.8 GB, within
  about 300 MB of the limit.
- **Memory comes back late, not never.** On the Mac, a full-size render
  leaves the footprint at about 5 GB for a few seconds after it returns, then
  it falls to about 650 MB with nothing else running. Renders in quick
  succession therefore stack one photo's leftovers on the next one's decode.
  Not yet known whether this is the allocator returning pages lazily or a
  thread that frees late.
- **GPU in the simulator:** falls back to the CPU, as the engine asks for 16
  inter-stage shader variables and the simulator's Metal allows 15. The M1
  allows it; the simulator's GPU results don't count.
- **Unsupported:** a Lightroom-made panorama DNG (JPEG XL–compressed RGB
  tiles) is refused with RAWmakase's own message, as on the desktop.

### What this means for the next stages

- Develop at full size only where it's needed (100% view, export); show Fit
  from a half-size decode or a smart preview. Half size uses a third of the
  memory and a quarter of the time.
- Before editing several photos in a row, find what holds memory for seconds
  after a render, and keep one photo's full-size decode alive at a time.
- Ask for Apple's increased-memory-limit entitlement, and size caches from
  `os_proc_available_memory`, not from physical RAM.
- A one-off render isn't faster on the GPU, as it includes building the
  pyramid and uploading it; the GPU's gain is in re-rendering while a slider
  moves (Stage 3).
