"""Spike strandline — descarga escenas L8 de Viña del Mar como test data.

Usa el pipeline CoastSat de propuesta-4 (GEE ya autenticado). Descarga una
ventana corta de verano 2024 (cielos despejados) y RETIENE los GeoTIFF
multiespectrales, que son el input del prototipo Rust.

Correr con: propuesta-4-erosion-costera/.venv/bin/python3
"""
import os
import sys

P4 = os.path.expanduser('~/proyectos/zenodo-gaps/propuesta-4-erosion-costera')
sys.path.insert(0, os.path.join(P4, 'CoastSat'))

from coastsat import SDS_download, SDS_tools  # noqa: E402

polygon = [[[-71.563, -32.965],
            [-71.490, -32.965],
            [-71.490, -33.025],
            [-71.563, -33.025],
            [-71.563, -32.965]]]
polygon = SDS_tools.smallest_rectangle(polygon)

inputs = {
    'polygon': polygon,
    'dates': ['2024-01-01', '2024-03-01'],
    'sat_list': ['L8'],
    'sitename': 'VINA_SPIKE',
    'filepath': os.path.join(os.path.expanduser('~/proyectos/strandline'), 'data'),
    'landsat_collection': 'C02',
}

os.makedirs(inputs['filepath'], exist_ok=True)
metadata = SDS_download.retrieve_images(inputs)
n = sum(len(v['dates']) for v in metadata.values())
print(f"OK: {n} escenas descargadas en {inputs['filepath']}/VINA_SPIKE")
