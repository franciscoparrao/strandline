//! Síntesis armónica de marea desde constituyentes locales.
//!
//! Predice la elevación de marea `h(t) = Σ_k f_k·A_k·cos(V0_k + u_k − g_k)`
//! donde `A_k, g_k` son la amplitud (m) y fase de Greenwich (grados) del
//! constituyente `k` (locales, provistos por el usuario o un tide gauge),
//! `V0_k` es el argumento astronómico de equilibrio y `f_k, u_k` las
//! correcciones nodales (Schureman 1958). Es el "port mínimo" de la
//! predicción de marea: no lee grids globales (GOT/FES) — toma los
//! constituyentes ya locales, que es lo que evita el *dependency hell*.
//!
//! Convención: `τ` = tiempo lunar medio (`15°·hora_UT + H − S`), con lo que
//! los coeficientes Doodson salen limpios (M2 = 2τ, S2 = 2τ+2s−2h, …) y el
//! efecto del nodo lunar queda íntegramente en `f, u`.

use crate::tide::astro::{lunar_time_tau, mean_longitudes};

/// Correcciones nodales `f` (factor de amplitud) y `u` (offset de fase,
/// grados) aplicables a cada constituyente. Seleccionan las fórmulas de
/// Schureman en función de la longitud del nodo lunar `N`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeCorrection {
    /// Constituyentes solares o de largo período sin dependencia nodal
    /// apreciable (S2, P1, Sa, Ssa): `f = 1`, `u = 0`.
    None,
    /// Grupo lunar semidiurno principal (M2, N2, …).
    M2,
    /// Grupo lunar diurno principal (O1, Q1, …).
    O1,
    /// Declinacional diurno lunisolar K1.
    K1,
    /// Declinacional semidiurno lunisolar K2.
    K2,
    /// Quincenal lunar Mf.
    Mf,
    /// Mensual lunar Mm.
    Mm,
    /// Somera cuarto-diurna M4 (`f = f_M2²`, `u = 2·u_M2`).
    M4,
}

impl NodeCorrection {
    /// Devuelve `(f, u_grados)` para la longitud del nodo `n` (grados).
    fn factors(self, n_deg: f64) -> (f64, f64) {
        let n = n_deg.to_radians();
        let (c1, c2, c3) = (n.cos(), (2.0 * n).cos(), (3.0 * n).cos());
        let (s1, s2, s3) = (n.sin(), (2.0 * n).sin(), (3.0 * n).sin());
        match self {
            NodeCorrection::None => (1.0, 0.0),
            NodeCorrection::M2 => (1.0004 - 0.0373 * c1 + 0.0002 * c2, -2.14 * s1),
            NodeCorrection::O1 => (
                1.0089 + 0.1871 * c1 - 0.0147 * c2 + 0.0014 * c3,
                10.80 * s1 - 1.34 * s2 + 0.19 * s3,
            ),
            NodeCorrection::K1 => (
                1.0060 + 0.1150 * c1 - 0.0088 * c2 + 0.0006 * c3,
                -8.86 * s1 + 0.68 * s2 - 0.07 * s3,
            ),
            NodeCorrection::K2 => (
                1.0241 + 0.2863 * c1 + 0.0083 * c2 - 0.0015 * c3,
                -17.74 * s1 + 0.68 * s2 - 0.04 * s3,
            ),
            NodeCorrection::Mf => (1.0429 + 0.4135 * c1 - 0.004 * c2, -23.7 * s1 + 2.7 * s2 - 0.4 * s3),
            NodeCorrection::Mm => (1.0 - 0.1300 * c1 + 0.0013 * c2, 0.0),
            NodeCorrection::M4 => {
                let (fm2, um2) = NodeCorrection::M2.factors(n_deg);
                (fm2 * fm2, 2.0 * um2)
            }
        }
    }
}

/// Definición astronómica de un constituyente de marea: coeficientes
/// Doodson en `[τ, s, h, p, ps]` (el término del nodo va en `node`), offset
/// de fase constante (grados) y tipo de corrección nodal.
#[derive(Debug, Clone, Copy)]
pub struct ConstituentDef {
    /// Nombre estándar (Darwin), ej. `"M2"`.
    pub name: &'static str,
    /// Coeficientes `[c_τ, c_s, c_h, c_p, c_ps]` del argumento de equilibrio.
    pub doodson: [f64; 5],
    /// Offset de fase constante del argumento (grados).
    pub phase: f64,
    /// Corrección nodal a aplicar.
    pub node: NodeCorrection,
}

impl ConstituentDef {
    /// Argumento astronómico de equilibrio `V0` (grados, sin corrección
    /// nodal) en el instante dado por las longitudes medias `l` y el tiempo
    /// lunar `tau`.
    #[inline]
    fn equilibrium(&self, tau: f64, l: &crate::tide::astro::MeanLongitudes) -> f64 {
        self.doodson[0] * tau
            + self.doodson[1] * l.s
            + self.doodson[2] * l.h
            + self.doodson[3] * l.p
            + self.doodson[4] * l.ps
            + self.phase
    }
}

/// Los 13 constituyentes principales (4 semidiurnos, 4 diurnos, 3 de largo
/// período, 1 somera + Sa). Cubren >95% de la varianza mareal en la mayoría
/// de las costas. Coeficientes verificados contra las velocidades angulares
/// conocidas (ej. M2 = 2τ = 28.9841042°/h).
pub const PRINCIPAL_CONSTITUENTS: &[ConstituentDef] = &[
    // Semidiurnos
    ConstituentDef { name: "M2", doodson: [2.0, 0.0, 0.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::M2 },
    ConstituentDef { name: "S2", doodson: [2.0, 2.0, -2.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::None },
    ConstituentDef { name: "N2", doodson: [2.0, -1.0, 0.0, 1.0, 0.0], phase: 0.0, node: NodeCorrection::M2 },
    ConstituentDef { name: "K2", doodson: [2.0, 2.0, 0.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::K2 },
    // Diurnos. Offsets de fase verificados contra el argumento de
    // equilibrio G de pyTMD: K1 lleva +90°, el resto de los diurnos −90°.
    ConstituentDef { name: "K1", doodson: [1.0, 1.0, 0.0, 0.0, 0.0], phase: 90.0, node: NodeCorrection::K1 },
    ConstituentDef { name: "O1", doodson: [1.0, -1.0, 0.0, 0.0, 0.0], phase: -90.0, node: NodeCorrection::O1 },
    ConstituentDef { name: "P1", doodson: [1.0, 1.0, -2.0, 0.0, 0.0], phase: -90.0, node: NodeCorrection::None },
    ConstituentDef { name: "Q1", doodson: [1.0, -2.0, 0.0, 1.0, 0.0], phase: -90.0, node: NodeCorrection::O1 },
    // Largo período
    ConstituentDef { name: "Mf", doodson: [0.0, 2.0, 0.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::Mf },
    ConstituentDef { name: "Mm", doodson: [0.0, 1.0, 0.0, -1.0, 0.0], phase: 0.0, node: NodeCorrection::Mm },
    ConstituentDef { name: "Ssa", doodson: [0.0, 0.0, 2.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::None },
    // Sa: argumento h − ps (convención GOT/pyTMD, la misma de los demás
    // constituyentes de esta tabla). Con [0,0,1,0,0] (solo h) la fase de la
    // componente anual queda ~283° corrida — bug B2 de la auditoría 2026-07.
    ConstituentDef { name: "Sa", doodson: [0.0, 0.0, 1.0, 0.0, -1.0], phase: 0.0, node: NodeCorrection::None },
    // Somera
    ConstituentDef { name: "M4", doodson: [4.0, 0.0, 0.0, 0.0, 0.0], phase: 0.0, node: NodeCorrection::M4 },
];

/// Amplitud (m) y fase de Greenwich (grados) local de un constituyente.
#[derive(Debug, Clone, Copy)]
pub struct Constituent {
    /// Nombre estándar; debe existir en [`PRINCIPAL_CONSTITUENTS`].
    pub name: &'static str,
    /// Amplitud local (m).
    pub amplitude: f64,
    /// Fase de Greenwich local (grados).
    pub phase_g: f64,
}

/// Busca la definición astronómica de un constituyente por su nombre
/// (case-insensitive), si está en [`PRINCIPAL_CONSTITUENTS`].
pub fn lookup(name: &str) -> Option<ConstituentDef> {
    PRINCIPAL_CONSTITUENTS
        .iter()
        .find(|d| d.name.eq_ignore_ascii_case(name))
        .copied()
}

/// Término resuelto del predictor: amplitud/fase locales + definición
/// astronómica (guardada por valor, sin lifetimes).
struct PreparedTerm {
    amplitude: f64,
    phase_g: f64,
    def: ConstituentDef,
}

/// Predictor de marea configurado con un juego de constituyentes locales.
pub struct TidePredictor {
    terms: Vec<PreparedTerm>,
    /// Nivel medio del mar respecto al datum (m).
    mean_sea_level: f64,
}

impl TidePredictor {
    /// Construye el predictor emparejando cada constituyente local con su
    /// definición en [`PRINCIPAL_CONSTITUENTS`]. Los nombres desconocidos se
    /// descartan y se devuelven en la segunda componente.
    pub fn new(constituents: &[Constituent], mean_sea_level: f64) -> (Self, Vec<String>) {
        let records: Vec<(String, f64, f64)> = constituents
            .iter()
            .map(|c| (c.name.to_string(), c.amplitude, c.phase_g))
            .collect();
        Self::from_records(&records, mean_sea_level)
    }

    /// Igual que [`Self::new`] pero tomando nombres como `String` (para
    /// entradas de CSV / CLI, sin requerir `&'static str`).
    pub fn from_records(
        records: &[(String, f64, f64)],
        mean_sea_level: f64,
    ) -> (Self, Vec<String>) {
        let mut terms = Vec::new();
        let mut unknown = Vec::new();
        for (name, amplitude, phase_g) in records {
            match lookup(name) {
                Some(def) => terms.push(PreparedTerm {
                    amplitude: *amplitude,
                    phase_g: *phase_g,
                    def,
                }),
                None => unknown.push(name.clone()),
            }
        }
        (
            Self {
                terms,
                mean_sea_level,
            },
            unknown,
        )
    }

    /// Cantidad de constituyentes activos.
    pub fn len(&self) -> usize {
        self.terms.len()
    }

    /// `true` si no hay constituyentes configurados.
    pub fn is_empty(&self) -> bool {
        self.terms.is_empty()
    }

    /// Predice la elevación de marea (m) en el instante `mjd`.
    pub fn predict(&self, mjd: f64) -> f64 {
        let l = mean_longitudes(mjd);
        let tau = lunar_time_tau(mjd, &l);
        let mut h = self.mean_sea_level;
        for t in &self.terms {
            let v0 = t.def.equilibrium(tau, &l);
            let (f, u) = t.def.node.factors(l.n);
            let arg = (v0 + u - t.phase_g).to_radians();
            h += f * t.amplitude * arg.cos();
        }
        h
    }

    /// Predice en varios instantes.
    pub fn predict_many(&self, mjds: &[f64]) -> Vec<f64> {
        mjds.iter().map(|&m| self.predict(m)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Velocidad angular (°/h) de cada constituyente derivada de sus
    /// coeficientes Doodson y las velocidades de las longitudes medias.
    fn speed_of(def: &ConstituentDef) -> f64 {
        // °/h de τ, s, h, p, ps
        const V: [f64; 5] = [
            14.492_052_1, // τ (tiempo lunar medio)
            0.549_016_5,  // s
            0.041_068_6,  // h
            0.004_641_8,  // p
            0.000_002_0,  // ps
        ];
        def.doodson.iter().zip(V).map(|(c, v)| c * v).sum()
    }

    #[test]
    fn constituent_speeds_match_known_values() {
        // valores de referencia (°/h) de las velocidades angulares clásicas
        let expected = [
            ("M2", 28.9841042),
            ("S2", 30.0000000),
            ("N2", 28.4397295),
            ("K2", 30.0821373),
            ("K1", 15.0410686),
            ("O1", 13.9430356),
            ("P1", 14.9589314),
            ("Q1", 13.3986609),
            ("M4", 57.9682084),
        ];
        for (name, want) in expected {
            let def = PRINCIPAL_CONSTITUENTS.iter().find(|d| d.name == name).unwrap();
            let got = speed_of(def);
            assert!((got - want).abs() < 1e-3, "{name}: got {got}, want {want}");
        }
    }

    #[test]
    fn pure_m2_is_periodic_near_12_42h() {
        // Un tren M2 puro debe tener período ≈ 12.42 h. Verificamos que la
        // elevación en t y t+12.42h coincide de cerca.
        let (pred, _) = TidePredictor::new(
            &[Constituent { name: "M2", amplitude: 1.0, phase_g: 0.0 }],
            0.0,
        );
        let mjd0 = 58858.5;
        let period_days = 12.420601 / 24.0;
        let a = pred.predict(mjd0);
        let b = pred.predict(mjd0 + period_days);
        assert!((a - b).abs() < 2e-3, "M2 no periódico: {a} vs {b}");
    }

    fn norm360(x: f64) -> f64 {
        let m = x % 360.0;
        if m < 0.0 {
            m + 360.0
        } else {
            m
        }
    }

    #[test]
    fn equilibrium_and_nodal_match_pytmd() {
        use crate::tide::astro::{lunar_time_tau, mean_longitudes};
        // Referencia: pyTMD.constituents.arguments(mjd, cons, corrections='GOT').
        // (name, G_deg, f, u_deg) en MJD 58858.6068518.
        let ref_data = [
            ("M2", 72.3498, 1.00521, -2.1157),
            ("S2", 76.9333, 0.99970, 0.1278),
            ("N2", 138.6318, 1.00521, -2.1157),
            ("K2", 296.1261, 0.97734, -17.8346),
            ("K1", 238.0630, 0.99918, -8.9133),
            ("O1", 194.2868, 0.99186, 10.9759),
            ("P1", 198.8703, 1.00157, -0.6349),
            ("Q1", 260.5688, 0.99237, 10.8548),
            ("Mf", 223.7762, 1.02993, -22.7198),
            ("Mm", 293.7180, 0.96896, 1.4712),
            ("Ssa", 219.1928, 1.00000, 0.0000),
            ("M4", 144.6996, 1.01044, -4.2313),
        ];
        let mjd = 58858.6068518;
        let l = mean_longitudes(mjd);
        let tau = lunar_time_tau(mjd, &l);
        for (name, g, f_ref, u_ref) in ref_data {
            let def = PRINCIPAL_CONSTITUENTS.iter().find(|d| d.name == name).unwrap();
            let v0 = norm360(def.equilibrium(tau, &l));
            let (f, u) = def.node.factors(l.n);
            // El argumento de equilibrio V0 debe coincidir con pyTMD muy de
            // cerca — es la parte astronómica exacta del motor.
            let dg = (norm360(v0 - g + 180.0) - 180.0).abs();
            assert!(dg < 0.01, "{name} V0: {v0} vs {g} (Δ {dg})");
            // Los factores nodales f, u de Schureman clásico difieren de las
            // tablas GOT de pyTMD según el constituyente: los de largo
            // período (Mf, Mm) usan una expansión distinta (~4% en f) y P1
            // se trata como solar puro. Su bajo peso hace que el error de
            // predicción total siga siendo sub-cm (ver test siguiente).
            let (f_tol, u_tol) = match name {
                "Mf" | "Mm" => (0.05, 1.6),
                "P1" => (0.01, 0.7),
                _ => (0.01, 0.35),
            };
            assert!((f - f_ref).abs() < f_tol, "{name} f: {f} vs {f_ref}");
            assert!((u - u_ref).abs() < u_tol, "{name} u: {u} vs {u_ref}");
        }
    }

    #[test]
    fn total_prediction_matches_pytmd() {
        // Validación decisiva del motor: predicción total contra
        // pyTMD.constituents.arguments (corrections='GOT') para
        // constituyentes realistas de la costa central de Chile.
        // Cubre los 13 constituyentes de la tabla — incluido Sa, cuya
        // omisión original escondió el bug B2 (auditoría 2026-07).
        // (name, amplitud m, fase Greenwich grados)
        let cons = [
            ("M2", 0.42, 95.0), ("S2", 0.14, 130.0), ("N2", 0.10, 78.0), ("K2", 0.04, 128.0),
            ("K1", 0.15, 330.0), ("O1", 0.10, 300.0), ("P1", 0.05, 325.0), ("Q1", 0.02, 285.0),
            ("Mf", 0.02, 100.0), ("Mm", 0.015, 80.0), ("Ssa", 0.03, 200.0), ("Sa", 0.04, 150.0),
            ("M4", 0.01, 60.0),
        ];
        let constituents: Vec<Constituent> = cons
            .iter()
            .map(|&(name, amplitude, phase_g)| Constituent { name, amplitude, phase_g })
            .collect();
        let (pred, unknown) = TidePredictor::new(&constituents, 0.0);
        assert!(unknown.is_empty());
        assert_eq!(pred.len(), PRINCIPAL_CONSTITUENTS.len());

        // (mjd, h_ref_m) de pyTMD
        let refs = [
            (46090.5, -0.590002),
            (51544.0, 0.375738),
            (55197.25, -0.045139),
            (58858.6068518, 0.421107),
            (60000.75, 0.447281),
            (60310.1, -0.106925),
        ];
        let mut max_err = 0.0f64;
        for (mjd, h_ref) in refs {
            let h = pred.predict(mjd);
            max_err = max_err.max((h - h_ref).abs());
        }
        // El error máximo respecto a pyTMD es de pocos mm pese a usar
        // factores nodales Schureman en vez de las tablas GOT —
        // despreciable para la corrección de shorelines (1 mm de marea ≈
        // 1 cm horizontal con pendiente 0.1, << pixel de 15 m).
        assert!(max_err < 0.003, "error máximo de predicción: {max_err:.4} m");
    }

    #[test]
    fn unknown_constituents_reported() {
        let (_, unknown) = TidePredictor::new(
            &[Constituent { name: "ZZ9", amplitude: 1.0, phase_g: 0.0 }],
            0.0,
        );
        assert_eq!(unknown, vec!["ZZ9".to_string()]);
    }
}
