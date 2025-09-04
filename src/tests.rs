#[cfg(test)]
mod tests {
    use super::*;
    use crate::human_mouse::Bounds;
    use std::time::Duration;

    #[test]
    fn test_bounds_validation() {
        let valid_bounds = Bounds {
            min_x: 100,
            max_x: 200,
            min_y: 100,
            max_y: 200,
        };
        assert!(valid_bounds.is_valid());
        assert_eq!(valid_bounds.width(), 100);
        assert_eq!(valid_bounds.height(), 100);

        let invalid_bounds = Bounds {
            min_x: 200,
            max_x: 100,
            min_y: 200,
            max_y: 100,
        };
        assert!(!invalid_bounds.is_valid());
    }

    #[test]
    fn test_click_job_creation() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds {
                min_x: 100,
                max_x: 200,
                min_y: 100,
                max_y: 200,
            }),
            button: ClickButton::Left,
            min_secs: 1.0,
            max_secs: 2.0,
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        assert!(job.running.load(Ordering::Relaxed));

        // Test stopping
        job.stop();
        assert!(!job.running.load(Ordering::Relaxed));
    }

    #[test]
    fn test_app_state_defaults() {
        let state = AppState::default();
        assert!(!state.picking_area);
        assert!(state.drag_start.is_none());
        assert!(state.drag_end.is_none());
        assert!(state.click_button_left);
        assert!(state.job.is_none());
        
        // Check default bounds
        assert_eq!(state.bounds_inputs, [100, 400, 100, 400]);
        
        // Check default timing
        assert_eq!(state.min_secs, 1.0);
        assert_eq!(state.max_secs, 3.0);
    }

    #[test]
    fn test_bounds_from_drag() {
        let mut state = AppState::default();
        
        // Test drag selection
        state.drag_start = Some(Pos2::new(100.0, 100.0));
        state.drag_end = Some(Pos2::new(200.0, 200.0));
        state.bounds_from_drag();

        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
        
        // Test reverse drag (from bottom-right to top-left)
        state.drag_start = Some(Pos2::new(200.0, 200.0));
        state.drag_end = Some(Pos2::new(100.0, 100.0));
        state.bounds_from_drag();

        assert_eq!(state.bounds_inputs, [100, 200, 100, 200]);
    }

    #[test]
    fn test_click_interval() {
        let config = Arc::new(Mutex::new(ClickConfig {
            bounds: Some(Bounds {
                min_x: 100,
                max_x: 200,
                min_y: 100,
                max_y: 200,
            }),
            button: ClickButton::Left,
            min_secs: 0.1,
            max_secs: 0.2,
        }));

        let job = ClickJob::spawn(Arc::clone(&config));
        
        // Let it run briefly to ensure it's working
        std::thread::sleep(Duration::from_millis(500));
        
        job.stop();
        assert!(!job.running.load(Ordering::Relaxed));
    }
}
