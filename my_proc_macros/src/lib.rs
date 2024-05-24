extern crate proc_macro;

use proc_macro::TokenStream;
use quote::quote;
use syn::{parse_macro_input, DeriveInput};

#[proc_macro_derive(DelegateTraits)]
pub fn delegate_traits(input: TokenStream) -> TokenStream {
    let input = parse_macro_input!(input as DeriveInput);
    let name = input.ident;

    let expanded = quote! {
        impl #name {
            pub fn inner(&self) -> &TaskPayload {
                &self.0
            }

            pub fn inner_mut(&mut self) -> &mut TaskPayload {
                &mut self.0
            }
        }

        impl Displayable for #name {
            fn display_task_cards(&mut self, ui: &mut Ui) -> anyhow::Result<(), anyhow::Error> {
                self.0.display_task_cards(ui)
            }
            // fn display_table(&mut self, ui: &mut Ui, tasks: Vec<TaskPayload>) -> anyhow::Result<(), anyhow::Error> {
            //     self.0.display_table(ui, tasks)
            // }
        }

        impl Updatable for #name {
            fn update_completed(&mut self, completed: bool, db: Database) {
                self.0.update_completed(completed, db)
            }

            fn update_due_date(&mut self, due_date: String, db: Database) {
                self.0.update_due_date(due_date, db)
            }

            fn update_assignee_initials(&mut self, initials: String, db: Database) {
                self.0.update_assignee_initials(initials, db)
            }

            fn update_task_name(&mut self, name: String, db: Database) {
                self.0.update_task_name(name, db)
            }

            fn update_status(&mut self, status: Status, db: Database) {
                self.0.update_status(status, db)
            }

            fn update_dep(&mut self, dep: Store, db: Database) {
                self.0.update_dep(dep, db)
            }

            fn update_priority(&mut self, priority: Option<Priority>, db: Database) {
                self.0.update_priority(priority, db)
            }

            fn update_task_description(&mut self, description: Option<String>, db: Database) {
                self.0.update_task_description(description, db)
            }
        }

        impl Interaction for #name {
            fn interact_task_name(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_task_name(ui)
            }

            fn interact_task_description(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_task_description(ui)
            }

            fn interact_recommendations(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_recommendations(ui)
            }

            fn interact_due_date(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_due_date(ui)
            }

            fn interact_completed(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_completed(ui)
            }

            fn interact_status(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_status(ui)
            }

            fn interact_dep(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_dep(ui)
            }

            fn interact_priority(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_priority(ui)
            }

            fn interact_assignee_initials(&mut self, ui: &mut Ui) -> Option<Response> {
                self.0.interact_assignee_initials(ui)
            }
        }

        impl TaskContext for #name{
            fn get_store_users(&mut self, db: Database, tx: Sender<Vec<ReturnedStoreUsers>>){
                self.0.get_store_users(db, tx)
            }
            fn get_computer_data(&mut self, db: Database, tx: Sender<Vec<ComputerData>>){
                self.0.get_computer_data(db, tx)
            }
            fn get_customer_data(&mut self, db: Database, tx: Sender<Vec<CustomerData>>){
                self.0.get_customer_data(db, tx)
            }
            fn get_service_data(&mut self, db: Database, tx: Sender<Vec<TicketData>>){
                self.0.get_service_data(db, tx)
            }
            fn get_task_notes(&mut self, db: Database, tx: Sender<Vec<TaskNotePayload>>){
                self.0.get_task_notes(db, tx)
            }
        }
    };

    TokenStream::from(expanded)
}
