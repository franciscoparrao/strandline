#!/usr/bin/env python3
"""
Labeler interactivo para reentrenar el clasificador de píxeles sobre
Surface Reflectance (hallazgo E4: el MLP de CoastSat entrenado en TOA de
GEE no transfiere a SR — clasifica ~68% como "sand" en landsat-c2-l2).

Adapta `coastsat.SDS_classify.label_images` (que asume el layout de
descarga GEE de CoastSat: metadata dict + SDS_preprocess.preprocess_single)
a los pares `*_ms.tif` / `*_mask.tif` que produce `strandline fetch`
(5 bandas B,G,R,NIR,SWIR1 reflectancia f32 + QA_PIXEL u16).

Reusa sin modificar de CoastSat:
  - `SDS_classify.SelectFromImage` (lasso para water/other)
  - `SDS_shoreline.calculate_features` (las 20 features — paridad exacta
    con el clasificador Rust ya validada, ver strandline-core::features)
  - `SDS_preprocess.rescale_image_intensity` + `SDS_tools.nd_index`
El formato de salida (`{out_dir}/{sitename}/{filename}.pkl` con
labels/features/label_ids) es IDÉNTICO al de CoastSat, así que
`SDS_classify.load_labels` se puede reusar tal cual en el training script.

Controles (mismos que CoastSat original):
  - flecha derecha: etiquetar esta imagen / flecha izquierda: saltar
  - click = flood-fill para SAND (tolerancia configurable), click en
    la esquina superior derecha = borrar la última selección
  - click = pixel individual para WHITE-WATER (sin flood-fill)
  - lasso (click y arrastrar) para WATER, luego para OTHER LAND
  - <Enter> para pasar a la siguiente clase, <Esc> para reiniciar la clase
  - <Esc> durante keep/skip = abortar toda la sesión
"""

import argparse
import os
import pickle
import sys
from pathlib import Path

import numpy as np
import rasterio
import matplotlib.pyplot as plt
from matplotlib.widgets import LassoSelector
from skimage.segmentation import flood
from pylab import ginput

COASTSAT_ROOT = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"
)
sys.path.insert(0, COASTSAT_ROOT)
from coastsat import SDS_tools, SDS_preprocess, SDS_shoreline  # noqa: E402
from coastsat.SDS_classify import SelectFromImage  # noqa: E402

LABELS = {"sand": 1, "white-water": 2, "water": 3, "other land features": 0}
COLORS = {
    "sand": [1, 1, 0],
    "white-water": [1, 0, 1],
    "water": [0.1, 0.1, 0.7],
    "other land features": [0.8, 0.3, 0.2],
}

# bits Landsat Collection 2 QA_PIXEL: dilated cloud(1), cirrus(2), cloud(3),
# cloud shadow(4) — mismos bits que preprocess_single usa para armar el
# cloud_mask de CoastSat sobre QA_PIXEL.
QA_BITS = (1 << 1) | (1 << 2) | (1 << 3) | (1 << 4)


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


def label_one(fig, ax, im_ms, cloud_mask, filename, tolerance):
    im_RGB = SDS_preprocess.rescale_image_intensity(im_ms[:, :, [2, 1, 0]], cloud_mask, 99.9)
    im_NDVI = SDS_tools.nd_index(im_ms[:, :, 3], im_ms[:, :, 2], cloud_mask)
    im_NDWI = SDS_tools.nd_index(im_ms[:, :, 3], im_ms[:, :, 1], cloud_mask)
    im_viz = im_RGB.copy()
    im_labels = np.zeros(im_RGB.shape[:2])

    ax.clear()
    ax.axis("off")
    ax.imshow(im_RGB)
    implot = ax.imshow(im_viz, alpha=0.6)
    ax.set_title(filename)

    key_event = {}

    def press(event):
        key_event["pressed"] = event.key

    # keep/skip
    while True:
        btn_keep = ax.text(1.1, 0.9, "keep ⇨", size=12, ha="right", va="top",
                            transform=ax.transAxes, bbox=dict(boxstyle="square", ec="k", fc="w"))
        btn_skip = ax.text(-0.1, 0.9, "⇦ skip", size=12, ha="left", va="top",
                            transform=ax.transAxes, bbox=dict(boxstyle="square", ec="k", fc="w"))
        btn_esc = ax.text(0.5, 0, "<esc> to quit", size=12, ha="center", va="top",
                           transform=ax.transAxes, bbox=dict(boxstyle="square", ec="k", fc="w"))
        fig.canvas.draw_idle()
        fig.canvas.mpl_connect("key_press_event", press)
        plt.waitforbuttonpress()
        btn_skip.remove()
        btn_keep.remove()
        btn_esc.remove()
        if key_event.get("pressed") == "right":
            break
        elif key_event.get("pressed") == "left":
            return None
        elif key_event.get("pressed") == "escape":
            plt.close(fig)
            raise StopIteration("usuario canceló el labeling")
        else:
            plt.waitforbuttonpress()

    # SAND (flood fill)
    ax.set_title(f"Click en píxeles de SAND (flood fill, tolerancia={tolerance:.2f})\n"
                 "<Enter> cuando termines")
    btn_erase = ax.text(im_RGB.shape[1], 0, "Erase", size=20, ha="right", va="top",
                         bbox=dict(boxstyle="square", ec="k", fc="w"))
    fig.canvas.draw_idle()
    color_sand = COLORS["sand"]
    sand_pixels = []
    while True:
        seed = ginput(n=1, timeout=0, show_clicks=True)
        if len(seed) == 0:
            break
        seed = np.round(seed[0]).astype(int)
        if seed[0] > 0.95 * im_RGB.shape[1] and seed[1] < 0.05 * im_RGB.shape[0]:
            if sand_pixels:
                im_labels[sand_pixels[-1]] = 0
                for k in range(im_viz.shape[2]):
                    im_viz[sand_pixels[-1], k] = im_RGB[sand_pixels[-1], k]
                implot.set_data(im_viz)
                fig.canvas.draw_idle()
                del sand_pixels[-1]
        else:
            fill_ndvi = flood(im_NDVI, (seed[1], seed[0]), tolerance=tolerance)
            fill_ndwi = flood(im_NDWI, (seed[1], seed[0]), tolerance=tolerance)
            fill_sand = np.logical_and(fill_ndvi, fill_ndwi)
            im_labels[fill_sand] = LABELS["sand"]
            sand_pixels.append(fill_sand)
            for k in range(im_viz.shape[2]):
                im_viz[im_labels == LABELS["sand"], k] = color_sand[k]
            implot.set_data(im_viz)
            fig.canvas.draw_idle()

    # WHITE-WATER (pixel a pixel)
    ax.set_title("Click en píxeles individuales de WHITE-WATER (sin flood fill)\n"
                 "<Enter> cuando termines")
    color_ww = COLORS["white-water"]
    ww_pixels = []
    while True:
        seed = ginput(n=1, timeout=0, show_clicks=True)
        if len(seed) == 0:
            break
        seed = np.round(seed[0]).astype(int)
        if seed[0] > 0.95 * im_RGB.shape[1] and seed[1] < 0.05 * im_RGB.shape[0]:
            if ww_pixels:
                im_labels[ww_pixels[-1][1], ww_pixels[-1][0]] = 0
                for k in range(im_viz.shape[2]):
                    im_viz[ww_pixels[-1][1], ww_pixels[-1][0], k] = im_RGB[ww_pixels[-1][1], ww_pixels[-1][0], k]
                implot.set_data(im_viz)
                fig.canvas.draw_idle()
                del ww_pixels[-1]
        else:
            im_labels[seed[1], seed[0]] = LABELS["white-water"]
            for k in range(im_viz.shape[2]):
                im_viz[seed[1], seed[0], k] = color_ww[k]
            implot.set_data(im_viz)
            fig.canvas.draw_idle()
            ww_pixels.append(seed)

    im_sand_ww = im_viz.copy()
    btn_erase.set(text="<Esc> to Erase", fontsize=12)

    # WATER (lasso)
    ax.set_title("Click y arrastra para dibujar lassos y seleccionar WATER\n"
                 "<Enter> cuando termines")
    fig.canvas.draw_idle()
    selector = SelectFromImage(ax, implot, COLORS["water"])
    key_event = {}
    while True:
        fig.canvas.draw_idle()
        fig.canvas.mpl_connect("key_press_event", press)
        plt.waitforbuttonpress()
        if key_event.get("pressed") == "enter":
            selector.disconnect()
            break
        elif key_event.get("pressed") == "escape":
            selector.array = im_sand_ww
            implot.set_data(selector.array)
            fig.canvas.draw_idle()
            selector.implot = implot
            selector.im_bool = np.zeros((selector.array.shape[0], selector.array.shape[1]))
            selector.ind = []
    im_viz = selector.array
    selector.im_bool = selector.im_bool.astype(bool)
    im_labels[selector.im_bool] = LABELS["water"]
    im_sand_ww_water = im_viz.copy()

    # OTHER LAND (lasso)
    ax.set_title("Click y arrastra para dibujar lassos y seleccionar OTHER LAND\n"
                 "<Enter> cuando termines")
    fig.canvas.draw_idle()
    selector = SelectFromImage(ax, implot, COLORS["other land features"])
    key_event = {}
    while True:
        fig.canvas.draw_idle()
        fig.canvas.mpl_connect("key_press_event", press)
        plt.waitforbuttonpress()
        if key_event.get("pressed") == "enter":
            selector.disconnect()
            break
        elif key_event.get("pressed") == "escape":
            selector.array = im_sand_ww_water
            implot.set_data(selector.array)
            fig.canvas.draw_idle()
            selector.implot = implot
            selector.im_bool = np.zeros((selector.array.shape[0], selector.array.shape[1]))
            selector.ind = []
    selector.im_bool = selector.im_bool.astype(bool)
    im_labels[selector.im_bool] = LABELS["other land features"]

    return im_labels


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dir", required=True, help="directorio con *_ms.tif/*_mask.tif (salida de `strandline fetch`)")
    ap.add_argument("--site", required=True, help="nombre del sitio (subcarpeta de salida)")
    ap.add_argument("--out-dir", required=True, help="directorio raíz de training data (filepath_train)")
    ap.add_argument("--max-images", type=int, default=5)
    ap.add_argument("--cloud-thresh", type=float, default=0.3)
    ap.add_argument("--tolerance", type=float, default=0.01)
    args = ap.parse_args()

    in_dir = Path(args.dir)
    ms_files = sorted(in_dir.glob("*_ms.tif"))
    out_site = Path(args.out_dir) / args.site
    out_site.mkdir(parents=True, exist_ok=True)

    fig, ax = plt.subplots(1, 1, figsize=[17, 10], tight_layout=True)
    try:
        mng = plt.get_current_fig_manager()
        mng.window.showMaximized()
    except Exception:
        pass

    n_labeled = 0
    for ms_path in ms_files:
        if n_labeled >= args.max_images:
            break
        mask_path = ms_path.with_name(ms_path.name.replace("_ms.tif", "_mask.tif"))
        if not mask_path.exists():
            continue
        im_ms, cloud_mask = load_pair(ms_path, mask_path)
        cloud_cover = cloud_mask.mean()
        if cloud_cover > args.cloud_thresh:
            print(f"salteada (nubes {cloud_cover:.1%}): {ms_path.name}")
            continue

        filename = ms_path.stem.replace("_ms", "")
        try:
            im_labels = label_one(fig, ax, im_ms, cloud_mask, filename, args.tolerance)
        except StopIteration:
            print("sesión cancelada por el usuario")
            break
        if im_labels is None:
            print(f"saltada: {filename}")
            continue

        fig.savefig(out_site / f"{filename}.jpg", dpi=150)
        features = {}
        for key in LABELS:
            im_bool = im_labels == LABELS[key]
            features[key] = SDS_shoreline.calculate_features(im_ms, cloud_mask, im_bool)
        training_data = {"labels": im_labels, "features": features, "label_ids": LABELS}
        with open(out_site / f"{filename}.pkl", "wb") as f:
            pickle.dump(training_data, f)
        n_labeled += 1
        print(f"guardado: {out_site / (filename + '.pkl')}")

    plt.close(fig)
    print(f"\n{n_labeled} imágenes etiquetadas en {out_site}")


if __name__ == "__main__":
    main()
