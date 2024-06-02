use egui::{Align, Layout, Ui};
use egui_extras::{Size, StripBuilder};
use serde::Serialize;

use crate::utilities::{DisplayModal, ModalTypes};

use super::{modals::ModalState, task_modal::ModalAction};

#[derive(Serialize, Default, Debug, Clone)]
pub struct CreateTaskModal{
    title: String,
    min_width: Option<f32>,
    min_height: Option<f32>,
    default_height: Option<f32>,
    full_span_content: bool,    
    state: ModalState
}

impl CreateTaskModal{
    /// Create a new modal with the given title.
    pub fn new(title: &str) -> Self {
        Self {
            title: title.to_owned(),
            min_width: Some(600.0),
            min_height: Some(600.0),
            default_height: Some(800.0),
            full_span_content: false,
            state: ModalState::default(),
        }
    }
}

impl ModalTypes for CreateTaskModal{
    fn modal_state(&mut self) -> &mut ModalState {
        &mut self.state
    }
    fn title(&self) -> String {
        "Task Details".to_string()
    }
}


impl DisplayModal for CreateTaskModal {
    fn display(&self, ui: &mut Ui) -> Option<ModalAction>{
        let mut response: Option<ModalAction> = None;
        StripBuilder::new(ui)
            .cell_layout(Layout::top_down_justified(Align::Center))
            .size(Size::exact(30.0))
            .size(Size::exact(10.0))
            .size(Size::exact(500.0))
            .vertical(|mut strip| 
        {
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::remainder())
                    .horizontal( |mut strip| 
                {
                    strip.cell(|_ui|{

                    });
                    
                });
            });
            strip.empty();
            strip
                .strip(|strip| 
            {
                strip
                    .size(Size::remainder())
                    .horizontal( |mut strip| 
                {
                    strip.cell(|_ui|
                    {

                    });
                });
            });
        });
        None
    }

    fn set_state(mut self, action: ModalAction){

    }
}
