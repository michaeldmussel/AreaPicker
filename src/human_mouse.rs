use enigo::MouseControllable;
use std::time::{Duration, Instant};

/// Smooth, time-based pointer travel with ease-in-out timing.
/// This is for UX polish only (not for evading detection).
pub fn move_mouse_smooth(
    enigo: &mut enigo::Enigo,
    start_x: i32,
    start_y: i32,
    end_x: i32,
    end_y: i32,
    duration: Duration,
    micro_jitter: bool,
) {
    let start = Instant::now();
    let dx = (end_x - start_x) as f32;
    let dy = (end_y - start_y) as f32;

    let mut last_sent = (start_x, start_y);

    loop {
        let t = start.elapsed();
        let done = t >= duration;
        let u = if done {
            1.0
        } else {
            (t.as_secs_f32() / duration.as_secs_f32()).clamp(0.0, 1.0)
        };

        // ease-in-out cubic
        let eased = if u < 0.5 {
            4.0 * u * u * u
        } else {
            1.0 - (-2.0 * u + 2.0).powi(3) / 2.0
        };

        let mut x = start_x as f32 + dx * eased;
        let mut y = start_y as f32 + dy * eased;

        if micro_jitter && !done {
            let phase = (t.as_micros() % 10_000) as f32;
            let j = (phase / 10_000.0 * std::f32::consts::TAU).sin() * 0.4;
            x += j;
            y += j;
        }

        let xi = x.round() as i32;
        let yi = y.round() as i32;

        if (xi, yi) != last_sent {
            enigo.mouse_move_to(xi, yi);
            last_sent = (xi, yi);
        }

        if done { break; }
        std::thread::sleep(Duration::from_millis(4));
    }
}
