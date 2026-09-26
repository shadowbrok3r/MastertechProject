use database::schema::Store;
use eframe::egui::Ui;

/// Adds one selectable entry per store, valued by its PrestaShop store id.
pub fn presta_store_options(ui: &mut Ui, selected: &mut u64, stores: &[Store]) {
    for store in stores {
        ui.selectable_value(selected, store.into_store_id() as u64, store.as_str());
    }
}
