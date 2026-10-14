use database::schema::user::{User, UserAuthorization};
use eframe::egui::WidgetText;
use serde::{Deserialize, Serialize};

use crate::ui_tools::{
    controls,
    icons::svg::{self, Icon},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TabId {
    TurSheet,
    PartOrder,
    Koth,
    Scripts,
    MyTools,
    FileBrowser,
    SysInfo,
    MinidumpAnalysis,
    Ai,
    StoreTasks,
    MyTasks,
    CompletedTasks,
    BugReport,
    Downloads,
    TaskAudit,
    Inventory,
    SalesTracker,
    Logs,
    ResourceMonitor,
    AdminConsole,
    QueryEditor,
    CreateOrder,
    Threads,
    Plugins,
    DatabaseEditor,
    FleetDashboard,
    StressLab,
    StressTest,
    Terminal,
    ShopifyOrders,
    ServerConsole,
    AgentAudit,
    AiAnalytics,
    SessionBoard,
    UserAdmin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabContext {
    MastertechNative,
    MtechServerWasm,
    WarehouseNative,
    WarehouseWasm,
}

impl TabContext {
    pub fn for_user(is_warehouse: bool) -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            if is_warehouse {
                Self::WarehouseWasm
            } else {
                Self::MtechServerWasm
            }
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            if is_warehouse {
                Self::WarehouseNative
            } else {
                Self::MastertechNative
            }
        }
    }
}

const MT_SERVER_WASM: &[TabId] = &[
    TabId::MyTasks,
    TabId::StoreTasks,
    TabId::CompletedTasks,
    TabId::SalesTracker,
    TabId::Inventory,
    TabId::TaskAudit,
    TabId::Threads,
    TabId::Ai,
    TabId::BugReport,
    TabId::FileBrowser,
    TabId::Logs,
    TabId::AdminConsole,
    TabId::DatabaseEditor,
    TabId::QueryEditor,
    TabId::Koth,
    TabId::CreateOrder,
    TabId::StressLab,
    TabId::ServerConsole,
    TabId::UserAdmin,
];

const MT_NATIVE: &[TabId] = &[
    TabId::TurSheet,
    TabId::PartOrder,
    TabId::Koth,
    TabId::Scripts,
    TabId::FileBrowser,
    TabId::MinidumpAnalysis,
    TabId::Ai,
    TabId::StoreTasks,
    TabId::MyTasks,
    TabId::CompletedTasks,
    TabId::BugReport,
    TabId::Downloads,
    TabId::TaskAudit,
    TabId::Inventory,
    TabId::SalesTracker,
    TabId::Logs,
    TabId::ResourceMonitor,
    TabId::AdminConsole,
    TabId::QueryEditor,
    TabId::CreateOrder,
    TabId::ShopifyOrders,
    TabId::Threads,
    TabId::Plugins,
    TabId::DatabaseEditor,
    TabId::FleetDashboard,
    TabId::StressLab,
    TabId::StressTest,
    TabId::Terminal,
    TabId::ServerConsole,
    TabId::AgentAudit,
    TabId::AiAnalytics,
    TabId::SessionBoard,
    TabId::UserAdmin,
];

const WH_WASM: &[TabId] = &[
    TabId::FleetDashboard,
    TabId::AdminConsole,
    TabId::Logs,
    TabId::FileBrowser,
    TabId::Threads,
];

const WH_NATIVE: &[TabId] = &[
    TabId::FleetDashboard,
    TabId::AdminConsole,
    TabId::ResourceMonitor,
    TabId::Logs,
    TabId::FileBrowser,
    TabId::Scripts,
    TabId::Plugins,
    TabId::Threads,
];

pub const WAREHOUSE_DEFAULT_OPEN: &[TabId] = &[TabId::FleetDashboard, TabId::AdminConsole];

/// Which tabs the signed-in user may open: Root-only tabs and the ones an admin hid from them.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TabAccess {
    is_root: bool,
    hidden: Vec<TabId>,
}

impl TabAccess {
    /// Access for `user`; an active Root sees every tab and no user sees no Root tab.
    pub fn for_user(user: Option<&User>) -> Self {
        let Some(user) = user else {
            return Self::default();
        };
        let is_root = user.get_authorization() == UserAuthorization::Root && user.is_active();
        let hidden = if is_root {
            Vec::new()
        } else {
            user.hidden_tabs()
                .iter()
                .filter_map(|slug| TabId::from_slug(slug))
                .collect()
        };
        Self { is_root, hidden }
    }

    /// Access for the signed-in user.
    pub fn current() -> Self {
        Self::for_user(crate::get_current_user_from_auth().as_ref())
    }

    pub fn is_root(&self) -> bool {
        self.is_root
    }

    pub fn allows(&self, tab: TabId) -> bool {
        (self.is_root || !tab.requires_root()) && !self.hidden.contains(&tab)
    }
}

impl TabId {
    pub fn slug(self) -> &'static str {
        match self {
            Self::SessionBoard => "session_board",
            Self::TurSheet => "tur_sheet",
            Self::PartOrder => "part_order",
            Self::Koth => "koth",
            Self::Scripts => "scripts",
            Self::MyTools => "my_tools",
            Self::FileBrowser => "file_browser",
            Self::SysInfo => "sysinfo",
            Self::MinidumpAnalysis => "minidump_analysis",
            Self::Ai => "ai",
            Self::StoreTasks => "store_tasks",
            Self::MyTasks => "my_tasks",
            Self::CompletedTasks => "completed_tasks",
            Self::BugReport => "bug_report",
            Self::Downloads => "downloads",
            Self::TaskAudit => "task_audit",
            Self::Inventory => "inventory",
            Self::SalesTracker => "sales_tracker",
            Self::Logs => "logs",
            Self::ResourceMonitor => "resource_monitor",
            Self::AdminConsole => "admin_console",
            Self::QueryEditor => "query_editor",
            Self::CreateOrder => "create_order",
            Self::Threads => "threads",
            Self::Plugins => "plugins",
            Self::DatabaseEditor => "database_editor",
            Self::FleetDashboard => "fleet_dashboard",
            Self::StressLab => "stress_lab",
            Self::StressTest => "stress_test",
            Self::Terminal => "terminal",
            Self::ShopifyOrders => "shopify_orders",
            Self::ServerConsole => "server_console",
            Self::AgentAudit => "agent_audit",
            Self::AiAnalytics => "ai_analytics",
            Self::UserAdmin => "user_admin",
        }
    }

    /// Tabs hidden from the View menu and refused at render for anyone but Root.
    pub fn requires_root(self) -> bool {
        matches!(self, Self::ServerConsole | Self::UserAdmin)
    }

    /// Tabs an admin may hide from a user: every listed tab except the Root-only ones.
    pub fn hideable() -> Vec<TabId> {
        let mut tabs: Vec<TabId> = Vec::new();
        for list in [MT_NATIVE, MT_SERVER_WASM, WH_NATIVE, WH_WASM] {
            for tab in list {
                if !tab.requires_root() && !tabs.contains(tab) {
                    tabs.push(*tab);
                }
            }
        }
        tabs.sort_by_key(|t| t.title(TabContext::MastertechNative));
        tabs
    }

    /// Icon drawn in front of the tab's title.
    pub fn icon(self) -> Icon {
        match self {
            Self::SessionBoard => svg::KANBAN,
            Self::TurSheet => svg::CLIPBOARD_TEXT,
            Self::PartOrder => svg::PACKAGE,
            Self::Koth => svg::TROPHY,
            Self::Scripts => svg::SCROLL,
            Self::MyTools => svg::TOOLBOX,
            Self::FileBrowser => svg::FOLDER_OPEN,
            Self::SysInfo => svg::CIRCUITRY,
            Self::MinidumpAnalysis => svg::BUG_BEETLE,
            Self::Ai => svg::SPARKLE,
            Self::StoreTasks => svg::STOREFRONT,
            Self::MyTasks => svg::LIST_CHECKS,
            Self::CompletedTasks => svg::CHECK_CIRCLE,
            Self::BugReport => svg::BUG,
            Self::Downloads => svg::DOWNLOAD_SIMPLE,
            Self::TaskAudit => svg::LIST_MAGNIFYING_GLASS,
            Self::Inventory => svg::BARCODE,
            Self::SalesTracker => svg::CHART_LINE_UP,
            Self::Logs => svg::LIST_BULLETS,
            Self::ResourceMonitor => svg::GAUGE,
            Self::AdminConsole => svg::SHIELD_CHECK,
            Self::QueryEditor => svg::CODE_BLOCK,
            Self::CreateOrder => svg::SHOPPING_CART,
            Self::Threads => svg::CHAT_CIRCLE_DOTS,
            Self::Plugins => svg::MT_PLUGIN,
            Self::DatabaseEditor => svg::DATABASE,
            Self::FleetDashboard => svg::HARD_DRIVES,
            Self::StressLab => svg::FLASK,
            Self::StressTest => svg::MT_STRESS,
            Self::Terminal => svg::TERMINAL_WINDOW,
            Self::ShopifyOrders => svg::SHOPPING_BAG,
            Self::ServerConsole => svg::TERMINAL,
            Self::AgentAudit => svg::ROBOT,
            Self::AiAnalytics => svg::CHART_BAR,
            Self::UserAdmin => svg::USER_GEAR,
        }
    }

    /// The tab's icon and title as one label, for tab strips and menus.
    pub fn label(self, ctx: TabContext) -> WidgetText {
        controls::tab_title(self.icon(), self.title(ctx))
    }

    pub fn title(self, ctx: TabContext) -> &'static str {
        match self {
            Self::SessionBoard => "Session Board",
            Self::TurSheet => "TUR Sheet",
            Self::PartOrder => "Part Order",
            Self::Koth => "KOTH",
            Self::Scripts => "Scripts",
            Self::MyTools => "My Tools",
            Self::FileBrowser => "File Browser",
            Self::SysInfo => "SysInfo",
            Self::MinidumpAnalysis => "Minidump Analysis",
            Self::Ai => "Ai",
            Self::StoreTasks => "Store Tasks",
            Self::MyTasks => "My Tasks",
            Self::CompletedTasks => "Completed Tasks",
            Self::BugReport => match ctx {
                TabContext::MtechServerWasm | TabContext::WarehouseWasm => "Bug Report",
                _ => "Bug Tracker",
            },
            Self::Downloads => "Downloads",
            Self::TaskAudit => "Task Audit",
            Self::Inventory => "Inventory",
            Self::SalesTracker => "Sales Tracker",
            Self::Logs => "Logs",
            Self::ResourceMonitor => "Resource Monitor",
            Self::AdminConsole => "Admin Console",
            Self::QueryEditor => "Query Editor",
            Self::CreateOrder => "Create Order",
            Self::Threads => "Threads",
            Self::Plugins => "Plugins",
            Self::DatabaseEditor => "Database Editor",
            Self::FleetDashboard => "Fleet Dashboard",
            Self::StressLab => "Stress Lab",
            Self::StressTest => "Stress Test",
            Self::Terminal => "Terminal",
            Self::ShopifyOrders => "Shopify Orders",
            Self::ServerConsole => "Server Console",
            Self::AgentAudit => "Agent Audit",
            Self::AiAnalytics => "AI Analytics",
            Self::UserAdmin => "User Admin",
        }
    }

    /// [`visible_for`] filtered to the tabs `access` allows.
    pub fn visible_for_user(ctx: TabContext, access: &TabAccess) -> Vec<TabId> {
        Self::visible_for(ctx)
            .iter()
            .copied()
            .filter(|t| access.allows(*t))
            .collect()
    }

    pub fn visible_for(ctx: TabContext) -> &'static [TabId] {
        match ctx {
            TabContext::MastertechNative => MT_NATIVE,
            TabContext::MtechServerWasm => MT_SERVER_WASM,
            TabContext::WarehouseNative => WH_NATIVE,
            TabContext::WarehouseWasm => WH_WASM,
        }
    }

    pub fn from_legacy_title(s: &str) -> Option<Self> {
        if let Some(id) = Self::from_slug(s) {
            return Some(id);
        }
        match s {
            "TUR Sheet" => Some(Self::TurSheet),
            "Part Order" => Some(Self::PartOrder),
            "KOTH" | "Koth" => Some(Self::Koth),
            "Scripts" => Some(Self::Scripts),
            "My Tools" => Some(Self::FileBrowser),
            "File Browser" | "File Browser 📂" => Some(Self::FileBrowser),
            "SysInfo" => Some(Self::ResourceMonitor),
            "Minidump Analysis" => Some(Self::MinidumpAnalysis),
            "QC ☑️" | "Qc" => None,
            "Ai" => Some(Self::Ai),
            "Store Tasks" => Some(Self::StoreTasks),
            "My Tasks" => Some(Self::MyTasks),
            "Completed Tasks" => Some(Self::CompletedTasks),
            "Bug Tracker" | "Bug Report" | "BugReport" => Some(Self::BugReport),
            "Websockets" | "Scene Editor" | "Web Console" | "ZeroClaw" | "Agent Sessions" => None,
            "Downloads" => Some(Self::Downloads),
            "Task Audit" | "TaskAudit" => Some(Self::TaskAudit),
            "Inventory" => Some(Self::Inventory),
            "Sales Tracker" => Some(Self::SalesTracker),
            "Logs" => Some(Self::Logs),
            "Resource Monitor" => Some(Self::ResourceMonitor),
            "Admin Console" => Some(Self::AdminConsole),
            "Query Editor" => Some(Self::QueryEditor),
            // "Create Prestashop Order" is still in saved dock layouts.
            "Create Order" | "Create Prestashop Order" => Some(Self::CreateOrder),
            "Session Board" => Some(Self::SessionBoard),
            "Threads" => Some(Self::Threads),
            "Plugins" => Some(Self::Plugins),
            "Database Editor" | "Database" => Some(Self::DatabaseEditor),
            "Fleet Dashboard" => Some(Self::FleetDashboard),
            "Stress Lab" => Some(Self::StressLab),
            "Stress Test" => Some(Self::StressTest),
            "Terminal" => Some(Self::Terminal),
            "Shopify Orders" => Some(Self::ShopifyOrders),
            "Server Console" => Some(Self::ServerConsole),
            "Agent Audit" => Some(Self::AgentAudit),
            "AI Analytics" => Some(Self::AiAnalytics),
            "User Admin" => Some(Self::UserAdmin),
            _ => None,
        }
    }

    pub fn from_slug(s: &str) -> Option<Self> {
        match s {
            "websockets" | "scene_editor" | "web_console" | "zeroclaw" | "zero_claw" | "agent_sessions" => None,
            "tur_sheet" => Some(Self::TurSheet),
            "part_order" => Some(Self::PartOrder),
            "koth" => Some(Self::Koth),
            "scripts" => Some(Self::Scripts),
            "my_tools" => Some(Self::FileBrowser),
            "file_browser" => Some(Self::FileBrowser),
            "sysinfo" => Some(Self::ResourceMonitor),
            "minidump_analysis" => Some(Self::MinidumpAnalysis),
            "qc" => None,
            "ai" => Some(Self::Ai),
            "store_tasks" => Some(Self::StoreTasks),
            "my_tasks" => Some(Self::MyTasks),
            "completed_tasks" => Some(Self::CompletedTasks),
            "bug_report" => Some(Self::BugReport),
            "downloads" => Some(Self::Downloads),
            "task_audit" => Some(Self::TaskAudit),
            "inventory" => Some(Self::Inventory),
            "sales_tracker" => Some(Self::SalesTracker),
            "logs" => Some(Self::Logs),
            "resource_monitor" => Some(Self::ResourceMonitor),
            "admin_console" => Some(Self::AdminConsole),
            "query_editor" => Some(Self::QueryEditor),
            "create_order" | "create_prestashop_order" => Some(Self::CreateOrder),
            "threads" => Some(Self::Threads),
            "plugins" => Some(Self::Plugins),
            "database_editor" => Some(Self::DatabaseEditor),
            "fleet_dashboard" => Some(Self::FleetDashboard),
            "session_board" => Some(Self::SessionBoard),
            "stress_lab" => Some(Self::StressLab),
            "stress_test" => Some(Self::StressTest),
            "terminal" => Some(Self::Terminal),
            "shopify_orders" => Some(Self::ShopifyOrders),
            "server_console" => Some(Self::ServerConsole),
            "agent_audit" => Some(Self::AgentAudit),
            "ai_analytics" => Some(Self::AiAnalytics),
            "user_admin" => Some(Self::UserAdmin),
            _ => None,
        }
    }

    pub fn layout_page_name(self) -> &'static str {
        match self {
            Self::MyTasks => "My Tasks",
            Self::StoreTasks => "Store Tasks",
            Self::CompletedTasks => "Completed Tasks",
            _ => self.title(TabContext::MastertechNative),
        }
    }
}

#[cfg(test)]
mod tab_id_tests {
    use super::*;

    /// The slug is stored in saved dock layouts, so retiring the old one has to
    /// keep parsing it — otherwise every layout saved before the rename loses
    /// this tab.
    #[test]
    fn the_renamed_tab_still_loads_old_layouts() {
        assert_eq!(TabId::CreateOrder.slug(), "create_order");
        assert_eq!(TabId::from_slug("create_order"), Some(TabId::CreateOrder));
        assert_eq!(
            TabId::from_slug("create_prestashop_order"),
            Some(TabId::CreateOrder),
            "layouts saved before the rename must still resolve"
        );
        assert_eq!(
            TabId::from_legacy_title("Create Prestashop Order"),
            Some(TabId::CreateOrder),
            "the old display title is also persisted in some layouts"
        );
        assert_eq!(TabId::from_legacy_title("Create Order"), Some(TabId::CreateOrder));
    }

    /// A slug that round-trips is what keeps a layout stable across restarts.
    #[test]
    fn every_tab_slug_round_trips() {
        for ctx in [
            TabContext::MastertechNative,
            TabContext::MtechServerWasm,
            TabContext::WarehouseNative,
            TabContext::WarehouseWasm,
        ] {
            for id in TabId::visible_for(ctx) {
                assert_eq!(
                    TabId::from_slug(id.slug()),
                    Some(*id),
                    "slug {:?} does not round-trip",
                    id.slug()
                );
            }
        }
    }

    /// A retired tab must stop resolving, so old layouts drop it instead of keeping a dead pane.
    #[test]
    fn the_retired_qc_tab_is_dropped_from_old_layouts() {
        assert_eq!(TabId::from_slug("qc"), None);
        assert_eq!(TabId::from_legacy_title("qc"), None);
        assert_eq!(TabId::from_legacy_title("QC ☑️"), None);
        assert_eq!(TabId::from_legacy_title("Qc"), None);
        assert!(
            !TabId::visible_for(TabContext::MastertechNative)
                .iter()
                .any(|t| t.slug() == "qc"),
            "the retired tab must not be listed in any visible set"
        );
    }

    fn user(role: UserAuthorization, active: bool, hidden: &[&str]) -> User {
        let mut value = surrealdb_types::SurrealValue::into_value(User::default());
        if let surrealdb_types::Value::Object(ref mut row) = value {
            row.insert(
                "authorization".to_string(),
                surrealdb_types::Value::String(role.as_str().into()),
            );
            row.insert("active".to_string(), surrealdb_types::Value::Bool(active));
            row.insert(
                "hidden_tabs".to_string(),
                surrealdb_types::SurrealValue::into_value(
                    hidden.iter().map(|s| s.to_string()).collect::<Vec<_>>(),
                ),
            );
        }
        surrealdb_types::SurrealValue::from_value(value).expect("user decodes")
    }

    #[test]
    fn root_only_tabs_need_an_active_root() {
        let root = TabAccess::for_user(Some(&user(UserAuthorization::Root, true, &[])));
        let off = TabAccess::for_user(Some(&user(UserAuthorization::Root, false, &[])));
        let tech = TabAccess::for_user(Some(&user(UserAuthorization::User, true, &[])));
        for tab in [TabId::ServerConsole, TabId::UserAdmin] {
            assert!(root.allows(tab), "{tab:?}");
            assert!(!off.allows(tab), "{tab:?}");
            assert!(!tech.allows(tab), "{tab:?}");
            assert!(!TabAccess::default().allows(tab), "{tab:?}");
        }
    }

    #[test]
    fn hidden_tabs_are_refused_except_for_root() {
        let hidden = ["stress_lab", "terminal", "not_a_tab"];
        let tech = TabAccess::for_user(Some(&user(UserAuthorization::User, true, &hidden)));
        assert!(!tech.allows(TabId::StressLab));
        assert!(!tech.allows(TabId::Terminal));
        assert!(tech.allows(TabId::MyTasks));
        let visible = TabId::visible_for_user(TabContext::MastertechNative, &tech);
        assert!(!visible.contains(&TabId::StressLab));
        assert!(visible.contains(&TabId::Ai));
        let root = TabAccess::for_user(Some(&user(UserAuthorization::Root, true, &hidden)));
        assert!(root.allows(TabId::StressLab));
    }

    #[test]
    fn hideable_tabs_exclude_root_only_ones_and_repeat_none() {
        let tabs = TabId::hideable();
        assert!(!tabs.iter().any(|t| t.requires_root()));
        let unique: std::collections::HashSet<TabId> = tabs.iter().copied().collect();
        assert_eq!(unique.len(), tabs.len());
        for list in [MT_NATIVE, MT_SERVER_WASM, WH_NATIVE, WH_WASM] {
            for tab in list.iter().filter(|t| !t.requires_root()) {
                assert!(tabs.contains(tab), "{tab:?} cannot be hidden");
            }
        }
    }

    /// The ZeroClaw and Agent Sessions tabs moved into the Ai tab, so layouts that still name them drop the pane.
    #[test]
    fn the_retired_agent_tabs_are_dropped_from_old_layouts() {
        for name in ["zeroclaw", "zero_claw", "ZeroClaw", "agent_sessions", "Agent Sessions"] {
            assert_eq!(TabId::from_legacy_title(name), None, "{name}");
        }
        for slug in ["zeroclaw", "agent_sessions"] {
            assert!(!TabId::visible_for(TabContext::MastertechNative).iter().any(|t| t.slug() == slug), "{slug}");
        }
    }
}
