use egui::{TextEdit, Ui};
use egui_form::{validator::validator::Validate, Form, FormField, _validator_field_path};
use egui_form::validator::field_path;
use crate::app_state::MtechServer;

#[derive(Debug, Default)]
struct Fields {
    user_name: String,
}

#[derive(Validate, Debug)]
struct Test {
    #[validate(length(min = 3, max = 10))]
    pub user_name: String,
    #[validate(email)]
    pub email: String,
    #[validate(nested)]
    pub nested: Nested,
    #[validate(nested)]
    pub vec: Vec<Nested>,
}





impl MtechServer{
    fn form_ui(ui: &mut egui::Ui, test: &mut Test) {
        let mut form = Form::new().add_report(
            egui_form::validator::ValidatorReport::new(test.validate()).with_translation(|error| {
                // Since validator doesn't have default messages, we have to provide our own
                if let Some(msg) = &error.message {
                    return msg.clone();
                }
    
                match error.code.as_ref() {
                    "email" => "Invalid email".into(),
                    "length" => format!(
                        "Must be between {} and {} characters long",
                        error.params["min"], error.params["max"]
                    )
                    .into(),
                    _ => format!("Validation Failed: {}", error.code).into(),
                }
            }),
        );
    
        FormField::new(&mut form, field_path!("user_name"))
            .label("User Name")
            .ui(ui, egui::TextEdit::singleline(&mut test.user_name));
        FormField::new(&mut form, field_path!("email"))
            .label("Email")
            .ui(ui, egui::TextEdit::singleline(&mut test.email));
        // FormField::new(&mut form, field_path!("nested", "test"))
        //     .label("Nested Test")
        //     .ui(ui, egui::Slider::new(&mut test.nested.test, 0..=11));
        FormField::new(&mut form, field_path!("vec", 0, "test"))
            .label("Vec Test")
            .ui(
                ui,
                egui::DragValue::new(&mut test.vec[0].test).clamp_range(0..=11),
            );
    
        if let Some(Ok(())) = form.handle_submit(&ui.button("Submit"), ui) {
            println!("Form submitted: {:?}", test);
        }
    }
}