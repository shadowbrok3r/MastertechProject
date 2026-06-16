use database::schema::{
    utilities::{check_for_duplicates, create_full_task_payload}, 
    merge_computer, merge_customer, merge_task, merge_ticket,
    DuplicateCheckResult, FieldDisplay, MergeResolution, RecordIdExt, TaskCreationResult, TaskHistory
};
use displays::{get_toast_sender, modals::DuplicateMergeModal, ToastMessage};
use crate::app_state::{MastertechContext, PendingTurData, TurSubmitState};
use tokio::spawn;
use log::info;
use eframe::egui::{Align, Align2, Area, Button, Color32, Frame, Layout, Order, RichText, Vec2};
use std::time::Instant;

/// Duration for confirmation countdown (in seconds)
const CONFIRMATION_COUNTDOWN_SECS: f32 = 5.0;

impl MastertechContext {
    /// Main entry point for submitting a TUR sheet.
    /// This shows a 5-second confirmation toast before starting the duplicate check.
    pub fn submit_tur_mastertech(&mut self) {
        // Don't start a new submission if one is already in progress
        if self.tur_submit_state != TurSubmitState::Idle {
            info!("TUR submission already in progress, ignoring duplicate click");
            return;
        }

        // Use local fields which are populated by the TUR sheet form
        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        let send_specs = self.send_specs;
        let service_number = ticket_data.service_number.clone();
        
        // Populate task_data fields that need to be set before duplicate check
        task_data.due_date = self.date.into();
        task_data.service_number = Some(service_number.clone());
        task_data.task_name = format!("{} - {}", &customer_data.name, &service_number);
        
        // Set assignee from the TUR form's task_data.assignee (should be set by the form)
        // If it's still a random ID, use current user as fallback
        if task_data.assignee.key_string().contains("-") {
            // Random UUID format - use current user instead
            if let Some(ref user) = self.shared_ctx.current_user {
                info!("Assignee was random ID, using current user: {:?}", user.get_id());
                task_data.assignee = user.get_id();
            }
        }
        info!("Task assignee: {:?}", task_data.assignee);

        // Store pending data for later use after resolution
        self.pending_tur_data = Some(PendingTurData {
            task_data: task_data.clone(),
            ticket_data: ticket_data.clone(),
            customer_data: customer_data.clone(),
            computer_data: computer_data.clone(),
            task_notes,
            send_specs,
            duplicate_check_result: None, // Will be populated when duplicate check completes
            confirmation_start: Some(Instant::now()),
        });

        // Update state to show confirmation toast
        self.tur_submit_state = TurSubmitState::AwaitingConfirmation;
        info!("Showing confirmation toast for service #{}", service_number);
    }

    /// Renders the confirmation toast UI. Call this in the UI loop.
    pub fn render_confirmation_toast(&mut self, ctx: &eframe::egui::Context) {
        if self.tur_submit_state != TurSubmitState::AwaitingConfirmation {
            return;
        }

        let Some(ref pending) = self.pending_tur_data else {
            self.tur_submit_state = TurSubmitState::Idle;
            return;
        };

        let Some(start_time) = pending.confirmation_start else {
            self.tur_submit_state = TurSubmitState::Idle;
            return;
        };

        let elapsed = start_time.elapsed().as_secs_f32();
        let remaining = (CONFIRMATION_COUNTDOWN_SECS - elapsed).max(0.0);
        let progress = elapsed / CONFIRMATION_COUNTDOWN_SECS;

        // Check if countdown has finished
        if remaining <= 0.0 {
            info!("Confirmation countdown finished, proceeding with duplicate check");
            self.start_duplicate_check();
            return;
        }

        // Render toast-like UI
        let mut should_submit = false;
        let mut should_cancel = false;

        Area::new("confirmation_toast".into())
            .anchor(Align2::RIGHT_BOTTOM, [-20.0, -80.0])
            .order(Order::Foreground)
            .interactable(true)
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .inner_margin(12.0)
                    .fill(ui.style().visuals.window_fill)
                    .stroke(ui.style().visuals.window_stroke)
                    .show(ui, |ui| {
                        ui.set_min_width(280.0);
                        
                        // Header with icon and countdown
                        ui.horizontal(|ui| {
                            ui.label(RichText::new("⏳").size(18.0));
                            ui.label(RichText::new(format!(
                                "Creating task in {:.1}s...", 
                                remaining
                            )).strong().size(14.0));
                        });
                        
                        ui.add_space(6.0);
                        
                        // Service number info
                        let service_num = pending.ticket_data.service_number.clone();
                        ui.label(RichText::new(format!("Service #{}", service_num))
                            .color(Color32::from_rgb(150, 180, 220)));
                        
                        ui.add_space(8.0);
                        
                        // Progress bar
                        let progress_rect = ui.available_rect_before_wrap();
                        let progress_height = 4.0;
                        let bar_rect = eframe::egui::Rect::from_min_size(
                            eframe::egui::pos2(progress_rect.left(), progress_rect.top()),
                            eframe::egui::vec2(progress_rect.width(), progress_height)
                        );
                        
                        // Background
                        ui.painter().rect_filled(
                            bar_rect,
                            2.0,
                            Color32::from_rgb(60, 60, 60)
                        );
                        
                        // Progress fill
                        let fill_width = bar_rect.width() * progress;
                        let fill_rect = eframe::egui::Rect::from_min_size(
                            bar_rect.min,
                            eframe::egui::vec2(fill_width, progress_height)
                        );
                        ui.painter().rect_filled(
                            fill_rect,
                            2.0,
                            Color32::from_rgb(52, 235, 171)
                        );
                        
                        ui.add_space(progress_height + 10.0);
                        
                        // Buttons
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.add(
                                Button::new(RichText::new("Submit Now").color(Color32::LIGHT_GREEN))
                                    .min_size(Vec2::new(90.0, 28.0))
                            ).clicked() {
                                should_submit = true;
                            }
                            
                            ui.add_space(8.0);
                            
                            if ui.add(
                                Button::new(RichText::new("Undo").color(Color32::LIGHT_RED))
                                    .min_size(Vec2::new(60.0, 28.0))
                            ).clicked() {
                                should_cancel = true;
                            }
                        });
                    });
            });

        // Request repaint for smooth countdown animation
        ctx.request_repaint();

        // Handle button clicks outside the closure
        if should_submit {
            info!("User clicked Submit Now");
            self.start_duplicate_check();
        } else if should_cancel {
            info!("User clicked Undo - cancelling task submission");
            self.tur_submit_state = TurSubmitState::Idle;
            self.pending_tur_data = None;
            
            let toast_tx = get_toast_sender();
            let _ = toast_tx.try_send(ToastMessage::Info("Task submission cancelled".to_string()));
        }
    }

    /// Actually starts the duplicate check process
    fn start_duplicate_check(&mut self) {
        let Some(ref pending) = self.pending_tur_data else {
            self.tur_submit_state = TurSubmitState::Idle;
            return;
        };

        let task_data = pending.task_data.clone();
        let ticket_data = pending.ticket_data.clone();
        let customer_data = pending.customer_data.clone();
        let computer_data = pending.computer_data.clone();
        let send_specs = pending.send_specs;
        let service_number = ticket_data.service_number.clone();

        // Update state
        self.tur_submit_state = TurSubmitState::CheckingDuplicates;
        
        // Start duplicate check
        let tx = self.duplicate_check_tx.clone();
        let computer_for_check = if send_specs { Some(computer_data.clone()) } else { None };
        
        spawn(async move {
            info!("Starting duplicate check for service #{}", service_number);
            
            match check_for_duplicates(
                &service_number,
                &task_data,
                &ticket_data,
                &customer_data,
                computer_for_check.as_ref(),
            ).await {
                Ok(result) => {
                    info!("Duplicate check completed: has_conflicts={}", result.has_conflicts());
                    let _ = tx.try_send(result);
                },
                Err(e) => {
                    info!("Duplicate check error: {:?}", e);
                    // Send empty result on error to continue with submission
                    let _ = tx.try_send(DuplicateCheckResult::new(service_number));
                }
            }
        });
    }

    /// Call this in the update loop to process duplicate check results
    pub fn process_duplicate_check_results(&mut self) {
        // Check for duplicate check results
        if let Ok(result) = self.duplicate_check_rx.try_recv() {
            info!("Received duplicate check result for service #{}", result.service_number);
            
            // Store the duplicate check result for later use in resolution
            if let Some(ref mut pending) = self.pending_tur_data {
                pending.duplicate_check_result = Some(result.clone());
            }
            
            if result.has_conflicts() && !result.all_identical() {
                // Show merge modal
                info!("Conflicts found, opening merge modal");
                let mut modal = DuplicateMergeModal::new(result.clone());
                
                // Populate user cache for assignee display
                // Add current user to cache
                if let Some(ref user) = self.shared_ctx.current_user {
                    modal.cache_user(&user.get_id(), user.get_username());
                }
                // Add store users to cache
                for user in &self.shared_ctx.store_users {
                    modal.cache_user(&user.get_id(), user.get_username());
                }
                // Log assignees from the duplicate check result
                if let Some(ref task_dup) = result.task {
                    info!("Existing task assignee: {:?}", task_dup.existing.assignee);
                    info!("New task assignee: {:?}", task_dup.new.assignee);
                }
                
                self.duplicate_merge_modal = Some(modal);
                self.tur_submit_state = TurSubmitState::AwaitingResolution;
            } else {
                // No conflicts or all identical - proceed with submission
                info!("No conflicts or all identical, proceeding with submission");
                self.proceed_with_submission(None);
            }
        }
    }

    /// Handle the merge modal updates
    pub fn handle_merge_modal(&mut self, ctx: &eframe::egui::Context) {
        if self.tur_submit_state != TurSubmitState::AwaitingResolution {
            return;
        }

        if let Some(ref mut modal) = self.duplicate_merge_modal {
            modal.show(ctx);
            
            if modal.is_confirmed() {
                info!("User confirmed merge resolution");
                let resolution = modal.get_resolution().clone();
                self.duplicate_merge_modal = None;
                self.proceed_with_submission(Some(resolution));
            } else if modal.is_cancelled() {
                info!("User cancelled merge resolution");
                self.duplicate_merge_modal = None;
                self.tur_submit_state = TurSubmitState::Idle;
                self.pending_tur_data = None;
                
                let toast_tx = get_toast_sender();
                let _ = toast_tx.try_send(ToastMessage::Info("Task submission cancelled".to_string()));
            }
        }
    }

    /// Proceed with actual submission after duplicate resolution
    fn proceed_with_submission(&mut self, resolution: Option<database::schema::DuplicateResolution>) {
        let Some(pending) = self.pending_tur_data.take() else {
            info!("No pending TUR data to submit");
            self.tur_submit_state = TurSubmitState::Idle;
            return;
        };

        self.tur_submit_state = TurSubmitState::Submitting;
        let toast_tx = get_toast_sender();

        // Apply resolution if provided
        let (mut task_data, ticket_data, customer_data, computer_data) = if let Some(ref res) = resolution {
            if let Some(ref check_result) = pending.duplicate_check_result {
                // Apply the user's resolution choices
                Self::apply_resolution(check_result, &pending, res)
            } else {
                info!("No duplicate check result found, using pending data as-is");
                (pending.task_data.clone(), pending.ticket_data.clone(), 
                 pending.customer_data.clone(), pending.computer_data.clone())
            }
        } else {
            (pending.task_data.clone(), pending.ticket_data.clone(), 
             pending.customer_data.clone(), pending.computer_data.clone())
        };

        // Determine if this is a modification (existing task with UseNew or Merge resolution)
        // and prepare task history data if so
        let task_history_data: Option<(database::schema::RecordId, serde_json::Value)> = 
            if let Some(ref res) = resolution {
                if let Some(ref check_result) = pending.duplicate_check_result {
                    if let Some(ref task_dup) = check_result.task {
                        // Only create history for UseNew or Merge resolutions
                        if matches!(res.task_resolution, MergeResolution::UseNew | MergeResolution::Merge) {
                            // Build diff JSON from the changes
                            let diff_fields = task_dup.existing.get_differing_fields(&task_data);
                            if !diff_fields.is_empty() {
                                let mut diff_map = serde_json::Map::new();
                                for (field_name, old_val, new_val) in diff_fields {
                                    diff_map.insert(field_name, serde_json::json!({
                                        "old": old_val,
                                        "new": new_val
                                    }));
                                }
                                Some((task_dup.existing.id.clone(), serde_json::Value::Object(diff_map)))
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            };

        // IMPORTANT: When there's an existing task, ALWAYS use the existing task's ID
        // The resolution determines which DATA to use, but we always update the existing record
        if let Some(ref check_result) = pending.duplicate_check_result {
            if let Some(ref task_dup) = check_result.task {
                // Always use existing task ID when there's a duplicate - this is an UPDATE
                info!("Existing task found, using existing task ID for update: {:?}", task_dup.existing.id);
                task_data.id = task_dup.existing.id.clone();
            }
        }

        let task_notes = pending.task_notes;
        let send_specs = pending.send_specs;

        // Get current user info for task history
        let current_user = self.shared_ctx.current_user.clone();

        let _state_tx = self.duplicate_check_tx.clone();
        
        spawn(async move {
            // Create task history record if we're modifying an existing task
            if let Some((task_id, diff)) = task_history_data {
                // Get current user for the history record
                if let Some(user) = &current_user {
                    let history = TaskHistory::new(
                        task_id,
                        user.get_id(),
                        user.get_username().to_string(),
                        diff,
                    );
                    match history.save().await {
                        Ok(_) => info!("Task history record created"),
                        Err(e) => log::error!("Failed to create task history record: {:?}", e),
                    }
                } else {
                    info!("No current user available for task history");
                }
            }

            let send_payload_result = create_full_task_payload(
                ticket_data,
                customer_data,
                computer_data,
                task_data,
                task_notes,
                send_specs,
                false,
                None,
            )
            .await;
            info!("send_payload_result: {send_payload_result:?}");

            // Send toast based on result
            match send_payload_result {
                TaskCreationResult::Created { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Success(
                        format!("Task created for service #{service_number}")
                    ));
                },
                TaskCreationResult::AlreadyExists { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Warning(
                        format!("Task already exists for service #{service_number}")
                    ));
                },
                TaskCreationResult::Updated { service_number } => {
                    let _ = toast_tx.try_send(ToastMessage::Info(
                        format!("Task updated for service #{service_number}")
                    ));
                },
                TaskCreationResult::Error { message } => {
                    let _ = toast_tx.try_send(ToastMessage::Error(
                        format!("Error creating task: {message}")
                    ));
                },
            }
        });

        // Reset state after spawning
        self.tur_submit_state = TurSubmitState::Idle;
    }

    /// Apply merge resolution to create final data
    pub fn apply_resolution(
        check_result: &DuplicateCheckResult,
        pending: &PendingTurData,
        resolution: &database::schema::DuplicateResolution,
    ) -> (database::schema::LiveTaskPayload, database::schema::TicketData, 
          database::schema::CustomerData, database::schema::ComputerData) {
        
        // Apply task resolution
        let task_data = if let Some(ref dup) = check_result.task {
            match resolution.task_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => pending.task_data.clone(),
                MergeResolution::Merge => merge_task(&dup.existing, &pending.task_data, &resolution.task_fields),
                MergeResolution::Cancel => pending.task_data.clone(),
            }
        } else {
            pending.task_data.clone()
        };

        // Apply ticket resolution
        let ticket_data = if let Some(ref dup) = check_result.service_order {
            match resolution.service_order_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => pending.ticket_data.clone(),
                MergeResolution::Merge => merge_ticket(&dup.existing, &pending.ticket_data, &resolution.service_order_fields),
                MergeResolution::Cancel => pending.ticket_data.clone(),
            }
        } else {
            pending.ticket_data.clone()
        };

        // Apply customer resolution
        let customer_data = if let Some(ref dup) = check_result.customer {
            match resolution.customer_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => pending.customer_data.clone(),
                MergeResolution::Merge => merge_customer(&dup.existing, &pending.customer_data, &resolution.customer_fields),
                MergeResolution::Cancel => pending.customer_data.clone(),
            }
        } else {
            pending.customer_data.clone()
        };

        // Apply computer resolution
        let computer_data = if let Some(ref dup) = check_result.computer {
            match resolution.computer_resolution {
                MergeResolution::KeepExisting => dup.existing.clone(),
                MergeResolution::UseNew => pending.computer_data.clone(),
                MergeResolution::Merge => merge_computer(&dup.existing, &pending.computer_data, &resolution.computer_fields),
                MergeResolution::Cancel => pending.computer_data.clone(),
            }
        } else {
            pending.computer_data.clone()
        };

        (task_data, ticket_data, customer_data, computer_data)
    }
}
