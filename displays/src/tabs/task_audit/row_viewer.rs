use crate::{channel_manager::ChannelManager, chats::ChatView, Spawner};
use eframe::egui::{Color32, Hyperlink, KeyboardShortcut, Label, Widget};
use database::schema::{helper_traits::parse_email_user, prestashop::OrderState, prestashop_schema::{MissedCallOrder, PrestashopPayload}, TaskNotePayload};
use chrono::{DateTime, NaiveDateTime, Utc};
use egui_data_table::{viewer::{default_hotkeys, RowCodec, UiActionContext}, RowViewer, UiAction};
use crate::PlatformSpawner;
use crossbeam::channel::{Receiver, Sender};
use egui_extras::Column;

use super::codec::Codec;

pub const BASE_URL: &str = "https://pclaptops.mojo11.com/pcladmin/index.php?controller=AdminOrders&vieworder=&id_order=";

#[derive(serde::Serialize)]
pub struct TaskRowViewer {
    pub filter: String,
    row_protection: bool,
    #[serde(skip)]
    pub hotkeys: Vec<(KeyboardShortcut, UiAction)>,
    pub selected: Option<PrestashopPayload>,
    order_data: PrestashopPayload,
    pub open_hotkeys: bool,
    pub chat_view: ChatView,
    #[serde(skip)]
    pub notes_channel: (Sender<Vec<TaskNotePayload>>, Receiver<Vec<TaskNotePayload>>),
    #[serde(skip)]
    pub tur_channel: (Sender<PrestashopPayload>, Receiver<PrestashopPayload>),
    pub missed_calls: Vec<MissedCallOrder>,
    pub store_selection: u64,
}

impl Default for TaskRowViewer {
    fn default() -> Self {
        let notes_channel = <Vec<TaskNotePayload>>::create_unbounded_channel();
        let tur_channel = PrestashopPayload::create_unbounded_channel();
        Self {
            notes_channel,
            tur_channel,
            filter: Default::default(),
            row_protection: Default::default(),
            hotkeys: Default::default(),
            selected: Default::default(),
            open_hotkeys: Default::default(),
            chat_view: ChatView::default(),
            order_data: PrestashopPayload::default(),
            missed_calls: Vec::new(),
            store_selection: 76,
        }
    }
}

impl RowViewer<PrestashopPayload> for TaskRowViewer {
    fn try_create_codec(&mut self, _: bool) -> Option<impl RowCodec<PrestashopPayload>> {
        Some(Codec)
    }

    fn num_columns(&mut self) -> usize {
        10
    }

    fn column_name(&mut self, column: usize) -> std::borrow::Cow<'static, str> {
        ["Order #", "Customer Name", "Date", "Status", "Sales Rep", "Split Rep", "# Missed Calls", "Device", "Model", "Checkin Notes"][column].into()
    }

    fn is_sortable_column(&mut self, column: usize) -> bool {
        [true, true, true, true, true, true, true, true, true, true][column]
    }

    fn row_filter_hash(&mut self) -> &impl std::hash::Hash {
        &self.filter
    }

    fn filter_row(&mut self, row: &PrestashopPayload) -> bool {
        row.order.id.contains(&self.filter) 
        || row.customer.name.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.sales_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().firstname.to_lowercase().contains(&self.filter)
        || row.split_rep.clone().unwrap_or_default().lastname.to_lowercase().contains(&self.filter)
    }

    fn hotkeys(&mut self, context: &UiActionContext) -> Vec<(KeyboardShortcut, UiAction)> {
        let hotkeys = default_hotkeys(context);
        self.hotkeys.clone_from(&hotkeys);
        hotkeys
    }

    fn show_cell_view(&mut self, ui: &mut eframe::egui::Ui, row: &PrestashopPayload, column: usize) {
        let _ = match column {
            0 => { ui.colored_label(ui.style().visuals.warn_fg_color, format!(" {}", row.order.id.clone())); },
            1 => { ui.label(format!(" {}", row.customer.name.clone())); },
            2 => {
                // Parse the input into a NaiveDateTime
                let naive_datetime = NaiveDateTime::parse_from_str(&row.order.date_add, "%Y-%m-%d %H:%M:%S").unwrap_or_default();

                // Convert to a DateTime with Utc timezone
                let datetime: DateTime<Utc> = DateTime::from_naive_utc_and_offset(naive_datetime, Utc);

                // Format the DateTime into yyyy/mm/dd
                let formatted_date = datetime.format(" %m/%d/%Y").to_string();
                let split1 = formatted_date.split_once('/').unwrap_or_default();
                let split2 = split1.1.split_once('/').unwrap_or_default();
                ui.horizontal(|ui| {
                    ui.colored_label(Color32::from_rgb(42, 195, 222), format!("{}/", split1.0));
                    ui.colored_label(ui.style().visuals.error_fg_color, format!("{}/", split2.0));
                    ui.colored_label(ui.style().visuals.warn_fg_color, split2.1)
                }).inner;
            },
            3 => { ui.label(format!(" {}", OrderState::from_id_str(&row.order.current_state))); },
            4 => {
                let emp = row.sales_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"));
            },
            5 => {
                let emp = row.split_rep.clone().unwrap_or_default();
                let split = parse_email_user(&emp.email);
                ui.label(format!(" {split}"));
            },
            6 => {
                let call = self.missed_calls.iter().find(|o| o.id == row.order.id).cloned();
                if let Some(missed_call) = call {
                    let num = missed_call.missing_days.len();
                    let txt = if num == 1 {
                        format!(" {num} Missed Call")
                    } else {
                        format!(" {num} Missed Calls")
                    };
                    ui.colored_label(ui.style().visuals.error_fg_color, txt);
                }
            },
            7 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg)); },
            8 => { ui.label(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model)); },
            9 => { Label::new(format!(" {}", row.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone())).wrap().ui(ui); },
            _ => {},
        };
    }

    fn column_render_config(&mut self, column: usize, _is_last_visible_column: bool) -> Column {
        let col_config = Column::auto();
        match column {
            0 => col_config.resizable(true).at_least(60.).at_most(60.),
            1 => col_config.resizable(true).at_least(180.).at_most(225.),
            2 => col_config.resizable(true).at_least(90.).at_most(100.),
            3 => col_config.resizable(true).at_least(130.).at_most(130.),
            4 => col_config.resizable(true).at_least(100.).at_most(150.),
            5 => col_config.resizable(true).at_least(100.).at_most(150.),
            6 => col_config.resizable(true).at_least(100.).at_most(150.),
            7 => col_config.resizable(true).at_least(100.).at_most(150.),
            8 => col_config.resizable(true).at_least(100.).at_most(150.),
            9 => col_config.resizable(true).at_least(150.),
            _ => col_config,
        }
    }
    
    fn show_cell_editor(
        &mut self,
        ui: &mut eframe::egui::Ui,
        row: &mut PrestashopPayload,
        column: usize,
    ) -> Option<eframe::egui::Response> {
        match column {
            0 => Some(
                Hyperlink::from_label_and_url(
                    format!(" {}", row.order.id.clone()), 
                    format!("{BASE_URL}{}", row.order.id.clone())
                )
                .open_in_new_tab(true)
                .ui(ui)
            ),
            _ => None,
        }
    }

    fn on_cell_view_response(
        &mut self,
        row: &PrestashopPayload,
        column: usize,
        resp: &eframe::egui::Response,
    ) -> Option<Box<PrestashopPayload>> {
        match column {
            0 | 1 => {
                if resp.clicked() {
                    log::info!("Clicked Col/Row: {column}/{}", row.order.id);
                    self.chat_view.messages.clear();
                    self.selected = Some(row.clone());
                    let notes_tx = self.notes_channel.0.clone();
                    let service_number = row.order.id.clone();
                    PlatformSpawner::spawn(async move {
                        match Self::get_order_notes(service_number).await {
                            Ok(notes) => notes_tx.try_send(notes).unwrap(),
                            Err(e) => log::error!("Error {e:?}"),
                        };
                    });
                }
            },
            _ => {}
        }
    
        resp
            .clone()
            .on_hover_and_drag_cursor(eframe::egui::CursorIcon::Crosshair)
            .dnd_release_payload::<String>()
            .map(|_| Box::new(PrestashopPayload::default()))
    }

    fn set_cell_value(
        &mut self,
        src: &PrestashopPayload,
        dst: &mut PrestashopPayload,
        column: usize,
    ) {
        match column {
            0 => dst.order.id = src.order.id.clone(),
            1 => dst.customer.name = src.customer.name.clone(),
            2 => dst.order.date_add = src.order.date_add.clone(),
            3 => dst.order.current_state = src.order.current_state.clone(),
            4 => dst.sales_rep = src.sales_rep.clone(),
            5 => dst.split_rep = src.split_rep.clone(),
            7 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().device_mfg.clone()
            8 => dst.sales_rep = src.sales_rep.clone(),
            9 => dst.order.associations = src.order.associations.clone(), // order_service.get(0).cloned().unwrap_or_default().check_in_notes.clone()
            _ => {},
        }
    }

    fn compare_cell(
        &self,
        row_l: &PrestashopPayload,
        row_r: &PrestashopPayload,
        column: usize,
    ) -> std::cmp::Ordering {
        match column {
            0 => row_l.order.id.cmp(&row_r.order.id),
            1 => row_l.customer.name.cmp(&row_r.customer.name),
            2 => row_l.order.date_add.cmp(&row_r.order.date_add),
            3 => row_l.order.current_state.cmp(&row_r.order.current_state),
            4 => {
                let emp = row_l.sales_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.sales_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            5 => {
                let emp = row_l.split_rep.clone().unwrap_or_default();
                let name = format!("{} {}", emp.firstname, emp.lastname);
                let emp1 = row_r.split_rep.clone().unwrap_or_default();
                let name1 = format!("{} {}", emp1.firstname, emp1.lastname);
                name.cmp(&name1)
            },
            6 => {
                let call_l = self.missed_calls.iter().find(|o| o.id == row_l.order.id).cloned();
                let call_r = self.missed_calls.iter().find(|o| o.id == row_r.order.id).cloned();
                if let (Some(missed_l), Some(missed_r)) = (call_l, call_r) {
                    missed_l.missing_days.len().cmp(&missed_r.missing_days.len())
                } else {
                    std::cmp::Ordering::Equal
                }
            }
            7 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_mfg),
            8 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().device_model),
            9 => row_l.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes.cmp(&row_r.order.associations.order_service.get(0).cloned().unwrap_or_default().check_in_notes),
            _ => std::cmp::Ordering::Equal
        }
    }

    fn new_empty_row(&mut self) -> PrestashopPayload {
        // Instead of requiring `Default` trait for row data types, the viewer is
        // responsible of providing default creation method.
        PrestashopPayload::default()
    }
}