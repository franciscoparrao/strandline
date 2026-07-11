//! Clasificador de píxeles: MLP portado desde CoastSat
//! (`NN_4classes_Landsat_new.pkl`, kvos/CoastSat).
//!
//! Arquitectura: 20 features → Dense(100, ReLU) → Dense(4, softmax).
//! Los pesos se embeben en el binario (`models/coastsat_landsat_mlp.json`,
//! exportados con `spike/scripts/04_export_classifier.py`) — no hay
//! dependencia de sklearn en runtime. El softmax final se omite: es
//! monótono por fila, así que el argmax de los logits ya da la clase
//! predicha.

use std::sync::OnceLock;

use serde::Deserialize;

/// Clases del clasificador, en el orden de `MLPClassifier.classes_`.
pub const CLASS_OTHER: u8 = 0;
/// Clase sand (playa).
pub const CLASS_SAND: u8 = 1;
/// Clase whitewater (rompiente/swash).
pub const CLASS_WHITEWATER: u8 = 2;
/// Clase water (agua abierta).
pub const CLASS_WATER: u8 = 3;

#[derive(Deserialize)]
struct MlpJson {
    n_features: usize,
    hidden_size: usize,
    n_classes: usize,
    w1: Vec<Vec<f32>>,
    b1: Vec<f32>,
    w2: Vec<Vec<f32>>,
    b2: Vec<f32>,
}

/// Pesos en almacenamiento plano row-major para acceso cache-friendly en
/// el loop caliente (evita la indexación 2D con bounds-checking de
/// `ndarray`, que domina el costo del clasificador). `w1` es
/// `[n_features][hidden]`, `w2` es `[hidden][n_classes]`.
struct Mlp {
    n_features: usize,
    hidden: usize,
    n_classes: usize,
    w1: Vec<f32>,
    b1: Vec<f32>,
    w2: Vec<f32>,
    b2: Vec<f32>,
}

static MODEL: OnceLock<Mlp> = OnceLock::new();

const WEIGHTS_JSON: &str = include_str!("../models/coastsat_landsat_mlp.json");

/// Número máximo de neuronas de la capa oculta soportado sin heap en el
/// forward pass (el modelo de CoastSat usa 100). Si un modelo futuro lo
/// excede, [`predict`] cae a un buffer en heap.
const MAX_HIDDEN_STACK: usize = 128;

fn model() -> &'static Mlp {
    MODEL.get_or_init(|| {
        let raw: MlpJson =
            serde_json::from_str(WEIGHTS_JSON).expect("embedded MLP weights malformed");
        Mlp {
            n_features: raw.n_features,
            hidden: raw.hidden_size,
            n_classes: raw.n_classes,
            w1: raw.w1.into_iter().flatten().collect(),
            b1: raw.b1,
            w2: raw.w2.into_iter().flatten().collect(),
            b2: raw.b2,
        }
    })
}

/// Número de features que espera el modelo (20).
pub fn n_features() -> usize {
    model().n_features
}

/// Clasifica un vector de features. Retorna la clase con mayor logit
/// (`CLASS_OTHER`..`CLASS_WATER`). El softmax final se omite: es monótono
/// por fila, así que el argmax de los logits coincide con el de las
/// probabilidades. No valida NaN — el llamador debe filtrar píxeles
/// inválidos antes de llamar.
///
/// Los dos productos matriz-vector se recorren en orden axpy (índice de
/// entrada externo, de salida interno) para acceso secuencial a los pesos.
pub fn predict(features: &[f32]) -> u8 {
    let m = model();
    debug_assert_eq!(features.len(), m.n_features);
    debug_assert!(m.n_classes <= 8, "logits buffer holds at most 8 classes");
    let hidden = m.hidden;

    // capa oculta: h = ReLU(b1 + W1ᵀ · features)
    let mut h_stack = [0f32; MAX_HIDDEN_STACK];
    let mut h_heap;
    let h: &mut [f32] = if hidden <= MAX_HIDDEN_STACK {
        &mut h_stack[..hidden]
    } else {
        h_heap = vec![0f32; hidden];
        &mut h_heap[..]
    };
    h.copy_from_slice(&m.b1);
    for (i, &f) in features.iter().enumerate() {
        let row = &m.w1[i * hidden..(i + 1) * hidden];
        for (hj, &w) in h.iter_mut().zip(row) {
            *hj += f * w;
        }
    }
    for hj in h.iter_mut() {
        *hj = hj.max(0.0);
    }

    // capa de salida: logits = b2 + W2ᵀ · h ; se devuelve el argmax
    let n_classes = m.n_classes;
    let mut logits = [f32::NEG_INFINITY; 8];
    logits[..n_classes].copy_from_slice(&m.b2);
    for (j, &hv) in h.iter().enumerate() {
        let row = &m.w2[j * n_classes..(j + 1) * n_classes];
        for (lk, &w) in logits[..n_classes].iter_mut().zip(row) {
            *lk += hv * w;
        }
    }

    let mut best = 0usize;
    let mut best_val = f32::NEG_INFINITY;
    for (k, &v) in logits[..n_classes].iter().enumerate() {
        if v > best_val {
            best_val = v;
            best = k;
        }
    }
    best as u8
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn model_loads_and_has_expected_shape() {
        assert_eq!(n_features(), 20);
    }

    #[test]
    fn predict_returns_valid_class() {
        let features = vec![0.1f32; 20];
        let class = predict(&features);
        assert!(class <= CLASS_WATER);
    }
}
