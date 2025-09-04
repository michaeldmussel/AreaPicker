    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if self.picking_area {
            // When in picking mode, make window fullscreen and translucent
            if self.window_visible {
                self.window_visible = false;
                // Get screen dimensions
                let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds();
                let width = max_x - min_x;
                let height = max_y - min_y;
                
                // Make window fullscreen and center it
                ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(width as f32, height as f32)));
                if let Some(cmd) = egui::ViewportCommand::center_on_screen(ctx) {
                    ctx.send_viewport_cmd(cmd);
                }
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(false));
                return;
            }
            
            let layer_id = egui::LayerId::new(egui::Order::Foreground, egui::Id::new("picker"));
            let painter = egui::Painter::new(
                ctx.clone(),
                layer_id,
                egui::Rect::EVERYTHING,
            );
            
            // Get the screen dimensions and create rect in absolute coordinates
            let (min_x, min_y, max_x, max_y) = Self::get_total_screen_bounds();
            let screen_rect = egui::Rect::from_min_max(
                Pos2::new(min_x as f32, min_y as f32),
                Pos2::new(max_x as f32, max_y as f32)
            );
            
            painter.rect_filled(screen_rect, 0.0, Color32::from_rgba_premultiplied(128, 128, 128, 100));

            egui::Area::new(egui::Id::new("picker_area")).order(egui::Order::Foreground).show(ctx, |ui| {
                let resp = ui.allocate_rect(screen_rect, Sense::click_and_drag());
                
                // Get absolute screen coordinates using winit
                let absolute_pos = if let Some(pos) = resp.hover_pos() {
                    // Use raw cursor position directly
                    Pos2::new(pos.x, pos.y)
                } else {
                    Pos2::new(0.0, 0.0)
                };
                
                if resp.drag_started() {
                    self.drag_start = Some(absolute_pos);
                    self.drag_end = self.drag_start;
                }
                if resp.dragged() {
                    self.drag_end = Some(absolute_pos);
                }
                if resp.drag_stopped() {
                    self.drag_end = Some(absolute_pos);
                    self.bounds_from_drag();
                    self.picking_area = false;
                    // Restore window to normal state
                    self.window_visible = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(false));
                    ctx.send_viewport_cmd(egui::ViewportCommand::Decorations(true));
                    // Reset window size to default
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::Vec2::new(520.0, 380.0)));
                }

                if let (Some(a), Some(b)) = (self.drag_start, self.drag_end) {
                    let rect = Rect::from_two_pos(a, b);
                    let stroke = egui::Stroke { width: 2.0, color: Color32::LIGHT_BLUE };
                    painter.rect_stroke(rect, 0.0, stroke);
                }
            });

            ctx.request_repaint();
            return;
        }

        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.heading("Area Clicker — MVP");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let sequence_mode = self.config.lock().sequence_mode;
            
            ui.horizontal_wrapped(|ui| {
                // Left panel - Sequence management
                ui.group(|ui| {
                    let mut sequence_mode = sequence_mode;
                    if ui.checkbox(&mut sequence_mode, "Enable Sequence Mode").changed() {
                        self.config.lock().sequence_mode = sequence_mode;
                    }

                    if sequence_mode {
                        ui.separator();
                        ui.label("Sequence Steps");
                        
                        // List existing steps
                        let mut to_remove = None;
                        {
                            let config = self.config.lock();
                            for (i, action) in config.sequence.iter().enumerate() {
                                ui.horizontal(|ui| {
                                    if ui.button(format!("Edit #{}", i + 1)).clicked() {
                                        self.current_sequence_index = i;
                                        self.editing_sequence = true;
                                        self.bounds_inputs = [
                                            action.bounds.min_x,
                                            action.bounds.max_x,
                                            action.bounds.min_y,
                                            action.bounds.max_y
                                        ];
                                        self.click_button_left = matches!(action.button, ClickButton::Left);
                                        self.min_secs = action.min_secs;
                                        self.max_secs = action.max_secs;
                                        self.sequence_edit_name = action.name.clone();
                                        self.sequence_edit_clicks = action.clicks_per_cycle;
                                    }

                                    let mut enabled = action.enabled;
                                    if ui.checkbox(&mut enabled, "").changed() {
                                        self.config.lock().sequence[i].enabled = enabled;
                                    }

                                    ui.label(format!(
                                        "{}: [{},{}]x[{},{}], {} clicks, {:.1}-{:.1}s",
                                        action.name,
                                        action.bounds.min_x, action.bounds.max_x,
                                        action.bounds.min_y, action.bounds.max_y,
                                        action.clicks_per_cycle,
                                        action.min_secs, action.max_secs
                                    ));

                                    if ui.button("🗑").clicked() {
                                        to_remove = Some(i);
                                    }
                                });
                            }
                        }

                        // Handle removal outside the iteration
                        if let Some(i) = to_remove {
                            self.config.lock().sequence.remove(i);
                        }

                        // Add new step button
                        if ui.button("Add New Step").clicked() {
                            self.current_sequence_index = self.config.lock().sequence.len();
                            self.editing_sequence = true;
                            self.bounds_inputs = [100, 400, 100, 400];
                            self.click_button_left = true;
                            self.min_secs = 1.0;
                            self.max_secs = 3.0;
                            self.sequence_edit_name = format!("Step {}", self.current_sequence_index + 1);
                            self.sequence_edit_clicks = 1;
                        }
                    }
                });

                ui.separator();

                // Right panel - Area configuration
                ui.group(|ui| {
                    if sequence_mode {
                        ui.label(if self.editing_sequence {
                            format!("Editing: {}", self.sequence_edit_name)
                        } else {
                            "Select or add a sequence step to edit".to_string()
                        });
                    } else {
                        ui.label("Single Area Configuration");
                    }

                    ui.horizontal(|ui| { ui.label("min X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[0])); });
                    ui.horizontal(|ui| { ui.label("max X"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[1])); });
                    ui.horizontal(|ui| { ui.label("min Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[2])); });
                    ui.horizontal(|ui| { ui.label("max Y"); ui.add(egui::DragValue::new(&mut self.bounds_inputs[3])); });

                    if ui.button("Pick Area (drag a rectangle)").clicked() { 
                        self.drag_start = None;
                        self.drag_end = None;
                        self.picking_area = true;
                        self.window_visible = true;
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Click type:");
                        ui.checkbox(&mut self.click_button_left, "Left");
                        let mut right = !self.click_button_left;
                        if ui.checkbox(&mut right, "Right").clicked() { self.click_button_left = !right; }
                    });
                    
                    ui.horizontal(|ui| {
                        ui.label("Interval (seconds):");
                        ui.add(egui::DragValue::new(&mut self.min_secs).speed(0.1));
                        ui.label("to");
                        ui.add(egui::DragValue::new(&mut self.max_secs).speed(0.1));
                    });

                    if sequence_mode && self.editing_sequence {
                        ui.horizontal(|ui| {
                            ui.label("Step name:");
                            ui.text_edit_singleline(&mut self.sequence_edit_name);
                        });
                        ui.horizontal(|ui| {
                            ui.label("Clicks per cycle:");
                            ui.add(egui::DragValue::new(&mut self.sequence_edit_clicks).speed(1.0).min(1));
                        });
                        
                        if ui.button("Save Step").clicked() {
                            let action = SequenceAction {
                                bounds: Bounds {
                                    min_x: self.bounds_inputs[0],
                                    max_x: self.bounds_inputs[1],
                                    min_y: self.bounds_inputs[2],
                                    max_y: self.bounds_inputs[3],
                                },
                                button: if self.click_button_left { ClickButton::Left } else { ClickButton::Right },
                                min_secs: self.min_secs,
                                max_secs: self.max_secs,
                                clicks_per_cycle: self.sequence_edit_clicks,
                                name: self.sequence_edit_name.clone(),
                                enabled: true,
                            };

                            let mut config = self.config.lock();
                            if self.current_sequence_index < config.sequence.len() {
                                config.sequence[self.current_sequence_index] = action;
                            } else {
                                config.sequence.push(action);
                            }
                            self.editing_sequence = false;
                        }
                    } else if !sequence_mode {
                        ui.horizontal(|ui| {
                            ui.label("Click limit:");
                            let mut has_limit = self.finite_clicks.is_some();
                            if ui.checkbox(&mut has_limit, "Limited clicks").clicked() {
                                self.finite_clicks = if has_limit { Some(100) } else { None };
                            }
                            if let Some(ref mut clicks) = self.finite_clicks {
                                ui.add(egui::DragValue::new(clicks).speed(1.0));
                            }
                        });
                    } else if sequence_mode {
                        ui.horizontal(|ui| {
                            ui.label("Sequence cycles:");
                            let mut has_cycles = self.sequence_cycles.is_some();
                            if ui.checkbox(&mut has_cycles, "Limited cycles").clicked() {
                                self.sequence_cycles = if has_cycles { Some(1) } else { None };
                            }
                            if let Some(ref mut cycles) = self.sequence_cycles {
                                ui.add(egui::DragValue::new(cycles).speed(1.0));
                            }
                        });
                    }

                    ui.horizontal(|ui| {
                        if ui.button("Start").clicked() { self.start(); }
                        if ui.button("Pause").clicked() { self.pause(); }
                        if ui.button("Stop").clicked() { self.stop(); }
                    });

                    if let Some(job) = &self.job {
                        let running = job.running.load(Ordering::Relaxed);
                        ui.label(format!("Status: {}", if running { "Running" } else { "Stopped" }));
                    } else {
                        ui.label("Status: Stopped");
                    }
                });
            });

            // Preview rectangle
            if let Some(b) = self.config.lock().bounds {
                let info = format!("Active bounds: x=[{}..{}], y=[{}..{}] ({}x{})", 
                    b.min_x, b.max_x, b.min_y, b.max_y, b.width(), b.height());
                ui.separator();
                ui.monospace(info);
            }
        });
    }
