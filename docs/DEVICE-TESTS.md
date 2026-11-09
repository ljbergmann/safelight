# Testing on the iPad

What the simulator can't show, checked by hand on a real iPad before a
TestFlight build goes out. Note what failed, and on which iPad and iPadOS.

## Input
- [ ] Brush mask and Heal with the Apple Pencil: pressure changes the flow;
      a finger paints at the brush's own flow.
- [ ] Pinch and pan at Fit and 100%; a double tap toggles the zoom.
- [ ] Swipe between photos in the editor.

## Files and import
- [ ] Import from a real SD card (USB-C reader): days listed, those already in
      the library marked, progress and Cancel.
- [ ] Import from the Files app (a folder on iCloud Drive or a USB drive).
- [ ] Import from the Photos library: RAW originals, not the JPEGs.
- [ ] Import Profiles and Import Presets from the Files app.

## The AI features (they can't run on the simulator)
- [ ] Select Subject, Sky and Background: the model downloads once, then
      selects.
- [ ] Refine a Subject with taps: Add and Leave out.
- [ ] AI Denoise: downloads, runs, and the result shows; time it.

## Big and unusual files
- [ ] The 271 MP Lightroom panorama opens (no crash), its corners white.
- [ ] An HDR merge DNG opens.
- [ ] Export of the panorama at full size (watch memory: it may be too big).

## Views
- [ ] Before/After in each layout, and the split line dragged.
- [ ] Reference View beside a photo of the other orientation.
- [ ] The Info Overlay on a photo without a lens (a phone's DNG).

## Round trip
- [ ] A library edited on the iPad, copied to a Mac, opens in RAWmakase there
      with the same edits (`scripts/compat-check.sh` proves the rendering; this
      proves the copying).
