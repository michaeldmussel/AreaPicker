# Create a temporary file with the corrected spawn function
impl ClickJob {
    fn spawn(config: Arc<Mutex<ClickConfig>>) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let running_clone = Arc::clone(&running);
        let config_clone = Arc::clone(&config);

        eprintln!("Starting click job with config: {:#?}", config.lock());

        let handle = thread::spawn(move || {
            let mut rng = rand::thread_rng();
            let mut last_pos: Option<(i32,i32)> = None;
            
            // For sequence mode tracking
            let mut current_action_clicks: u32 = 0;
            let mut cycles_completed: u32 = 0;
            
            // For legacy single-region mode
            let mut clicks_remaining = config_clone.lock().finite_clicks;

            while running_clone.load(Ordering::Relaxed) {
                let config = config_clone.lock();
                let mut enigo = enigo::Enigo::new();

                // Check if we should continue based on finite clicks setting
                if let Some(clicks) = clicks_remaining {
                    if clicks == 0 {
                        break;
                    }
                }
                
                // Handle sequence mode
                if config.sequence_mode {
                    if config.sequence.is_empty() {
                        drop(config);
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }

                    let action = &config.sequence[config.current_action];
                    
                    // Check if we need to move to next action
                    if current_action_clicks >= action.clicks_per_cycle {
                        current_action_clicks = 0;
                        
                        // Update cycle count if we're at the end of sequence
                        if config.current_action == config.sequence.len() - 1 {
                            cycles_completed += 1;
                            
                            // Check cycle limit
                            if let Some(max_cycles) = config.sequence_cycles {
                                if cycles_completed >= max_cycles {
                                    break;
                                }
                            }
                            
                            config.current_action = 0;
                        } else {
                            config.current_action += 1;
                        }
                        
                        continue;
                    }

                    // Get random point within current action's bounds
                    let x = rng.gen_range(action.bounds.min_x..=action.bounds.max_x);
                    let y = rng.gen_range(action.bounds.min_y..=action.bounds.max_y);
                    
                    // Move mouse with human-like motion
                    if let Some((last_x, last_y)) = last_pos {
                        enigo.mouse_move_to(last_x, last_y);
                        human_mouse::move_mouse_human(&mut enigo, last_x, last_y, x, y);
                    } else {
                        enigo.mouse_move_to(x, y);
                    }
                    last_pos = Some((x, y));
                    
                    // Click
                    enigo.mouse_click(match action.button {
                        ClickButton::Left => MouseButton::Left,
                        ClickButton::Right => MouseButton::Right,
                    });
                    current_action_clicks += 1;

                    // Random delay based on action's interval settings
                    let min_ms = (action.min_secs * 1000.0) as u64;
                    let max_ms = (action.max_secs * 1000.0) as u64;
                    let delay = rng.gen_range(min_ms..=max_ms);
                    std::thread::sleep(Duration::from_millis(delay));
                
                // Handle single region mode
                } else if let Some(bounds) = &config.bounds {
                    if !bounds.is_valid() {
                        drop(config);
                        std::thread::sleep(Duration::from_millis(200));
                        continue;
                    }

                    // Get random point within bounds
                    let x = rng.gen_range(bounds.min_x..=bounds.max_x);
                    let y = rng.gen_range(bounds.min_y..=bounds.max_y);
                    
                    // Move mouse with human-like motion
                    if let Some((last_x, last_y)) = last_pos {
                        enigo.mouse_move_to(last_x, last_y);
                        human_mouse::move_mouse_human(&mut enigo, last_x, last_y, x, y);
                    } else {
                        enigo.mouse_move_to(x, y);
                    }
                    last_pos = Some((x, y));
                    
                    // Click
                    enigo.mouse_click(match config.button {
                        ClickButton::Left => MouseButton::Left,
                        ClickButton::Right => MouseButton::Right,
                    });

                    // Update click counter if using finite clicks
                    if let Some(ref mut clicks) = clicks_remaining {
                        *clicks = clicks.saturating_sub(1);
                    }

                    // Random delay based on interval settings
                    let min_ms = (config.min_secs * 1000.0) as u64;
                    let max_ms = (config.max_secs * 1000.0) as u64;
                    let delay = rng.gen_range(min_ms..=max_ms);
                    std::thread::sleep(Duration::from_millis(delay));
                } else {
                    drop(config);
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        });

        Self {
            running,
            config,
            handle: Some(handle),
        }
    }
