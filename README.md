# strandline

A dependency-light Rust engine for satellite-derived shoreline extraction and
change analysis. It ports the [CoastSat](https://github.com/kvos/CoastSat)
method — water index, pixel classification, class-balanced Otsu threshold and
sub-pixel marching-squares contour — into a single compiled binary, and extends
it with native harmonic tidal synthesis, vectorised transect intersection and
full DSAS-style change rates with quality control.

No Python, no GDAL bindings, no PROJ installation at run time.

> **Status**: research software accompanying a manuscript in preparation for
> *Computers & Geosciences*. The API is not yet stable.

## Why

Open Python toolkits democratised satellite-derived shorelines but do not scale.
Reprocessing decades of imagery over hundreds of kilometres of coast is slow,
and the dependency chain (GDAL bindings, scikit-learn, `pyfes`/`pyTMD`) is
fragile to reproduce. strandline performs the same computation in a compiled
binary, parallelised across cores.

## Installing

```bash
git clone https://github.com/franciscoparrao/strandline.git && cd strandline
cargo build --release          # binary at target/release/strandline
cargo install --path strandline-cli   # or install it on your PATH
```

Requires Rust 1.75 or newer. All dependencies resolve from crates.io; nothing
else has to be installed — no Python, no GDAL, no PROJ. Build with `--locked`
to reproduce the exact dependency versions recorded in `Cargo.lock`.

Parallelism is a default feature and its output is identical to the
single-threaded path — the golden tests pass in both configurations:

```bash
cargo build --release                                      # parallel (default)
cargo build --release --no-default-features --features cloud   # deterministic, single-threaded
cargo build --release --no-default-features                # single-threaded, no HTTP stack
```

To limit threads at run time without rebuilding, set `RAYON_NUM_THREADS=1`.

## Quick start

Extract a shoreline from a multispectral GeoTIFF:

```bash
strandline extract --ms scene_ms.tif --mask scene_mask.tif \
  --mask-kind qa --output-epsg 32719 --output shoreline.geojson
```

From scenes to per-transect change rates:

```bash
strandline fetch --collection landsat-c2-l2 --sensors all \
  --bbox=-71.72,-33.40,-71.62,-33.28 --datetime 1984-01-01/2025-12-31 -o scenes/
strandline baseline --dir scenes/ --mask-kind qa --output baseline.csv
strandline transects --baseline baseline.csv --spacing 100 --length 500 \
  --seaward-e=-1 --seaward-n=0 --output transects.csv
strandline intersect --transects transects.csv --shorelines shorelines.csv \
  --along-dist 25 -o cross.csv
strandline tide --constituents constituents.csv --dates dates.csv \
  --beach-slope 0.1 --output tide.csv
strandline rates --intersections cross.csv --dates dates.csv \
  --tide-correction tidecorr.csv --chainage-direction seaward --qc -o rates.csv
```

## Subcommands

| | |
|---|---|
| `fetch` | Acquire scenes from a STAC catalogue reading only the AOI window (COG range requests). Full Landsat archive 1984–present, plus Sentinel-2 |
| `extract` | Sub-pixel shoreline from a multispectral GeoTIFF (`--classify`, `--coastsat-compat`, `--subpixel-refine`) |
| `profile` | Sub-pixel water-line position by sigmoid profile fit per transect, with per-point uncertainty |
| `baseline` | Pick the best baseline automatically among candidate scenes |
| `transects` | Generate shore-normal transects along a smoothed baseline |
| `intersect` | Vectorised, parallel transect × shoreline intersection with quality control |
| `tide` | Native harmonic tidal synthesis and the horizontal correction of Vos et al. (2019) |
| `rates` | NSM, SCE, EPR, LRR and WLR with uncertainty, despiking and QC flags (`--qc`) |
| `slope` | Beach-face slope estimated from the shoreline series itself |
| `stl` | Seasonal-trend decomposition of per-transect series |
| `satellite-weights` | Per-sensor precision calibrated from the data, for weighted rates |

### Consistency checks

Errors that repeat on every date pass per-series quality control. These checks
compare each series with something outside it.

| | |
|---|---|
| `refcheck` | Do the transects sample the ocean shoreline? Anchor distance to an independent coastline, anchor elevation on a DEM, distance of the measured crossings |
| `extract --reference` | Restrict extraction and threshold to a band around a reference coastline (CoastSat's `max_dist_ref`) |
| `sensor-offset` | Shoreline offset between sensors over the years both imaged the same beaches (L7−L5, L8−L7, L9−L8 by default) |
| `angle` | Persistent angle between fixed transects and the local shore-normal, against a null built with the same estimator |

## Validation

Every stage is checked against the artefact that defines its correct answer —
CoastSat, `pyTMD`, or the stored per-transect tables of a national erosion
study — before any speed is claimed.

| Stage | Reference | Agreement |
|---|---|---|
| Pixel classification | CoastSat MLP | within 1.5 % of pixel counts |
| Shoreline extraction | CoastSat, matched scenes | 0.20 m median point-to-line |
| Tidal synthesis | `pyTMD` (GOT) | 1.15 mm maximum over 13 constituents |
| Transect generation | reference dataset | exact (0.0 m) |
| Intersection | vectorised Python reference | 49,602 crossings, 0 NaN mismatches, 0.12 mm max |
| Change rates | reference `rates` table | NSM/SCE/EPR ~5e-5 m, LRR/R²/CI ~1e-6 |
| Quality control | reference labelling | 0 mismatches in flag and reason |
| Coordinate transforms | `pyproj` | below 1 mm |

Validated end to end at six sites spanning 24° of latitude along the coast of
Chile. The test suite has 106 tests and passes in both parallel and
single-threaded configurations.

### Speed

Intersection benchmark, 250 transects × 200 shorelines, 21 interleaved
repetitions on an Intel Core i7-1270P:

| Configuration | Time | Speed-up | IQR |
|---|---|---|---|
| Vectorised Python reference | 7.34 s | — | — |
| Intersection operator, 16 threads | 0.027 s | 280× | 15 % |
| Intersection operator, 1 thread | 0.201 s | 37× | 15 % |
| End-to-end command line | 0.132 s | 56× | 9 % |

The first two rows compare the operator against the equivalent Python function,
both on data already in memory; the third charges the engine with process
start-up and CSV parsing that the reference does not pay. Harness and protocol:
`spike/transects/benchmark_intersect.py`.

## Reproducibility

- **Toolchain**: Rust 1.75 or newer. Pinned dependency versions in
  `Cargo.lock`; build with `--locked` to reproduce exactly.
- **Determinism**: parallel and single-threaded builds produce bit-for-bit
  identical output. No random subsampling anywhere in the pipeline: the
  class-balanced Otsu threshold weights the two populations instead of sampling
  them.
- **Tests**: `cargo test --workspace` (106 tests), including golden tests of the
  full GeoTIFF-to-shoreline path and of the ported MLP against the original
  scikit-learn outputs.
- **Benchmarks**: `python3 spike/transects/benchmark_intersect.py` reproduces the
  speed table above, and refuses to run on a loaded machine.
- **Data**: the benchmark artefacts live under `spike/`. Satellite scenes are
  not redistributed — they are public Landsat and Sentinel-2 products,
  retrievable with `strandline fetch` from the bounding boxes given in the
  manuscript.

## Documented divergences from CoastSat

A port whose value is fidelity should state where it is not identical. Four
deliberate divergences, all in the extraction stage and none affecting the
transect, rate or quality-control results: the thresholding domain (whole scene
with deterministic class balancing, rather than a reference-shoreline buffer
with random subsampling), pixels with undefined features (assigned to *other*
rather than filled with a placeholder), no automatic fallback when little sand
is present, and distance to nodata sampled from an exact Euclidean distance
transform at the nearest pixel. See Sect. 4.2 of the manuscript.

## Citing

A citable archive with a DOI is being prepared. Until then, cite the repository
and the manuscript in preparation.

## License

GPL-3.0-only (see [LICENSE](LICENSE)). strandline embeds the trained pixel classifier of
[CoastSat](https://github.com/kvos/CoastSat) (`strandline-core/models/coastsat_landsat_mlp.json`,
exported from `NN_4classes_Landsat_new.pkl`) and follows its algorithm closely, so it is distributed
under the same licence as CoastSat (GPL-3.0). Versions before v0.1.0 were labelled MIT OR Apache-2.0.
