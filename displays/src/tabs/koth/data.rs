use eframe::egui::{Button, CentralPanel, Color32, ComboBox, FontId, Grid, Hyperlink, Id, RichText, ScrollArea, TopBottomPanel, Ui, Vec2, Widget};
use database::schema::{prestashop::{generate_orders_report, get_order_payments, Order, OrderPayment, OrderState, PayPeriod}, User};
use crate::{get_current_user_from_auth, modals::tabs::return_colors, tabs::koth::row_viewer::KothRowViewer, PlatformSpawner, Spawner};
use crate::tabs::task_audit::row_viewer::BASE_URL;
use crossbeam::channel::{Receiver, Sender};
use chrono::NaiveDateTime;
use itertools::Itertools;
use std::f32;

#[derive(Default, serde::Serialize)]
pub struct KothTableData {
    order: Order,
    order_payment: OrderPayment
}