use ratatui::crossterm::event::ModifierKeyCode;

#[derive(Debug, Clone)]
pub enum AppEvent {
    Tick,
    Resize(u16, u16),
    Shutdown,
    /// A key event.
    StartupAnimation,
    SelectedCategoryFxSandbox,
    AutoSelectCategory,
    DeselectCategory,
    NextCategory,
    PreviousCategory,
    ToggleFilterKey(ModifierKeyCode),
    CategoryWidgetNavigationOrder(Vec<usize>)
}
