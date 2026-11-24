//! Skor dan ukuran bubble (spec §9).
//!
//! Decay setengah-usia, pemetaan logaritmik ke radius, dan koreksi anti-sesak.

use crate::config::Bubbles as BubblesCfg;

/// Menghitung skor yang telah membusuk (decayed) pada waktu `now_secs`.
pub fn decayed_score(score: f64, score_t0: i64, now_secs: i64, half_life_days: f64) -> f64 {
    let half_life_secs = half_life_days * 86_400.0;
    let elapsed = (now_secs - score_t0).max(0) as f64;
    score * 0.5_f64.powf(elapsed / half_life_secs)
}

/// Menambah skor setelah peluncuran berhasil.
/// Mengembalikan `(score_baru, t0_baru)`.
// ponytail: dipakai M4 saat user klik item
#[allow(dead_code)]
pub fn add_launch(score: f64, score_t0: i64, now_secs: i64, half_life_days: f64) -> (f64, i64) {
    let d = decayed_score(score, score_t0, now_secs, half_life_days);
    (d + 1.0, now_secs)
}

/// Memetakan skor ke radius display dalam piksel (spec §9).
pub fn score_to_radius(score: f64, score_t0: i64, now_secs: i64, cfg: &BubblesCfg) -> f32 {
    let d = decayed_score(score, score_t0, now_secs, cfg.score_half_life_days);
    let x = (((1.0 + d).ln()) / (1.0 + cfg.score_cap).ln()).clamp(0.0, 1.0);
    (cfg.r_min + (cfg.r_max - cfg.r_min) * x) as f32
}

/// Faktor skala adaptif agar kumpulan bubble memenuhi anggaran layar (`budget`)
/// dan otomatis mengecil saat item bertambah atau membesar saat item berkurang.
///
/// Mengembalikan `(k, max_display_radius)`. Setiap radius dasar dikalikan `k`
/// lalu di-clamp ke paling besar `max_display_radius`.
pub fn adaptive_scale_factor(
    radii: &[f32],
    work_area_px: f64,
    area_w: f32,
    area_h: f32,
    budget: f64,
    r_max_cfg: f64,
) -> (f32, f32) {
    let total_area: f64 = radii
        .iter()
        .map(|&r| std::f64::consts::PI * r as f64 * r as f64)
        .sum();
    let max_display_r = ((area_w.min(area_h) * 0.22) as f64).max(r_max_cfg) as f32;
    if total_area <= 0.0 || work_area_px <= 0.0 {
        return (1.0, max_display_r);
    }
    let ratio = total_area / work_area_px;
    let k = ((budget / ratio).sqrt() as f32).max(0.5);
    (k, max_display_r)
}

/// Faktor koreksi anti-sesak (spec §9).
/// Mengembalikan `k` untuk dikalikan ke semua radius.
/// `k` selalu dalam rentang `[0.5, 1.0]`.
#[allow(dead_code)]
pub fn crowd_factor(radii: &[f32], work_area_px: f64, budget: f64) -> f32 {
    let total_area: f64 = radii
        .iter()
        .map(|&r| std::f64::consts::PI * r as f64 * r as f64)
        .sum();
    if total_area <= 0.0 || work_area_px <= 0.0 {
        return 1.0;
    }
    let ratio = total_area / work_area_px;
    if ratio <= budget {
        1.0
    } else {
        ((budget / ratio).sqrt() as f32).max(0.5)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Bubbles;

    fn default_cfg() -> Bubbles {
        Bubbles {
            r_min: 28.0,
            r_max: 110.0,
            score_half_life_days: 30.0,
            score_cap: 40.0,
            min_hit_radius: 20.0,
            label_always_min_radius: 60.0,
            gap: 6.0,
            crowd_budget: 0.55,
        }
    }

    #[test]
    fn skor_baru_lebih_besar() {
        let cfg = default_cfg();
        let now = 1_700_000_000i64;
        let r0 = score_to_radius(0.0, now, now, &cfg);
        let r1 = score_to_radius(1.0, now, now, &cfg);
        let r10 = score_to_radius(10.0, now, now, &cfg);
        // Monotonik
        assert!(r0 <= r1);
        assert!(r1 <= r10);
    }

    #[test]
    fn decay_mengurangi_skor() {
        let now = 1_700_000_000i64;
        let hl_secs = 30 * 86_400;
        let d = decayed_score(1.0, now - hl_secs, now, 30.0);
        // Setelah 1 half-life, skor ≈ 0.5
        assert!((d - 0.5).abs() < 1e-9);
    }

    #[test]
    fn skor_clamp_di_batas() {
        let cfg = default_cfg();
        let now = 1_700_000_000i64;
        // Skor sangat besar tidak melebihi r_max
        let r = score_to_radius(999.0, now, now, &cfg);
        assert!(r <= cfg.r_max as f32 + 1.0);
    }

    #[test]
    fn crowd_factor_tidak_koreksi_jika_masuk_budget() {
        // 4 bubble r=28 di layar besar
        let radii = vec![28.0f32; 4];
        let k = crowd_factor(&radii, 1_000_000.0, 0.55);
        assert!((k - 1.0).abs() < 1e-6);
    }

    #[test]
    fn crowd_factor_minumum_setengah() {
        // 1000 bubble r=200 di layar kecil → pasti sangat padat
        let radii = vec![200.0f32; 1000];
        let k = crowd_factor(&radii, 100_000.0, 0.55);
        assert!(k >= 0.5);
    }

    #[test]
    fn adaptive_scale_mengisi_layar_dan_mengecil_saat_item_bertambah() {
        let radii_31 = vec![28.0f32; 31];
        let (k31, max_r) =
            adaptive_scale_factor(&radii_31, 1600.0 * 900.0, 1600.0, 900.0, 0.55, 110.0);
        let r31 = (28.0 * k31).min(max_r);
        assert!(r31 > 80.0 && r31 < 105.0, "r31={r31}");

        // Bertambah jadi 60 item → semua bubble mengecil
        let radii_60 = vec![28.0f32; 60];
        let (k60, _) = adaptive_scale_factor(&radii_60, 1600.0 * 900.0, 1600.0, 900.0, 0.55, 110.0);
        let r60 = (28.0 * k60).min(max_r);
        assert!(r60 < r31, "r60={r60} harus lebih kecil dari r31={r31}");

        // Sangat sedikit item (2 item) → ter-clamp di max_r agar tidak keluar layar
        let radii_2 = vec![28.0f32; 2];
        let (k2, max_r2) =
            adaptive_scale_factor(&radii_2, 1600.0 * 900.0, 1600.0, 900.0, 0.55, 110.0);
        let r2 = (28.0 * k2).min(max_r2);
        assert!((r2 - max_r2).abs() < 1e-4);
    }
}
