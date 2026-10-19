"""Fuente de datos de la medición del ángulo, seleccionable con SDS_DATASET.

- ``original`` (default): transectos y shorelines publicados del estudio de
  referencia. Es la base de la versión del paper de agosto de 2026. El
  reanálisis de propuesta-4 mostró que está contaminada por agua interior: solo
  el 24 % de los 538 transectos medidos tiene el origen a menos de 200 m de la
  costa oceánica (ver ``angle_vs_coast.txt``).
- ``ocean``: reanálisis con la referencia anclada a la costa oceánica
  (propuesta-4, scripts 16/19/28): transectos generados sobre la referencia
  oceánica y shorelines re-extraídas con ``reference_shoreline`` +
  ``max_dist_ref``. Agrega Chiloé, que el conjunto original no tenía con
  shorelines por fecha.

Cada script escribe sus salidas en ``OUT_DIR`` (este directorio para
``original``, ``ocean/`` para ``ocean``), así que las cifras del paper
anterior no se sobreescriben.
"""
import os
import pickle

P4 = '/home/franciscoparrao/proyectos/zenodo-gaps/propuesta-4-erosion-costera'
KING = '/mnt/kingston'
HERE = os.path.dirname(os.path.abspath(__file__))

DATASET = os.environ.get('SDS_DATASET', 'original')
if DATASET not in ('original', 'ocean'):
    raise SystemExit(f'SDS_DATASET inválido: {DATASET!r} (original | ocean)')

LABELS = {
    'VINA_DEL_MAR': 'Viña del Mar', 'COQUIMBO': 'Coquimbo',
    'CONSTITUCION': 'Constitución', 'VALDIVIA': 'Valdivia', 'CHILOE': 'Chiloé',
}

if DATASET == 'original':
    SITES = ['VINA_DEL_MAR', 'COQUIMBO', 'CONSTITUCION', 'VALDIVIA']
    _ANALYSIS = {
        'VINA_DEL_MAR': f'{P4}/output/VINA_DEL_MAR_tidal_corrected.pkl',
        'COQUIMBO': f'{P4}/output/COQUIMBO_analysis.pkl',
        'CONSTITUCION': f'{P4}/output/CONSTITUCION_analysis.pkl',
        'VALDIVIA': f'{P4}/output/VALDIVIA_analysis.pkl',
    }
    _OUTPUT = {s: f'{KING}/{s}/{s}_output.pkl' for s in SITES}
    OUT_DIR = HERE
else:
    SITES = ['VINA_DEL_MAR', 'COQUIMBO', 'CONSTITUCION', 'VALDIVIA', 'CHILOE']
    _ANALYSIS = {s: f'{P4}/output_reprocessed/{s}_analysis_ocean.pkl' for s in SITES}
    _OUTPUT = {s: f'{P4}/data_reprocessed/{s}/{s}_output_masked.pkl' for s in SITES}
    _OUTPUT['CHILOE'] = f'{P4}/data_reprocessed/CHILOE/CHILOE_output.pkl'  # script 16, ya con máscara
    OUT_DIR = os.path.join(HERE, 'ocean')
    os.makedirs(OUT_DIR, exist_ok=True)


def load(site):
    """(análisis, output) del sitio: transectos/referencia y shorelines por fecha."""
    an = pickle.load(open(_ANALYSIS[site], 'rb'))
    out = pickle.load(open(_OUTPUT[site], 'rb'))
    return an, out
