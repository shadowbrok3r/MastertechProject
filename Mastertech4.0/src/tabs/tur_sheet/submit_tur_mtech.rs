use database::schema::{
    utilities::{check_for_duplicates, create_full_task_payload}, 
    merge_computer, merge_customer, merge_task, merge_ticket,
    DuplicateCheckResult, MergeResolution, TaskCreationResult
};
use displays::{get_toast_sender, modals::DuplicateMergeModal, ToastMessage};
use crate::app_state::{MastertechContext, PendingTurData, TurSubmitState};
use tokio::spawn;
use log::info;

impl MastertechContext {
    /// Main entry point for submitting a TUR sheet.
    /// This initiates the duplicate check flow.
    pub fn submit_tur_mastertech(&mut self) {
        // Don't start a new submission if one is already in progress
        if self.tur_submit_state != TurSubmitState::Idle {
            info!("TUR submission already in progress, ignoring duplicate click");
            return;
        }

        let mut task_data = self.task_data.clone();
        let customer_data = self.customer_data.clone();
        let ticket_data = self.ticket_data.clone();
        let computer_data = self.computer_data.clone();
        let task_notes = self.task_notes.clone();

        task_data.due_date = self.date.into();
        let send_specs = self.send_specs;
        let service_number = ticket_data.service_number.clone();

        // Store pending data for later use after resolution
        self.pending_tur_data = Some(PendingTurData {
            task_data: task_data.clone(),
            ticket_data: ticket_data.clone(),
            customer_data: customer_data.clone(),
            computer_data: computer_data.clone(),
            task_notes,
            send_specs,
        });

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
            
            if result.has_conflicts() && !result.all_identical() {
                // Show merge modal
                info!("Conflicts found, opening merge modal");
                self.duplicate_merge_modal = Some(DuplicateMergeModal::new(result));
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
        let (task_data, ticket_data, customer_data, computer_data) = if let Some(res) = resolution {
            // Get the duplicate check result to access existing data
            // For now, we'll just use the pending data since the modal was closed
            // In a real implementation, you'd store the check result
            (pending.task_data, pending.ticket_data, pending.customer_data, pending.computer_data)
        } else {
            (pending.task_data, pending.ticket_data, pending.customer_data, pending.computer_data)
        };

        let task_notes = pending.task_notes;
        let send_specs = pending.send_specs;

        let state_tx = self.duplicate_check_tx.clone();
        
        spawn(async move {
            let send_payload_result = create_full_task_payload(
                ticket_data,
                customer_data,
                computer_data,
                task_data,
                task_notes,
                send_specs,
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
