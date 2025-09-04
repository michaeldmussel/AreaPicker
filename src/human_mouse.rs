use enigo::{MouseControllable, MouseButton};
use rand::{Rng, SeedableRng};
use rand::rngs::StdRng;
use std::{thread, time::Duration};

#[derive(Clone, Copy, Debug)]
pub struct Bounds {
    pub min_x: i32, pub max_x: i32,
    pub min_y: i32, pub max_y: i32,
}
impl Bounds {
    pub fn clamp(&self, (x, y): (i32, i32)) -> (i32, i32) {
        (x.clamp(self.min_x, self.max_x), y.clamp(self.min_y, self.max_y))
    }
    pub fn contains(&self, (x, y): (i32, i32)) -> bool {
        (self.min_x..=self.max_x).contains(&x) && (self.min_y..=self.max_y).contains(&y)
    }
    pub fn nearest_point(&self, (x, y): (i32, i32)) -> (i32, i32) {
        self.clamp((x, y))
    }
    pub fn width(&self) -> i32 { self.max_x - self.min_x }
    pub fn height(&self) -> i32 { self.max_y - self.min_y }
    pub fn is_valid(&self) -> bool { self.width() > 0 && self.height() > 0 }
}

#[derive(Clone, Debug)]
pub struct HumanMouseSettings {
    /// Average speed in px/sec; actual speed varies around this.
    pub avg_speed: f32,               // e.g. 1400.0
    /// Random speed variation factor (0.0–1.0). 0.25 => ±25%.
    pub speed_jitter: f32,            // e.g. 0.25
    /// Small jitter amplitude in pixels applied along the path.
    pub micro_jitter_px: f32,         // e.g. 0.6
    /// Frequency of jitter wiggles per second (randomized a bit).
    pub micro_jitter_hz: f32,         // e.g. 9.0
    /// Chance to slightly overshoot target before settling.
    pub overshoot_chance: f32,        // e.g. 0.25
    /// Max overshoot distance in px.
    pub overshoot_px: f32,            // e.g. 12.0
    /// Min & max micro-pause durations inserted mid-movement.
    pub min_pause_ms: u64,            // e.g. 15
    pub max_pause_ms: u64,            // e.g. 60
    /// Seed for reproducible tests. Use None in prod.
    pub rng_seed: Option<u64>,
}

impl Default for HumanMouseSettings {
    fn default() -> Self {
        Self {
            avg_speed: 1400.0,
            speed_jitter: 0.25,
            micro_jitter_px: 0.6,
            micro_jitter_hz: 9.0,
            overshoot_chance: 0.25,
            overshoot_px: 12.0,
            min_pause_ms: 15,
            max_pause_ms: 60,
            rng_seed: None,
        }
    }
}

/// Cosine ease-in-out (smooth velocity bell curve).
fn ease_in_out(t: f32) -> f32 {
    0.5 - 0.5 * (std::f32::consts::PI * t).cos()
}

/// Cubic Bezier interpolation
fn cubic_bezier(p0: (f32,f32), p1: (f32,f32), p2: (f32,f32), p3: (f32,f32), t: f32) -> (f32,f32) {
    let u = 1.0 - t;
    let uu = u*u;
    let tt = t*t;
    let uuu = uu*u;
    let ttt = tt*t;
    (
        uuu*p0.0 + 3.0*uu*t*p1.0 + 3.0*u*tt*p2.0 + ttt*p3.0,
        uuu*p0.1 + 3.0*uu*t*p1.1 + 3.0*u*tt*p2.1 + ttt*p3.1,
    )
}

fn len((x1,y1):(f32,f32),(x2,y2):(f32,f32)) -> f32 {
    ((x2-x1).hypot(y2-y1)).max(1.0)
}

/// Build a wiggly cubic path with control points roughly perpendicular to the segment.
fn make_bezier_with_wiggle(
    from: (i32,i32), to: (i32,i32), rng: &mut impl Rng
) -> ((f32,f32),(f32,f32),(f32,f32),(f32,f32)) {
    let p0 = (from.0 as f32, from.1 as f32);
    let p3 = (to.0 as f32, to.1 as f32);

    let dx = p3.0 - p0.0;
    let dy = p3.1 - p0.1;
    let dist = len(p0, p3);
    // Perp vector (normalized)
    let (nx, ny) = if dist > 0.0 { (-dy / dist, dx / dist) } else { (0.0, 0.0) };

    // Control point distance as a fraction of total distance
    let cdist = 0.25 * dist;
    // Random perpendicular offsets
    let amp1 = rng.gen_range(-0.12..0.12) * dist;
    let amp2 = rng.gen_range(-0.12..0.12) * dist;

    let p1 = (p0.0 + dx * 0.30 + nx * amp1, p0.1 + dy * 0.30 + ny * amp1);
    let p2 = (p0.0 + dx * 0.70 + nx * amp2, p0.1 + dy * 0.70 + ny * amp2);

    // Nudge control points slightly along the direction to reduce weird loops on short hops
    let p1 = (p1.0 + (dx / dist) * (cdist * 0.1), p1.1 + (dy / dist) * (cdist * 0.1));
    let p2 = (p2.0 - (dx / dist) * (cdist * 0.1), p2.1 - (dy / dist) * (cdist * 0.1));

    (p0, p1, p2, p3)
}

/// Optionally insert a tiny overshoot point before the true `to`.
fn maybe_overshoot(to: (i32,i32), from: (i32,i32), settings: &HumanMouseSettings, rng: &mut impl Rng) -> (i32,i32) {
    if rng.gen::<f32>() < settings.overshoot_chance {
        let dx = (to.0 - from.0) as f32;
        let dy = (to.1 - from.1) as f32;
        let d = (dx*dx + dy*dy).sqrt().max(1.0);
        let ux = dx / d;
        let uy = dy / d;
        let overshoot = rng.gen_range(0.0..settings.overshoot_px);
        return (to.0 + (ux*overshoot) as i32, to.1 + (uy*overshoot) as i32);
    }
    to
}

/// Move the mouse like a human: smooth path, velocity bell curve, jitter, pauses, optional overshoot.
pub fn human_move_and_click(
    enigo: &mut impl MouseControllable,
    mut from: (i32,i32),
    to: (i32,i32),
    bounds: Option<Bounds>,
    settings: &HumanMouseSettings,
    button: MouseButton,
) {
    let mut rng: StdRng = match settings.rng_seed {
        Some(seed) => StdRng::seed_from_u64(seed),
        None => StdRng::from_entropy(),
    };

    // If we start outside the target square, first glide to the nearest point on its edge.
    if let Some(b) = bounds {
        if !b.contains(from) {
            let entry = b.nearest_point(from);
            human_move_inner(enigo, from, entry, None, settings, &mut rng);
            from = entry;
        }
    }

    // Sometimes overshoot a bit, then settle back.
    let over = maybe_overshoot(to, from, settings, &mut rng);
    if over != to {
        human_move_inner(enigo, from, over, bounds, settings, &mut rng);
        // short settle
        thread::sleep(Duration::from_millis(20 + rng.gen_range(0..20)));
        human_move_inner(enigo, over, to, bounds, settings, &mut rng);
    } else {
        human_move_inner(enigo, from, to, bounds, settings, &mut rng);
    }

    // Human click: press + tiny hold + release with slight randomness
    enigo.mouse_down(button);
    thread::sleep(Duration::from_millis(20 + rng.gen_range(0..50)));
    enigo.mouse_up(button);
}

fn human_move_inner(
    enigo: &mut impl MouseControllable,
    from: (i32,i32),
    to: (i32,i32),
    bounds: Option<Bounds>,
    settings: &HumanMouseSettings,
    rng: &mut StdRng,
) {
    // Build a bezier-like path with curvature.
    let (p0, p1, p2, p3) = make_bezier_with_wiggle(from, to, rng);
    // Approximate duration from average speed (add jitter).
    let distance = len(p0, p3);
    let speed_variation = 1.0 + settings.speed_jitter * rng.gen_range(-1.0..1.0);
    let px_per_sec = (settings.avg_speed * speed_variation).max(200.0);
    let total_ms = ((distance / px_per_sec) * 1000.0).clamp(60.0, 1600.0) as u64;

    // Steps: one every ~8–12 ms (human OS scheduler granularity), scaled by distance.
    let step_ms = rng.gen_range(8..=12);
    let steps = (total_ms / step_ms.max(1)).max(3) as usize;

    // Random chance to insert a tiny pause mid-path (people hesitate).
    let maybe_pause_at = if rng.gen::<f32>() < 0.25 { Some(rng.gen_range(steps/3..(2*steps/3).max(steps/3+1))) } else { None };

    // Jitter parameters
    let jitter_amp = settings.micro_jitter_px;
    let jitter_hz = (settings.micro_jitter_hz * (1.0 + rng.gen_range(-0.2..0.2))).max(1.0);

    for i in 0..=steps {
        let raw_t = i as f32 / steps as f32;
        let t = ease_in_out(raw_t);

        let (mut x, mut y) = cubic_bezier(p0, p1, p2, p3, t);

        // Micro jitter (sinusoid + tiny random) applied orthogonally to path direction
        let w = 2.0 * std::f32::consts::PI * jitter_hz * (i as f32 * (step_ms as f32 / 1000.0));
        let jitter = w.sin() * jitter_amp + rng.gen_range(-jitter_amp..jitter_amp) * 0.25;

        // Estimate tangent for orthogonal jitter
        let tp = cubic_bezier(p0, p1, p2, p3, (t + 1.0/steps as f32).min(1.0));
        let dx = tp.0 - x;
        let dy = tp.1 - y;
        let d = (dx*dx + dy*dy).sqrt().max(1.0);
        let (nx, ny) = (-dy/d, dx/d);
        x += nx * jitter;
        y += ny * jitter;

        let (mut xi, mut yi) = (x.round() as i32, y.round() as i32);
        if let Some(b) = bounds {
            (xi, yi) = b.clamp((xi, yi));
        }

        enigo.mouse_move_to(xi, yi);

        // Mid-path micro-pause
        if let Some(pause_idx) = maybe_pause_at {
            if i == pause_idx {
                thread::sleep(Duration::from_millis(
                    rng.gen_range(settings.min_pause_ms..=settings.max_pause_ms)
                ));
            }
        }

        thread::sleep(Duration::from_millis(step_ms as u64));
    }
}
