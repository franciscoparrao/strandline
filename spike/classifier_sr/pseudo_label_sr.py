#!/usr/bin/env python3
"""
Pseudo-labeling automático para reentrenar el clasificador SR (hallazgo E4)
sin etiquetado manual — decisión explícita del usuario de aceptar reglas de
alta confianza en vez de clics interactivos (ver discusión en la sesión).

Riesgo aceptado y documentado: el MLP resultante tiende a aprender una
versión suavizada/combinada de las MISMAS reglas de umbral usadas acá para
generar las labels — no aporta poder de discriminación más allá de esas
reglas, a diferencia de un MLP entrenado con ejemplos reales dibujados a
mano. La clase "whitewater" es la más débil de las cuatro porque no hay una
regla espectral de alta confianza clara para espuma/rompiente (se aproxima
por brillo alto dentro de la franja de agua costera, y por eso se limita a
una muestra pequeña — de hecho la clase ya es minoritaria en el dataset
original de CoastSat: 752 de 55673 píxeles, ~1.4%).

Reglas de alta confianza (todas excluyen nubes/nodata):
  - water:  MNDWI (Green,SWIR1) > mndwi_water  (agua clara, señal fuerte)
  - other:  MNDWI < mndwi_land  AND  NDVI (NIR,R) > ndvi_veg  (vegetación)
  - sand:   MNDWI < mndwi_land  AND  NDVI < ndvi_bare  AND brillo visible
            (B,G,R) alto (percentil) AND dentro de un buffer cerca del
            agua (evita confundir con suelo desnudo tierra adentro)
  - white-water: dentro de la franja de agua costera (buffer de la
            frontera agua/tierra, lado agua) AND brillo visible muy por
            encima de la media del agua de la escena — se toma solo el
            top-K más brillante por escena para no inflar la clase.

Guarda el mismo formato de pkl que `label_sr_images.py` (compatible con
`SDS_classify.load_labels` sin cambios), así que `train_sr_classifier.py`
se reusa tal cual, ya sea con labels manuales o automáticas.
"""
import argparse
import os
import pickle
import sys
from pathlib import Path

import numpy as np
import rasterio
from skimage.morphology import binary_dilation, disk

COASTSAT_ROOT = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"
)
sys.path.insert(0, COASTSAT_ROOT)
from coastsat import SDS_tools, SDS_shoreline  # noqa: E402

LABELS = {"sand": 1, "white-water": 2, "water": 3, "other land features": 0}
QA_BITS = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4)  # dilated/cirrus/cloud/shadow


def load_pair(ms_path, mask_path):
    with rasterio.open(ms_path) as src:
        im_ms = np.stack([src.read(b) for b in range(1, 6)], axis=-1).astype(np.float32)
    with rasterio.open(mask_path) as src:
        qa = src.read(1).astype(np.uint16)
    cloud_mask = (qa & QA_BITS) != 0
    nodata = np.any(np.isnan(im_ms), axis=-1)
    cloud_mask = cloud_mask | nodata
    im_ms = np.nan_to_num(im_ms, nan=0.0)
    return im_ms, cloud_mask


def pseudo_label_scene(im_ms, cloud_mask, args, rng):
    valid = ~cloud_mask
    B, G, R, NIR, SWIR1 = (im_ms[:, :, k] for k in range(5))

    mndwi = SDS_tools.nd_index(G, SWIR1, cloud_mask)
    ndvi = SDS_tools.nd_index(NIR, R, cloud_mask)
    bright_vis = (B + G + R) / 3.0

    water_mask = valid & (mndwi > args.mndwi_water)
    land_mask = valid & (mndwi < args.mndwi_land)
    veg_mask = land_mask & (ndvi > args.ndvi_veg)

    # sand: banda de playa real. Usar mndwi_land (-0.30) para "tierra"
    # excluye justo la franja húmeda cerca del agua donde vive la arena
    # real (su reflectancia está influida por humedad/agua adyacente). Se
    # usa un umbral suelto (< 0, "no-agua") solo para candidatos de sand,
    # y el contraste local (percentil dentro del anillo costero, no de
    # toda la escena) para separar arena de roca/vegetación oscura vecina.
    not_water = valid & (mndwi < 0.0)
    water_buffer = binary_dilation(water_mask, disk(args.buffer_px))
    coastal_land = not_water & water_buffer & (ndvi < args.ndvi_bare)
    if coastal_land.sum() > 0:
        bright_thresh = np.nanpercentile(bright_vis[coastal_land], args.sand_bright_pct)
    else:
        bright_thresh = np.inf
    sand_mask = coastal_land & (bright_vis > bright_thresh)

    # other también necesita contraejemplos de tierra NO vegetada lejos
    # del agua (techos, suelo seco, roca) — sin esto el MLP ve "brillante
    # + no-vegetado" solo en ejemplos de sand y generaliza esa regla a
    # cualquier píxel brillante de la escena (rooftops, cerros secos),
    # porque el modelo en inferencia solo ve espectro, no la proximidad al
    # agua que se usó para generar la label. Con esta primera iteración
    # (sin este contraejemplo) sand salió sobre-predicho ~22% en Viña
    # (vs ~63% del modelo TOA aplicado a SR — mejor, pero aún alto).
    far_land = not_water & (ndvi < args.ndvi_bare) & ~water_buffer
    other_mask = veg_mask | far_land

    # white-water: solo la franja de agua inmediatamente pegada a la costa
    # (buffer angosto), muy por encima del brillo del AGUA COSTERA local
    # (no del agua de toda la escena, que puede incluir zonas turbias lejos
    # de la costa y sesgar el umbral hacia abajo).
    land_buffer = binary_dilation(land_mask, disk(args.ww_buffer_px))
    coastal_water = water_mask & land_buffer
    ww_mask = np.zeros_like(water_mask)
    if coastal_water.sum() > 0:
        coastal_water_bright = bright_vis[coastal_water]
        ww_thresh = np.nanmean(coastal_water_bright) + args.ww_std_mult * np.nanstd(coastal_water_bright)
        candidates = np.argwhere(coastal_water & (bright_vis > ww_thresh))
        if len(candidates) > args.ww_max_px:
            idx = rng.choice(len(candidates), size=args.ww_max_px, replace=False)
            candidates = candidates[idx]
        ww_mask[candidates[:, 0], candidates[:, 1]] = True

    masks = {
        "sand": sand_mask,
        "white-water": ww_mask,
        "water": water_mask & ~ww_mask,
        "other land features": other_mask,
    }
    # subsample para no desbalancear con una sola escena grande
    for key, m in masks.items():
        idx = np.argwhere(m)
        if len(idx) > args.max_px_per_class:
            sel = rng.choice(len(idx), size=args.max_px_per_class, replace=False)
            new_mask = np.zeros_like(m)
            new_mask[idx[sel, 0], idx[sel, 1]] = True
            masks[key] = new_mask
    return masks


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--sites-dir", required=True, nargs="+",
                     help="uno o más directorios con *_ms.tif/*_mask.tif (uno por sitio)")
    ap.add_argument("--out-dir", required=True)
    ap.add_argument("--cloud-thresh", type=float, default=0.3)
    ap.add_argument("--mndwi-water", type=float, default=0.30)
    ap.add_argument("--mndwi-land", type=float, default=-0.30)
    ap.add_argument("--ndvi-veg", type=float, default=0.30)
    ap.add_argument("--ndvi-bare", type=float, default=0.15)
    ap.add_argument("--sand-bright-pct", type=float, default=50.0)
    ap.add_argument("--ww-std-mult", type=float, default=3.0)
    ap.add_argument("--ww-max-px", type=int, default=150)
    ap.add_argument("--buffer-px", type=int, default=3)
    ap.add_argument("--ww-buffer-px", type=int, default=2)
    ap.add_argument("--max-px-per-class", type=int, default=3000)
    ap.add_argument("--seed", type=int, default=0)
    args = ap.parse_args()

    rng = np.random.default_rng(args.seed)
    totals = {k: 0 for k in LABELS}

    for site_dir in args.sites_dir:
        site_dir = Path(site_dir)
        site = site_dir.name
        out_site = Path(args.out_dir) / site
        out_site.mkdir(parents=True, exist_ok=True)

        for ms_path in sorted(site_dir.glob("*_ms.tif")):
            mask_path = ms_path.with_name(ms_path.name.replace("_ms.tif", "_mask.tif"))
            if not mask_path.exists():
                continue
            im_ms, cloud_mask = load_pair(ms_path, mask_path)
            if cloud_mask.mean() > args.cloud_thresh:
                continue

            masks = pseudo_label_scene(im_ms, cloud_mask, args, rng)
            # ojo: "other land features" vale 0, igual que el default de
            # im_labels — reconstruir el bool desde im_labels==0 confundiría
            # "etiquetado como other" con "nunca etiquetado". Se usan las
            # máscaras directamente, la matriz `labels` es solo para debug.
            im_labels = np.full(im_ms.shape[:2], -1)
            for key, m in masks.items():
                im_labels[m] = LABELS[key]

            features = {}
            for key in LABELS:
                im_bool = masks[key]
                features[key] = SDS_shoreline.calculate_features(im_ms, cloud_mask, im_bool)
                totals[key] += im_bool.sum()

            filename = ms_path.stem.replace("_ms", "")
            training_data = {"labels": im_labels, "features": features, "label_ids": LABELS}
            with open(out_site / f"{filename}.pkl", "wb") as f:
                pickle.dump(training_data, f)
            print(f"{site}/{filename}: " +
                  ", ".join(f"{k}={int(masks[k].sum())}" for k in LABELS))

    print("\nTotales:")
    for k, v in totals.items():
        print(f"  {k}: {v}")


if __name__ == "__main__":
    main()
