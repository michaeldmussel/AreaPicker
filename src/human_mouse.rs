use enigo::MouseControllable;
use rand::Rng;
use std::time::Duration;

/// Simulates human-like mouse movement using Bézier curves
pub fn move_mouse_human(enigo: &mut enigo::Enigo, start_x: i32, start_y: i32, end_x: i32, end_y: i32) {
    let mut rng = rand::thread_rng();
    
    // Control points for Bézier curve (creates natural arc)
    let control1_x = start_x + (end_x - start_x) / 3 + rng.gen_range(-20..=20);
    let control1_y = start_y + (end_y - start_y) / 3 + rng.gen_range(-20..=20);
    let control2_x = start_x + 2 * (end_x - start_x) / 3 + rng.gen_range(-20..=20);
    let control2_y = start_y + 2 * (end_y - start_y) / 3 + rng.gen_range(-20..=20);

    // Number of steps (more steps = smoother but slower movement)
    let steps = ((((end_x - start_x).pow(2) + (end_y - start_y).pow(2)) as f64).sqrt() / 2.0) as i32;
    let steps = steps.max(10).min(50); // Minimum 10 steps, maximum 50

    for i in 0..=steps {
        let t = i as f64 / steps as f64;
        
        // Cubic Bézier curve formula
        let x = (1.0 - t).powi(3) * start_x as f64
            + 3.0 * (1.0 - t).powi(2) * t * control1_x as f64
            + 3.0 * (1.0 - t) * t.powi(2) * control2_x as f64
            + t.powi(3) * end_x as f64;
            
        let y = (1.0 - t).powi(3) * start_y as f64
            + 3.0 * (1.0 - t).powi(2) * t * control1_y as f64
            + 3.0 * (1.0 - t) * t.powi(2) * control2_y as f64
            + t.powi(3) * end_y as f64;

        // Move to calculated position
        enigo.mouse_move_to(x as i32, y as i32);
        
        // Random small delay between movements (5-15ms)
        std::thread::sleep(Duration::from_millis(rng.gen_range(5..=15)));
    }
}
