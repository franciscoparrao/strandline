# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-07

First tagged release, archived on Zenodo for citation.

### Licence change

- strandline is now distributed under **GPL-3.0-only**. It embeds the trained pixel
  classifier of CoastSat (GPL-3.0) and follows its algorithm closely. Earlier,
  untagged snapshots of the repository were labelled MIT OR Apache-2.0.

### Added

- `extract`: sub-pixel shoreline from a multispectral GeoTIFF (water index, CoastSat
  pixel classifier, class-balanced Otsu threshold, marching-squares contour), with a
  `--coastsat-compat` mode for bit-level parity and an optional `--subpixel-refine`.
- `fetch`: scene acquisition from Microsoft Planetary Computer by COG range requests
  over the area of interest only; covers the full Landsat archive (TM, ETM+, OLI;
  `--sensors`) and Sentinel-2.
- `profile`: waterline position by sigmoid fit along each transect, with a per-point
  uncertainty.
- `tide`: native harmonic tidal synthesis and the horizontal correction of
  Vos et al. (2019).
- `baseline` and `transects`: baseline selection among candidate scenes and
  shore-normal transect generation.
- `intersect`: vectorised cross-shore intersection (equivalent to CoastSat's
  `compute_intersection_QC`).
- `rates`: DSAS-style change rates (NSM, SCE, EPR, LRR, WLR) with quality control and
  despiking.
- `slope`, `stl` and `satellite-weights`: beach slope from the satellite series, STL
  decomposition of each transect series, and per-sensor WLR weights calibrated from
  the data.
- Filter that discards shoreline vertices close to nodata (parity with CoastSat's
  `process_shoreline`).
- Test suite (106 tests) and the benchmark and validation artefacts under `spike/`.

### Changed

- SurtGIS dependencies now come from crates.io (1.4.0) instead of local paths.

### Removed

- Third-party field profiles and an unrelated internal pilot are no longer tracked
  in the repository.
