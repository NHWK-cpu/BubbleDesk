//! Simulasi fisika bubble (spec §10).
//!
//! Semi-implicit Euler, dt tetap 1/120 s, penempatan spiral, sleep.
//! Tidak ada alokasi heap di jalur update (buffer body pra-alokasi).

use crate::config::{Bubbles as BubblesCfg, Physics as PhysicsCfg};

/// Konstanta fisika (spec §10).
/// Ukuran sel grid = 2 × r_max.
const DT: f32 = 1.0 / 120.0;
const MAX_SUBSTEPS: u32 = 4;

/// Ambang tidur: kecepatan maksimum di bawah nilai ini selama 500 ms → sleep.
const SLEEP_VEL_THRESHOLD: f32 = 2.0; // px/s

/// Ambang kecepatan tumbukan agar restitusi aktif; di bawah nilai ini kontak bersifat inelastis.
const RESTITUTION_SPEED_THRESHOLD: f32 = 15.0; // px/s

/// Margin dari tepi area kerja (spec §10 aturan 4).
const WORK_MARGIN: f32 = 8.0;

/// Kekuatan pegas pemusat (1/s²) agar bubble merapat kembali dan menutup celah kosong
/// saat ada item dihapus, dengan batas akselerasi maksimum agar tepi luar tidak menekan terlalu keras.
const HOME_SPRING_K: f32 = 0.45;
const MAX_HOME_ACCEL: f32 = 110.0; // px/s²

/// Kekuatan pegas untuk bubble yang di-pin oleh pengguna (spec §10 aturan 1).
const PIN_SPRING_K: f32 = 2.0;
const MAX_PIN_ACCEL: f32 = 2_000.0; // px/s²

/// Konstanta pegas & redaman kritis saat bubble di-drag mengikuti kursor (spec §10 aturan 6).
const DRAG_SPRING_K: f32 = 220.0;
const DRAG_DAMPING_C: f32 = 28.0;
const MAX_DRAG_ACCEL: f32 = 45_000.0;
const MAX_THROW_VEL: f32 = 900.0;

/// Koefisien gesekan Coulomb antar-bubble dan dinding (`|jt| <= mu * jn`).
const CONTACT_COULOMB_MU: f32 = 0.35;

/// Impuls pemecah simetri lengkungan saat bubble dihapus agar tetangga terdekat
/// langsung mengisi celah di tengah.
const ARCH_BREAK_IMPULSE: f32 = 240.0;

/// Satu badan fisika.
#[derive(Debug, Clone)]
pub struct Body {
    pub pos: [f32; 2],
    pub vel: [f32; 2],
    /// Radius fisika (tidak termasuk gap).
    pub radius: f32,
    pub mass: f32, // = radius²
    pub pinned: bool,
    pub pinned_pos: Option<[f32; 2]>,
    pub dragging: bool,
    pub drag_target: Option<[f32; 2]>,
    /// Radius target saat ini (bisa berubah saat skor berubah).
    pub radius_target: f32,
    /// Waktu terakhir radius target berubah (untuk sleep).
    pub radius_stable_ms: f32,
    /// ID item yang diwakili body ini.
    pub item_id: String,
}

impl Body {
    pub fn new(pos: [f32; 2], radius: f32, item_id: String) -> Self {
        Self {
            pos,
            vel: [0.0, 0.0],
            radius,
            mass: radius * radius,
            pinned: false,
            pinned_pos: None,
            dragging: false,
            drag_target: None,
            radius_target: radius,
            radius_stable_ms: 0.0,
            item_id,
        }
    }
}

/// State simulasi fisika.
pub struct PhysicsWorld {
    pub bodies: Vec<Body>,
    /// Akumulator waktu (sisa dt yang belum disimulasikan).
    accumulator: f32,
    /// Total waktu tidur (kecepatan rendah, ms).
    sleep_timer_ms: f32,
    /// Apakah dunia sedang tidur.
    pub sleeping: bool,
    /// Lebar dan tinggi area layar (px).
    pub area_w: f32,
    pub area_h: f32,
    /// Batas area kerja (`rcWork` dikurangi taskbar) dalam koordinat monitor lokal.
    pub work_min: [f32; 2],
    pub work_max: [f32; 2],
}

impl PhysicsWorld {
    pub fn new(area_w: f32, area_h: f32) -> Self {
        Self {
            bodies: Vec::new(),
            accumulator: 0.0,
            sleep_timer_ms: 0.0,
            sleeping: false,
            area_w,
            area_h,
            work_min: [0.0, 0.0],
            work_max: [area_w.max(1.0), area_h.max(1.0)],
        }
    }

    /// Memperbarui batas area kerja (bebas taskbar) dan meng-clamp posisi pin bila perlu.
    pub fn set_work_bounds(&mut self, work_min: [f32; 2], work_max: [f32; 2]) {
        self.work_min = work_min;
        self.work_max = [
            work_max[0].max(work_min[0] + 32.0),
            work_max[1].max(work_min[1] + 32.0),
        ];
        for b in &mut self.bodies {
            if let Some(pin) = b.pinned_pos {
                b.pinned_pos = Some(clamp_point(pin, b.radius, self.work_min, self.work_max));
            }
        }
    }

    /// Memastikan titik `pos` berada dalam batas kerja untuk radius `radius`.
    pub fn clamp_pos(&self, pos: [f32; 2], radius: f32) -> [f32; 2] {
        clamp_point(pos, radius, self.work_min, self.work_max)
    }

    /// Menggerakkan body dalam mode drag mengikuti target kursor.
    pub fn set_dragging(&mut self, item_id: &str, target: [f32; 2]) {
        if let Some(b) = self.bodies.iter_mut().find(|b| b.item_id == item_id) {
            let clamped = clamp_point(target, b.radius, self.work_min, self.work_max);
            b.dragging = true;
            b.drag_target = Some(clamped);
            self.wake();
        }
    }

    /// Mengakhiri drag: membatasi kecepatan lempar, mem-pin posisi saat dilepas,
    /// dan mengembalikan posisi pin piksel yang telah di-clamp.
    pub fn end_drag(&mut self, item_id: &str) -> Option<[f32; 2]> {
        let mut out_pos = None;
        if let Some(b) = self.bodies.iter_mut().find(|b| b.item_id == item_id) {
            b.dragging = false;
            b.drag_target = None;
            let speed = b.vel[0].hypot(b.vel[1]);
            if speed > MAX_THROW_VEL {
                let s = MAX_THROW_VEL / speed;
                b.vel[0] *= s;
                b.vel[1] *= s;
            }
            let clamped = clamp_point(b.pos, b.radius, self.work_min, self.work_max);
            b.pos = clamped;
            b.pinned = true;
            b.pinned_pos = Some(clamped);
            out_pos = Some(clamped);
        }
        if out_pos.is_some() {
            self.wake();
        }
        out_pos
    }

    /// Melepas status pin sebuah body agar kembali ditarik lembut ke pusat klaster.
    pub fn unpin(&mut self, item_id: &str) {
        if let Some(b) = self.bodies.iter_mut().find(|b| b.item_id == item_id) {
            b.pinned = false;
            b.pinned_pos = None;
            self.wake();
        }
    }

    /// Memasukkan satu body baru; membatalkan sleep.
    pub fn add_body(&mut self, body: Body) {
        self.bodies.push(body);
        self.wake();
    }

    /// Menghapus body berdasarkan item_id; menarik tetangga terdekat ke arahnya
    /// agar tidak terbentuk lengkungan simetris (bolong di tengah) lalu membangunkan world.
    pub fn remove_body(&mut self, item_id: &str) {
        let removed_pos = self
            .bodies
            .iter()
            .find(|b| b.item_id == item_id)
            .map(|b| b.pos);
        self.bodies.retain(|b| b.item_id != item_id);
        if let Some(rpos) = removed_pos {
            if let Some(nearest) = self
                .bodies
                .iter_mut()
                .filter(|b| !b.pinned && !b.dragging)
                .min_by(|a, b| {
                    let da = (a.pos[0] - rpos[0]).powi(2) + (a.pos[1] - rpos[1]).powi(2);
                    let db = (b.pos[0] - rpos[0]).powi(2) + (b.pos[1] - rpos[1]).powi(2);
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
            {
                let dx = rpos[0] - nearest.pos[0];
                let dy = rpos[1] - nearest.pos[1];
                let dist = dx.hypot(dy);
                if dist > 1e-3 {
                    nearest.vel[0] += (dx / dist) * ARCH_BREAK_IMPULSE;
                    nearest.vel[1] += (dy / dist) * ARCH_BREAK_IMPULSE;
                }
            }
        }
        self.wake();
    }

    /// Membangunkan simulasi.
    pub fn wake(&mut self) {
        self.sleeping = false;
        self.sleep_timer_ms = 0.0;
    }

    /// Langkah simulasi dengan `frame_dt` detik nyata.
    /// Menggunakan akumulator agar dt tetap 1/120 s.
    pub fn step(&mut self, frame_dt: f32, cfg_bub: &BubblesCfg, cfg_phy: &PhysicsCfg) {
        if self.sleeping {
            return;
        }
        self.accumulator += frame_dt.min(0.1); // cap besar
        let mut substeps = 0u32;
        while self.accumulator >= DT && substeps < MAX_SUBSTEPS {
            self.accumulator -= DT;
            self.integrate_one(cfg_bub, cfg_phy);
            substeps += 1;
        }

        // Periksa kondisi tidur (spec §10)
        let any_dragging = self.bodies.iter().any(|b| b.dragging);
        let max_speed = self
            .bodies
            .iter()
            .map(|b| b.vel[0].abs().max(b.vel[1].abs()))
            .fold(0.0_f32, f32::max);
        let all_stable = self.bodies.iter().all(|b| b.radius_stable_ms > 500.0);

        if !any_dragging && max_speed < SLEEP_VEL_THRESHOLD && all_stable {
            self.sleep_timer_ms += frame_dt * 1000.0;
            if self.sleep_timer_ms > 500.0 {
                self.sleeping = true;
            }
        } else {
            self.sleep_timer_ms = 0.0;
        }
    }

    fn integrate_one(&mut self, cfg_bub: &BubblesCfg, cfg_phy: &PhysicsCfg) {
        let gap = cfg_bub.gap as f32;
        let margin = WORK_MARGIN;
        let damping_factor = (-cfg_phy.damping as f32 * DT).exp();
        let w_min = self.work_min;
        let w_max = self.work_max;
        let work_w = (w_max[0] - w_min[0]).max(1.0);
        let work_h = (w_max[1] - w_min[1]).max(1.0);

        // 1. Gaya home / pin / drag spring
        let cx = (w_min[0] + w_max[0]) * 0.5;
        let cy = (w_min[1] + w_max[1]) * 0.5;
        let aspect = (work_w / work_h).clamp(0.5, 3.0);
        let kx = HOME_SPRING_K / aspect;
        let ky = HOME_SPRING_K * aspect;
        for b in &mut self.bodies {
            if b.dragging {
                if let Some(target) = b.drag_target {
                    let mut ax = DRAG_SPRING_K * (target[0] - b.pos[0]) - DRAG_DAMPING_C * b.vel[0];
                    let mut ay = DRAG_SPRING_K * (target[1] - b.pos[1]) - DRAG_DAMPING_C * b.vel[1];
                    let a_mag = ax.hypot(ay);
                    if a_mag > MAX_DRAG_ACCEL {
                        let s = MAX_DRAG_ACCEL / a_mag;
                        ax *= s;
                        ay *= s;
                    }
                    b.vel[0] += ax * DT;
                    b.vel[1] += ay * DT;
                }
                continue;
            }
            if b.pinned {
                if let Some(pin) = b.pinned_pos {
                    let mut ax = PIN_SPRING_K * (pin[0] - b.pos[0]);
                    let mut ay = PIN_SPRING_K * (pin[1] - b.pos[1]);
                    let a_mag = ax.hypot(ay);
                    if a_mag > MAX_PIN_ACCEL {
                        let s = MAX_PIN_ACCEL / a_mag;
                        ax *= s;
                        ay *= s;
                    }
                    b.vel[0] += ax * DT;
                    b.vel[1] += ay * DT;
                }
                continue;
            }
            let mut ax = kx * (cx - b.pos[0]);
            let mut ay = ky * (cy - b.pos[1]);
            let a_mag = ax.hypot(ay);
            if a_mag > MAX_HOME_ACCEL {
                let s = MAX_HOME_ACCEL / a_mag;
                ax *= s;
                ay *= s;
            }
            b.vel[0] += ax * DT;
            b.vel[1] += ay * DT;
        }

        // 2. Damping
        for b in &mut self.bodies {
            b.vel[0] *= damping_factor;
            b.vel[1] *= damping_factor;
        }

        // 3. Resolusi kecepatan kontak (14 iterasi Gauss-Seidel) sebelum integrasi posisi
        // agar kontak diam tidak terus mendorong masuk ke tetangganya.
        let rest_body_cfg = cfg_phy.restitution_body as f32;
        let rest_wall_cfg = cfg_phy.restitution_wall as f32;
        let contact_slop = 1.0_f32;
        let n = self.bodies.len();
        for iter in 0..14 {
            for i in 0..n {
                for j in (i + 1)..n {
                    let min_dist =
                        self.bodies[i].radius + self.bodies[j].radius + gap + contact_slop;
                    let dx = self.bodies[j].pos[0] - self.bodies[i].pos[0];
                    let dy = self.bodies[j].pos[1] - self.bodies[i].pos[1];
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq >= min_dist * min_dist || dist_sq < 1e-6 {
                        continue;
                    }
                    let dist = dist_sq.sqrt();
                    let nx = dx / dist;
                    let ny = dy / dist;
                    let mi = self.bodies[i].mass;
                    let mj = self.bodies[j].mass;
                    let inv_mass_sum = 1.0 / mi + 1.0 / mj;

                    let rel_vx = self.bodies[j].vel[0] - self.bodies[i].vel[0];
                    let rel_vy = self.bodies[j].vel[1] - self.bodies[i].vel[1];
                    let vn = rel_vx * nx + rel_vy * ny;
                    if vn < 0.0 {
                        let rest = if iter == 0 && -vn > RESTITUTION_SPEED_THRESHOLD {
                            rest_body_cfg
                        } else {
                            0.0
                        };
                        let jn = -(1.0 + rest) * vn / inv_mass_sum;
                        self.bodies[i].vel[0] -= jn / mi * nx;
                        self.bodies[i].vel[1] -= jn / mi * ny;
                        self.bodies[j].vel[0] += jn / mj * nx;
                        self.bodies[j].vel[1] += jn / mj * ny;

                        let rel_vx_after = self.bodies[j].vel[0] - self.bodies[i].vel[0];
                        let rel_vy_after = self.bodies[j].vel[1] - self.bodies[i].vel[1];
                        let tx = -ny;
                        let ty = nx;
                        let vt = rel_vx_after * tx + rel_vy_after * ty;
                        let max_jt = CONTACT_COULOMB_MU * jn;
                        let jt = (-vt / inv_mass_sum).clamp(-max_jt, max_jt);
                        self.bodies[i].vel[0] -= jt / mi * tx;
                        self.bodies[i].vel[1] -= jt / mi * ty;
                        self.bodies[j].vel[0] += jt / mj * tx;
                        self.bodies[j].vel[1] += jt / mj * ty;
                    }
                }
            }

            // Batasi komponen kecepatan yang menembus dinding saat sudah menempel dinding
            for b in &mut self.bodies {
                let left = w_min[0] + margin + b.radius;
                let right = (w_max[0] - margin - b.radius).max(left);
                let top = w_min[1] + margin + b.radius;
                let bottom = (w_max[1] - margin - b.radius).max(top);
                if b.pos[0] <= left + contact_slop && b.vel[0] < 0.0 {
                    let vn = -b.vel[0];
                    b.vel[0] = if iter == 0 && vn > RESTITUTION_SPEED_THRESHOLD {
                        vn * rest_wall_cfg
                    } else {
                        0.0
                    };
                    let max_vt = CONTACT_COULOMB_MU * vn;
                    b.vel[1] -= b.vel[1].clamp(-max_vt, max_vt);
                } else if b.pos[0] >= right - contact_slop && b.vel[0] > 0.0 {
                    let vn = b.vel[0];
                    b.vel[0] = if iter == 0 && vn > RESTITUTION_SPEED_THRESHOLD {
                        -vn * rest_wall_cfg
                    } else {
                        0.0
                    };
                    let max_vt = CONTACT_COULOMB_MU * vn;
                    b.vel[1] -= b.vel[1].clamp(-max_vt, max_vt);
                }
                if b.pos[1] <= top + contact_slop && b.vel[1] < 0.0 {
                    let vn = -b.vel[1];
                    b.vel[1] = if iter == 0 && vn > RESTITUTION_SPEED_THRESHOLD {
                        vn * rest_wall_cfg
                    } else {
                        0.0
                    };
                    let max_vt = CONTACT_COULOMB_MU * vn;
                    b.vel[0] -= b.vel[0].clamp(-max_vt, max_vt);
                } else if b.pos[1] >= bottom - contact_slop && b.vel[1] > 0.0 {
                    let vn = b.vel[1];
                    b.vel[1] = if iter == 0 && vn > RESTITUTION_SPEED_THRESHOLD {
                        -vn * rest_wall_cfg
                    } else {
                        0.0
                    };
                    let max_vt = CONTACT_COULOMB_MU * vn;
                    b.vel[0] -= b.vel[0].clamp(-max_vt, max_vt);
                }
            }
        }

        // 4. Integrasi posisi
        for b in &mut self.bodies {
            b.pos[0] += b.vel[0] * DT;
            b.pos[1] += b.vel[1] * DT;
        }

        // 5. Koreksi posisi batas dinding + lingkaran–lingkaran bersama (8 iterasi)
        for _ in 0..8 {
            for i in 0..n {
                for j in (i + 1)..n {
                    let min_dist = self.bodies[i].radius + self.bodies[j].radius + gap;
                    let dx = self.bodies[j].pos[0] - self.bodies[i].pos[0];
                    let dy = self.bodies[j].pos[1] - self.bodies[i].pos[1];
                    let dist_sq = dx * dx + dy * dy;
                    if dist_sq >= min_dist * min_dist || dist_sq < 1e-6 {
                        continue;
                    }
                    let dist = dist_sq.sqrt();
                    let overlap = min_dist - dist;
                    let nx = dx / dist;
                    let ny = dy / dist;

                    let mi = self.bodies[i].mass;
                    let mj = self.bodies[j].mass;
                    let total = mi + mj;
                    let fi = mj / total;
                    let fj = mi / total;

                    self.bodies[i].pos[0] -= nx * overlap * fi;
                    self.bodies[i].pos[1] -= ny * overlap * fi;
                    self.bodies[j].pos[0] += nx * overlap * fj;
                    self.bodies[j].pos[1] += ny * overlap * fj;
                }
            }

            // Proyeksi batas area kerja (spec §10 aturan 4)
            for b in &mut self.bodies {
                let left = w_min[0] + margin + b.radius;
                let right = (w_max[0] - margin - b.radius).max(left);
                let top = w_min[1] + margin + b.radius;
                let bottom = (w_max[1] - margin - b.radius).max(top);
                b.pos[0] = b.pos[0].clamp(left, right);
                b.pos[1] = b.pos[1].clamp(top, bottom);
            }
        }

        // 6. Update radius_stable_ms
        for b in &mut self.bodies {
            if (b.radius - b.radius_target).abs() < 0.5 {
                b.radius_stable_ms += DT * 1000.0;
            } else {
                b.radius_stable_ms = 0.0;
                // Smooth radius ke target (spec §9: τ=0.35s)
                let tau = 0.35_f32;
                b.radius += (b.radius_target - b.radius) * (1.0 - (-DT / tau).exp());
                b.mass = b.radius * b.radius;
            }
        }
    }
}

#[inline]
fn clamp_point(pos: [f32; 2], radius: f32, work_min: [f32; 2], work_max: [f32; 2]) -> [f32; 2] {
    let left = work_min[0] + WORK_MARGIN + radius;
    let right = (work_max[0] - WORK_MARGIN - radius).max(left);
    let top = work_min[1] + WORK_MARGIN + radius;
    let bottom = (work_max[1] - WORK_MARGIN - radius).max(top);
    [pos[0].clamp(left, right), pos[1].clamp(top, bottom)]
}

/// Menempatkan `n` body dalam spiral dari pusat area, item terbesar dulu.
/// Seed deterministik agar posisi dapat diuji.
/// Mengembalikan posisi awal `[f32; 2]` untuk setiap body, berurutan sesuai
/// indeks setelah sorting by radius descending.
#[cfg_attr(not(test), allow(dead_code))]
pub fn initial_positions(radii: &[f32], area_w: f32, area_h: f32, gap: f32) -> Vec<[f32; 2]> {
    initial_positions_in_rect(radii, [0.0, 0.0], [area_w, area_h], gap)
}

/// Sama seperti `initial_positions`, tetapi dibatasi ke persegi area kerja `[work_min, work_max]`.
pub fn initial_positions_in_rect(
    radii: &[f32],
    work_min: [f32; 2],
    work_max: [f32; 2],
    gap: f32,
) -> Vec<[f32; 2]> {
    let n = radii.len();
    if n == 0 {
        return Vec::new();
    }
    let area_w = (work_max[0] - work_min[0]).max(1.0);
    let area_h = (work_max[1] - work_min[1]).max(1.0);
    let cx = (work_min[0] + work_max[0]) * 0.5;
    let cy = (work_min[1] + work_max[1]) * 0.5;
    // Spiral Vogel elips mengikuti rasio aspek layar agar mengisi seluruh layar.
    let golden_angle: f32 = 2.399_963;
    let aspect = (area_w / area_h).clamp(0.5, 3.0);
    let sx = aspect.sqrt();
    let sy = 1.0 / sx;
    let mut positions = Vec::with_capacity(n);
    let mut angle: f32 = 0.0;
    let mut area_sum: f32 = 0.0;

    for (i, &r) in radii.iter().enumerate() {
        let r_eff = r + gap * 0.5;
        if i == 0 {
            positions.push([cx, cy]);
            area_sum = std::f32::consts::PI * r_eff * r_eff;
        } else {
            // Tambah luas sel mendekati kerapatan kemasan heksagonal (2*sqrt(3) ≈ 3.464)
            area_sum += 3.55 * r_eff * r_eff;
            let ring_r = (area_sum / std::f32::consts::PI)
                .sqrt()
                .max(radii[0] + r + gap);
            let p = clamp_point(
                [
                    cx + ring_r * sx * angle.cos(),
                    cy + ring_r * sy * angle.sin(),
                ],
                r,
                work_min,
                work_max,
            );
            positions.push(p);
            angle += golden_angle;
        }
    }
    positions
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{Bubbles, Physics};

    fn cfg() -> (Bubbles, Physics) {
        (Bubbles::default(), Physics::default())
    }

    #[test]
    fn tidak_overlap_setelah_settle() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(1600.0, 900.0);
        // 10 body besar, posisikan tumpang tindih di pusat
        for i in 0..10 {
            let b = Body::new([800.0 + i as f32, 450.0], 60.0, format!("b{i}"));
            world.add_body(b);
        }
        // Simulasikan 3 detik
        for _ in 0..180 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
        }
        let gap = cfg_b.gap as f32;
        let n = world.bodies.len();
        for i in 0..n {
            for j in (i + 1)..n {
                let dx = world.bodies[i].pos[0] - world.bodies[j].pos[0];
                let dy = world.bodies[i].pos[1] - world.bodies[j].pos[1];
                let dist = (dx * dx + dy * dy).sqrt();
                let min_d = world.bodies[i].radius + world.bodies[j].radius + gap;
                // Toleransi 1px karena iterasi koreksi tidak sempurna
                assert!(
                    dist >= min_d - 1.0,
                    "overlap: dist={dist:.1} min={min_d:.1} bodies {i} dan {j}"
                );
            }
        }
    }

    #[test]
    fn body_tidak_keluar_batas() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(800.0, 600.0);
        for i in 0..20 {
            let r = 30.0 + (i as f32) * 2.0;
            let b = Body::new([400.0, 300.0], r, format!("b{i}"));
            world.add_body(b);
        }
        for _ in 0..300 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
        }
        let margin = 8.0_f32;
        for b in &world.bodies {
            assert!(b.pos[0] - b.radius >= margin - 2.0, "keluar kiri");
            assert!(b.pos[0] + b.radius <= 800.0 - margin + 2.0, "keluar kanan");
            assert!(b.pos[1] - b.radius >= margin - 2.0, "keluar atas");
            assert!(b.pos[1] + b.radius <= 600.0 - margin + 2.0, "keluar bawah");
        }
    }

    #[test]
    fn sleep_tercapai_saat_diam() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(800.0, 600.0);
        // Satu body kecil di tengah
        let mut b = Body::new([400.0, 300.0], 30.0, "single".to_owned());
        b.radius_stable_ms = 600.0; // sudah stabil
        world.add_body(b);
        // Simulasikan sampai sleep
        for _ in 0..600 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
            if world.sleeping {
                break;
            }
        }
        assert!(world.sleeping, "dunia harus tidur setelah body diam");
    }

    #[test]
    fn sleep_tercapai_untuk_banyak_body() {
        let (cfg_b, cfg_p) = cfg();
        let radii = vec![90.0_f32; 31];
        let positions = initial_positions(&radii, 1600.0, 900.0, cfg_b.gap as f32);
        let mut world = PhysicsWorld::new(1600.0, 900.0);
        for (i, (&r, &pos)) in radii.iter().zip(positions.iter()).enumerate() {
            world.add_body(Body::new(pos, r, format!("b{i}")));
        }
        // Simulasikan maks 5 detik (300 frame @ 60Hz)
        for _ in 0..300 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
            if world.sleeping {
                break;
            }
        }
        let max_speed = world
            .bodies
            .iter()
            .map(|b| b.vel[0].abs().max(b.vel[1].abs()))
            .fold(0.0_f32, f32::max);
        let min_stable = world
            .bodies
            .iter()
            .map(|b| b.radius_stable_ms)
            .fold(f32::INFINITY, f32::min);
        assert!(
            world.sleeping,
            "31 body harus mencapai sleep dalam 5 detik (max_speed={max_speed:.3}, min_stable={min_stable:.1}, sleep_timer={:.1})",
            world.sleep_timer_ms
        );
    }

    #[test]
    fn celah_terhapus_menutup_tanpa_bolong() {
        let (cfg_b, cfg_p) = cfg();
        let radii = vec![90.0_f32; 31];
        let positions = initial_positions(&radii, 1600.0, 900.0, cfg_b.gap as f32);
        let mut world = PhysicsWorld::new(1600.0, 900.0);
        for (i, (&r, &pos)) in radii.iter().zip(positions.iter()).enumerate() {
            world.add_body(Body::new(pos, r, format!("b{i}")));
        }
        for _ in 0..300 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
        }

        // Hapus body paling dekat dengan pusat (800, 450)
        world.remove_body("b0");
        for b in &mut world.bodies {
            b.radius_target = 91.7;
        }
        world.wake();

        for _ in 0..540 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
            if world.sleeping {
                break;
            }
        }

        // Jarak dari pusat layar ke pusat bubble terdekat harus < 1.35 * radius (tidak ada bolong seukuran 1 bubble)
        let nearest_dist = world
            .bodies
            .iter()
            .map(|b| ((b.pos[0] - 800.0).powi(2) + (b.pos[1] - 450.0).powi(2)).sqrt())
            .fold(f32::INFINITY, f32::min);
        assert!(
            nearest_dist < 91.7 * 1.35,
            "celah tengah tidak menutup: nearest_dist={nearest_dist:.1}"
        );
        let mut max_speed = 0.0_f32;
        let mut fast_info = String::new();
        for b in &world.bodies {
            let sp = b.vel[0].abs().max(b.vel[1].abs());
            if sp > max_speed {
                max_speed = sp;
                fast_info = format!(
                    "{}@({:.1},{:.1}) v=({:.2},{:.2})",
                    b.item_id, b.pos[0], b.pos[1], b.vel[0], b.vel[1]
                );
            }
        }
        let min_stable = world
            .bodies
            .iter()
            .map(|b| b.radius_stable_ms)
            .fold(f32::INFINITY, f32::min);
        assert!(
            world.sleeping,
            "world harus kembali tidur setelah menutup celah (max_speed={max_speed:.3} [{fast_info}], min_stable={min_stable:.1}, sleep_timer={:.1})",
            world.sleep_timer_ms
        );
    }

    #[test]
    fn posisi_initial_deterministik() {
        let radii = vec![60.0, 50.0, 40.0, 30.0];
        let p1 = initial_positions(&radii, 1600.0, 900.0, 6.0);
        let p2 = initial_positions(&radii, 1600.0, 900.0, 6.0);
        assert_eq!(p1.len(), p2.len());
        for (a, b) in p1.iter().zip(p2.iter()) {
            assert_eq!(a, b);
        }
    }

    #[test]
    fn fisika_tidak_nan_inf() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(1920.0, 1080.0);
        for i in 0..50 {
            let r = 28.0 + (i % 5) as f32 * 15.0;
            world.add_body(Body::new([960.0, 540.0], r, format!("b{i}")));
        }
        for _ in 0..600 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
        }
        for b in &world.bodies {
            assert!(b.pos[0].is_finite(), "pos.x NaN/inf");
            assert!(b.pos[1].is_finite(), "pos.y NaN/inf");
            assert!(b.vel[0].is_finite(), "vel.x NaN/inf");
            assert!(b.vel[1].is_finite(), "vel.y NaN/inf");
        }
    }

    #[test]
    fn pinned_body_menuju_pinned_pos_dan_sleep() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(1600.0, 900.0);
        let mut b = Body::new([400.0, 300.0], 40.0, "pin".to_owned());
        b.pinned = true;
        b.pinned_pos = Some([1200.0, 650.0]);
        b.radius_stable_ms = 600.0;
        world.add_body(b);

        for _ in 0..900 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
            if world.sleeping {
                break;
            }
        }
        let pos = world.bodies[0].pos;
        assert!(
            (pos[0] - 1200.0).abs() < 3.0 && (pos[1] - 650.0).abs() < 3.0,
            "body pin harus menetap dekat pinned_pos, pos={pos:?}"
        );
        assert!(world.sleeping, "world harus tidur setelah pin settle");
    }

    #[test]
    fn drag_mengikuti_target_dalam_batas_dan_unpin() {
        let (cfg_b, cfg_p) = cfg();
        let mut world = PhysicsWorld::new(1000.0, 700.0);
        world.set_work_bounds([0.0, 0.0], [1000.0, 650.0]);
        let b = Body::new([500.0, 300.0], 40.0, "d1".to_owned());
        world.add_body(b);

        world.set_dragging("d1", [990.0, 690.0]);
        for _ in 0..45 {
            world.step(1.0 / 60.0, &cfg_b, &cfg_p);
        }
        assert!(!world.sleeping, "tidak boleh sleep saat sedang drag");
        let pos = world.bodies[0].pos;
        // Harus tetap di dalam work_max dikurangi margin+radius (650 - 8 - 40 = 602)
        assert!(pos[0] <= 1000.0 - 48.0 + 0.5);
        assert!(pos[1] <= 650.0 - 48.0 + 0.5);

        let pinned_at = world.end_drag("d1").expect("body ada");
        assert!(world.bodies[0].pinned);
        assert_eq!(world.bodies[0].pinned_pos, Some(pinned_at));

        world.unpin("d1");
        assert!(!world.bodies[0].pinned);
        assert!(world.bodies[0].pinned_pos.is_none());
    }
}
