//! Corrección mareal de shorelines satelitales.
//!
//! Dos piezas independientes:
//!
//! 1. **Corrección horizontal** ([`correct_chainage`]): dada la elevación
//!    de marea al momento de cada imagen, desplaza la posición cross-shore
//!    de la shoreline a un datum de referencia común, usando la pendiente
//!    de playa (Vos et al. 2019). Es la corrección que elimina el "ruido"
//!    mareal de las series de cambio costero.
//!
//! 2. **Predicción de marea** (módulos [`astro`] y [`harmonic`]): síntesis
//!    armónica nativa desde constituyentes locales, sin depender de pyTMD /
//!    modelos globales en runtime. Alternativamente, los niveles pueden
//!    leerse de un CSV externo (SHOA, pyTMD, tide gauge) con [`read_csv`].
//!
//! El diferenciador de strandline frente a CoastSat es tener (2) integrado
//! en el binario: el paper de erosión costera de Chile sufrió el
//! *dependency hell* de pyfes/pyTMD (conda-forge, GOT5.5) justamente aquí.

pub mod astro;
pub mod harmonic;

mod csv;
pub use csv::{parse_timestamp, read_constituents_csv, read_csv, TideSeries};

/// Parámetros de la corrección horizontal de marea.
///
/// Los campos son privados a propósito: `beach_slope` es un divisor, así que
/// se construye solo vía [`TideCorrection::new`], que valida el dominio.
#[derive(Debug, Clone, Copy)]
pub struct TideCorrection {
    /// Pendiente de la cara de playa, `tan(β)` (adimensional, m/m). Para
    /// playas arenosas microtidales de Chile central ≈ 0.08–0.12.
    beach_slope: f64,
    /// Elevación del datum de referencia al que se corrige (m). Típicamente
    /// 0 = nivel medio del mar del modelo de marea.
    reference_elevation: f64,
}

impl TideCorrection {
    /// Crea los parámetros de corrección.
    ///
    /// # Errors
    /// Si `beach_slope` no es finito o no es estrictamente positivo: la
    /// corrección lo usa como divisor, así que un 0 convertiría la serie
    /// entera en `±inf`/`NaN` en silencio y un valor negativo invertiría el
    /// signo del desplazamiento.
    pub fn new(beach_slope: f64, reference_elevation: f64) -> crate::error::Result<Self> {
        if !beach_slope.is_finite() || beach_slope <= 0.0 {
            return Err(crate::error::StrandlineError::InvalidParameter {
                name: "beach_slope".into(),
                reason: format!("debe ser finito y > 0, se recibió {beach_slope}"),
            });
        }
        if !reference_elevation.is_finite() {
            return Err(crate::error::StrandlineError::InvalidParameter {
                name: "reference_elevation".into(),
                reason: format!("debe ser finita, se recibió {reference_elevation}"),
            });
        }
        Ok(Self {
            beach_slope,
            reference_elevation,
        })
    }

    /// Desplazamiento horizontal (m) que induce una elevación de marea
    /// `tide` respecto al datum de referencia, según Vos et al. (2019):
    /// `Δx = (tide − reference) / beach_slope`.
    ///
    /// Positivo cuando la marea está sobre el datum: con marea alta la
    /// línea de agua aparece más hacia tierra sobre la cara de playa, así
    /// que la distancia cross-shore cruda es mayor de lo que debería y hay
    /// que restarle este desplazamiento.
    #[inline]
    pub fn horizontal_shift(&self, tide: f64) -> f64 {
        (tide - self.reference_elevation) / self.beach_slope
    }

    /// Pendiente de la cara de playa usada, `tan(β)`.
    #[inline]
    pub fn beach_slope(&self) -> f64 {
        self.beach_slope
    }

    /// Elevación del datum de referencia (m).
    #[inline]
    pub fn reference_elevation(&self) -> f64 {
        self.reference_elevation
    }
}

/// Corrige una serie de distancias cross-shore `chainage` a un datum común
/// dadas las elevaciones de marea `tides` (una por observación) y la
/// [`TideCorrection`].
///
/// `corrected[i] = chainage[i] − (tides[i] − reference) / beach_slope`.
///
/// Los `NaN` en `chainage` (observaciones faltantes) se propagan tal cual.
///
/// # Panics
/// Si `chainage.len() != tides.len()`.
pub fn correct_chainage(chainage: &[f64], tides: &[f64], params: &TideCorrection) -> Vec<f64> {
    assert_eq!(
        chainage.len(),
        tides.len(),
        "chainage y tides deben tener el mismo largo"
    );
    chainage
        .iter()
        .zip(tides)
        .map(|(&raw, &tide)| raw - params.horizontal_shift(tide))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_shift_matches_paper_formula() {
        // Ground truth del paper: beach_slope=0.10, ref=0.0.
        // correction = tide / 0.10 = tide * 10.
        let p = TideCorrection::new(0.10, 0.0).unwrap();
        for (tide, expected) in [(-0.1945, -1.945), (0.0403, 0.403), (0.9690, 9.690)] {
            assert!((p.horizontal_shift(tide) - expected).abs() < 1e-9);
        }
    }

    #[test]
    fn correct_chainage_subtracts_shift() {
        let p = TideCorrection::new(0.10, 0.0).unwrap();
        let chainage = vec![100.0, 100.0, f64::NAN];
        let tides = vec![0.5, -0.5, 0.2];
        let out = correct_chainage(&chainage, &tides, &p);
        assert!((out[0] - (100.0 - 5.0)).abs() < 1e-9);
        assert!((out[1] - (100.0 + 5.0)).abs() < 1e-9);
        assert!(out[2].is_nan());
    }

    #[test]
    fn reference_elevation_offsets_datum() {
        let p = TideCorrection::new(0.10, 0.2).unwrap();
        // con ref=0.2, una marea de 0.2 no induce desplazamiento
        assert!(p.horizontal_shift(0.2).abs() < 1e-12);
    }

    /// `beach_slope` es un divisor: un 0 (typo del CLI, o un slope estimado
    /// degenerado) convertía la serie entera en ±inf/NaN en silencio.
    #[test]
    fn pendiente_no_positiva_o_no_finita_se_rechaza() {
        for bad in [0.0, -0.1, f64::NAN, f64::INFINITY] {
            assert!(
                TideCorrection::new(bad, 0.0).is_err(),
                "beach_slope {bad} debería rechazarse"
            );
        }
        assert!(TideCorrection::new(0.1, f64::NAN).is_err());
        assert!(TideCorrection::new(0.1, 0.0).is_ok());
    }
}
