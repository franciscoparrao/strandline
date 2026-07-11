#!/usr/bin/env python3
"""
Exporta el MLP de CoastSat (NN_4classes_Landsat_new.pkl) a JSON portable,
para poder hacer el forward pass en Rust sin depender de sklearn.

Arquitectura: 20 features -> Dense(100, ReLU) -> Dense(4, softmax).
Nota: el modelo NO tiene un StandardScaler embebido (sklearn Pipeline);
es un MLPClassifier crudo — CoastSat entrena y aplica directo sobre las
features sin normalizar. Se verifica eso mismo al exportar.
"""

import json
import os

import joblib
import numpy as np

MODEL_PATH = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat/"
    "classification/models/NN_4classes_Landsat_new.pkl"
)
OUT_PATH = os.path.expanduser(
    "~/proyectos/strandline/strandline-core/models/coastsat_landsat_mlp.json"
)

clf = joblib.load(MODEL_PATH)

assert clf.n_layers_ == 3, f"arquitectura inesperada: {clf.n_layers_} capas"
assert clf.activation == "relu"
assert clf.out_activation_ == "softmax"

model = {
    "source": "CoastSat NN_4classes_Landsat_new.pkl (kvos/CoastSat)",
    "n_features": int(clf.coefs_[0].shape[0]),
    "hidden_size": int(clf.coefs_[0].shape[1]),
    "n_classes": int(clf.coefs_[1].shape[1]),
    "classes": [int(c) for c in clf.classes_],
    "class_names": ["other", "sand", "whitewater", "water"],
    "w1": clf.coefs_[0].tolist(),
    "b1": clf.intercepts_[0].tolist(),
    "w2": clf.coefs_[1].tolist(),
    "b2": clf.intercepts_[1].tolist(),
}

with open(OUT_PATH, "w") as f:
    json.dump(model, f)

print(f"Exportado: {OUT_PATH}")
print(f"  n_features={model['n_features']} hidden={model['hidden_size']} "
      f"n_classes={model['n_classes']}")
size_kb = os.path.getsize(OUT_PATH) / 1024
print(f"  tamaño: {size_kb:.1f} KB")
