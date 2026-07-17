#!/usr/bin/env python3
"""
Entrena el MLP de clasificación de píxeles sobre las labels SR producidas
por `label_sr_images.py` (hallazgo E4: reentrenamiento para Surface
Reflectance, ver strandline CLAUDE.md adenda pendiente).

Arquitectura idéntica a la de producción `NN_4classes_Landsat_new.pkl`
(no la de ejemplo del notebook de CoastSat, que es distinta):
    20 features -> Dense(100, ReLU) -> Dense(4, softmax)
    labels: other=0, sand=1, white-water=2, water=3
Reusa `SDS_classify.load_labels` / `format_training_data` sin modificar,
porque `label_sr_images.py` escribe el mismo formato de pkl que CoastSat.

Uso:
    python3 train_sr_classifier.py --train-dir <out-dir de label_sr_images> \
        --sites VINA ARICA VALDIVIA --out models/coastsat_sr_mlp.pkl
"""
import argparse
import os
import sys

import joblib
import numpy as np
from sklearn.model_selection import train_test_split
from sklearn.neural_network import MLPClassifier
from sklearn.metrics import confusion_matrix, classification_report

COASTSAT_ROOT = os.path.expanduser(
    "~/proyectos/zenodo-gaps/propuesta-4-erosion-costera/CoastSat"
)
sys.path.insert(0, COASTSAT_ROOT)
from coastsat.SDS_classify import load_labels, format_training_data  # noqa: E402

LABELS = {"sand": 1, "white-water": 2, "water": 3, "other land features": 0}
CLASS_NAMES = ["other", "sand", "whitewater", "water"]  # índice = valor de label


def main():
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--train-dir", required=True, help="filepath_train (raíz con subcarpetas por sitio)")
    ap.add_argument("--sites", nargs="+", required=True, help="nombres de sitio (subcarpetas) a incluir")
    ap.add_argument("--out", required=True, help="ruta de salida del .pkl entrenado")
    ap.add_argument("--test-size", type=float, default=0.3)
    ap.add_argument("--random-state", type=int, default=0)
    args = ap.parse_args()

    settings = {"filepath_train": args.train_dir, "labels": LABELS}
    # load_labels espera nombres de "sitio.algo" (usa site[:site.find('.')])
    train_sites = [f"{s}.site" for s in args.sites]
    features = load_labels(train_sites, settings)

    classes = list(LABELS.keys())
    label_values = [LABELS[c] for c in classes]
    X, y = format_training_data(features, classes, label_values)
    y = y.ravel()

    print(f"\nTotal: {X.shape[0]} píxeles, {X.shape[1]} features")
    X_train, X_test, y_train, y_test = train_test_split(
        X, y, test_size=args.test_size, shuffle=True, random_state=args.random_state
    )

    clf = MLPClassifier(hidden_layer_sizes=(100,), solver="adam", random_state=args.random_state)
    clf.fit(X_train, y_train)
    acc = clf.score(X_test, y_test)
    print(f"Accuracy (held-out {args.test_size:.0%}): {acc:.4f}")

    y_pred = clf.predict(X_test)
    cm = confusion_matrix(y_test, y_pred, labels=[0, 1, 2, 3])
    print("\nMatriz de confusión (filas=verdad, cols=predicción; orden other/sand/whitewater/water):")
    print(cm)
    print("\n", classification_report(y_test, y_pred, target_names=CLASS_NAMES, labels=[0, 1, 2, 3]))

    if acc < 0.90:
        print(f"\nADVERTENCIA: accuracy {acc:.4f} < 0.90 — CoastSat original reporta >95%. "
              "Considera etiquetar más imágenes o revisar clases desbalanceadas antes de exportar.")

    # entrena la versión final con TODOS los datos (igual que el notebook original)
    clf_final = MLPClassifier(hidden_layer_sizes=(100,), solver="adam", random_state=args.random_state)
    clf_final.fit(X, y)
    assert clf_final.n_layers_ == 3, f"arquitectura inesperada: {clf_final.n_layers_} capas"
    assert clf_final.out_activation_ == "softmax"

    os.makedirs(os.path.dirname(args.out) or ".", exist_ok=True)
    joblib.dump(clf_final, args.out)
    print(f"\nGuardado: {args.out}")


if __name__ == "__main__":
    main()
