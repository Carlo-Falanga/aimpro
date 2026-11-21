//! La camera in prima persona. Convenzioni: Y in alto, a imbardata 0 si guarda verso -Z; imbardata positiva
//! gira a destra, beccheggio positivo guarda in alto. Gli angoli restano in f64 (si sommano migliaia di count
//! al secondo); in f32 si passa solo la matrice finale per la scheda video.

use glam::{DMat4, DVec3, Mat4};

const BECCHEGGIO_MAX: f64 = 89.0 * std::f64::consts::PI / 180.0;
pub const VICINO: f64 = 0.05;
pub const LONTANO: f64 = 100.0;

pub fn limita_beccheggio(p: f64) -> f64 {
    p.clamp(-BECCHEGGIO_MAX, BECCHEGGIO_MAX)
}

/// Il versore dello sguardo.
pub fn avanti(imbardata: f64, beccheggio: f64) -> DVec3 {
    let cp = beccheggio.cos();
    DVec3::new(imbardata.sin() * cp, beccheggio.sin(), -imbardata.cos() * cp)
}

/// Il versore a destra dello sguardo (sempre orizzontale).
pub fn destra(imbardata: f64) -> DVec3 {
    DVec3::new(imbardata.cos(), 0.0, imbardata.sin())
}

/// Proiezione per vista, con la profondità 0..1 di wgpu.
pub fn vista_proiezione(
    occhio: DVec3,
    imbardata: f64,
    beccheggio: f64,
    fov_y: f64,
    proporzione: f64,
) -> Mat4 {
    // Vista: base destra r, su u, avanti f (destrorsa, la camera guarda verso -f nel suo spazio).
    let f = avanti(imbardata, beccheggio);
    let r = destra(imbardata);
    let u = r.cross(f);
    let vista = DMat4::from_cols_array(&[
        r.x,
        u.x,
        -f.x,
        0.0,
        r.y,
        u.y,
        -f.y,
        0.0,
        r.z,
        u.z,
        -f.z,
        0.0,
        -r.dot(occhio),
        -u.dot(occhio),
        f.dot(occhio),
        1.0,
    ]);
    // Prospettiva destrorsa con profondità 0..1 (quella di wgpu e DirectX).
    let k = 1.0 / (fov_y / 2.0).tan();
    let ni = 1.0 / (VICINO - LONTANO);
    let proiezione = DMat4::from_cols_array(&[
        k / proporzione,
        0.0,
        0.0,
        0.0,
        0.0,
        k,
        0.0,
        0.0,
        0.0,
        0.0,
        LONTANO * ni,
        -1.0,
        0.0,
        0.0,
        VICINO * LONTANO * ni,
        0.0,
    ]);
    (proiezione * vista).as_mat4()
}

/// Il raggio dall'occhio nella direzione `d` (versore) tocca la sfera?
pub fn colpisce_sfera(occhio: DVec3, d: DVec3, centro: DVec3, raggio: f64) -> bool {
    let oc = centro - occhio;
    let t = oc.dot(d);
    if t < 0.0 {
        return false;
    }
    oc.length_squared() - t * t <= raggio * raggio
}

#[cfg(test)]
mod test {
    use super::*;
    use glam::Vec4;

    fn vicino(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-9, "{a} non è {b}");
    }

    #[test]
    fn a_zero_si_guarda_verso_meno_z() {
        let d = avanti(0.0, 0.0);
        vicino(d.x, 0.0);
        vicino(d.y, 0.0);
        vicino(d.z, -1.0);
    }

    #[test]
    fn imbardata_positiva_gira_a_destra() {
        let d = avanti(90f64.to_radians(), 0.0);
        vicino(d.x, 1.0);
        vicino(d.z, 0.0);
        let r = destra(0.0);
        vicino(r.x, 1.0);
    }

    #[test]
    fn beccheggio_positivo_guarda_in_alto() {
        vicino(avanti(0.0, 30f64.to_radians()).y, 0.5);
    }

    #[test]
    fn beccheggio_limitato_a_89_gradi() {
        vicino(limita_beccheggio(2.0), 89f64.to_radians());
        vicino(limita_beccheggio(-2.0), -(89f64.to_radians()));
        vicino(limita_beccheggio(0.1), 0.1);
    }

    #[test]
    fn un_punto_davanti_finisce_al_centro() {
        let occhio = DVec3::new(0.0, 1.7, 0.0);
        let (y, p) = (37f64.to_radians(), 12f64.to_radians());
        let m = vista_proiezione(occhio, y, p, 70f64.to_radians(), 16.0 / 9.0);
        let punto = occhio + avanti(y, p) * 10.0;
        let c = m * Vec4::new(punto.x as f32, punto.y as f32, punto.z as f32, 1.0);
        assert!((c.x / c.w).abs() < 1e-5 && (c.y / c.w).abs() < 1e-5 && c.w > 0.0);
        let z = c.z / c.w;
        assert!((0.0..=1.0).contains(&z), "profondità {z} fuori da 0..1");
    }

    #[test]
    fn meta_fov_verticale_tocca_il_bordo_alto() {
        let m = vista_proiezione(DVec3::ZERO, 0.0, 0.0, 60f64.to_radians(), 1.0);
        let y = (30f64.to_radians().tan() * 5.0) as f32;
        let c = m * Vec4::new(0.0, y, -5.0, 1.0);
        assert!((c.y / c.w - 1.0).abs() < 1e-5);
    }

    #[test]
    fn colpo_al_centro_e_al_bordo() {
        let centro = DVec3::new(0.0, 0.0, -10.0);
        assert!(colpisce_sfera(DVec3::ZERO, DVec3::NEG_Z, centro, 0.5));
        let a = (0.5f64 / 10.0).asin();
        let dentro = DVec3::new((a * 0.99).sin(), 0.0, -(a * 0.99).cos());
        let fuori = DVec3::new((a * 1.01).sin(), 0.0, -(a * 1.01).cos());
        assert!(colpisce_sfera(DVec3::ZERO, dentro, centro, 0.5));
        assert!(!colpisce_sfera(DVec3::ZERO, fuori, centro, 0.5));
        assert!(!colpisce_sfera(DVec3::ZERO, DVec3::Z, centro, 0.5));
    }
}
