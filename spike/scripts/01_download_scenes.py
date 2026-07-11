#!/usr/bin/env python3
"""
Spike strandline — Paso 1: descargar escenas L8 de Viña del Mar vía GEE.

Rango acotado (verano 2020, pocas nubes en Chile central) para obtener
~5-8 escenas L8; el spike solo necesita 3-5 utilizables.

Ejecutar con el venv de propuesta-4-erosion-costera (GEE ya autenticado).
"""

import os
import sys
import warnings
import pickle

warnings.filterwarnings("ignore")

COASTSAT_DIR = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"
)
sys.path.insert(0, COASTSAT_DIR)

from coastsat import SDS_download, SDS_tools

GEE_PROJECT = "1052232428380"
SDS_download.authenticate_and_initialize(GEE_PROJECT)

# Mismo polígono del piloto del paper (Viña del Mar / Reñaca)
polygon = [[[-71.563, -32.965],
            [-71.490, -32.965],
            [-71.490, -33.025],
            [-71.563, -33.025],
            [-71.563, -32.965]]]
polygon = SDS_tools.smallest_rectangle(polygon)

dates = ["2020-01-01", "2020-05-01"]
sat_list = ["L8"]
sitename = "VINA_SPIKE"
filepath_data = os.path.expanduser("~/proyectos/strandline/spike/data")

inputs = {
    "polygon": polygon,
    "dates": dates,
    "sat_list": sat_list,
    "sitename": sitename,
    "filepath": filepath_data,
    "landsat_collection": "C02",
}

print("Verificando imágenes disponibles...")
SDS_download.check_images_available(inputs)

print("\nDescargando imágenes...")
metadata = SDS_download.retrieve_images(inputs)

for sat in metadata:
    print(f"  {sat}: {len(metadata[sat]['dates'])} imágenes")
    for d, fn in zip(metadata[sat]["dates"], metadata[sat]["filenames"]):
        print(f"    {d} -> {fn}")
