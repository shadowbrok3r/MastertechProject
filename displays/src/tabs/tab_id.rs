use serde::{Deserialize, Serialize};

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
    Qc,
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
    WebConsole,
    QueryEditor,
    CreatePrestashopOrder,
    Threads,
    Plugins,
    DatabaseEditor,
    FleetDashboard,
    StressLab,
    StressTest,
    Terminal,
    ShopifyOrders,
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
    TabId::BugReport,
    TabId::FileBrowser,
    TabId::Logs,
    TabId::AdminConsole,
    TabId::WebConsole,
    TabId::DatabaseEditor,
    TabId::QueryEditor,
    TabId::Koth,
    TabId::CreatePrestashopOrder,
    TabId::StressLab,
];

const MT_NATIVE: &[TabId] = &[
    TabId::TurSheet,
    TabId::PartOrder,
    TabId::Koth,
    TabId::Scripts,
    TabId::FileBrowser,
    TabId::MinidumpAnalysis,
    TabId::Qc,
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
    TabId::WebConsole,
    TabId::QueryEditor,
    TabId::CreatePrestashopOrder,
    TabId::ShopifyOrders,
    TabId::Threads,
    TabId::Plugins,
    TabId::DatabaseEditor,
    TabId::FleetDashboard,
    TabId::StressLab,
    TabId::StressTest,
    TabId::Terminal,
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
    TabId::WebConsole,
    TabId::ResourceMonitor,
    TabId::Logs,
    TabId::FileBrowser,
    TabId::Scripts,
    TabId::Plugins,
    TabId::Threads,
];

pub const WAREHOUSE_DEFAULT_OPEN: &[TabId] = &[TabId::FleetDashboard, TabId::AdminConsole];

impl TabId {
    pub fn slug(self) -> &'static str {
        match self {
            Self::TurSheet => "tur_sheet",
            Self::PartOrder => "part_order",
            Self::Koth => "koth",
            Self::Scripts => "scripts",
            Self::MyTools => "my_tools",
            Self::FileBrowser => "file_browser",
            Self::SysInfo => "sysinfo",
            Self::MinidumpAnalysis => "minidump_analysis",
            Self::Qc => "qc",
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
            Self::WebConsole => "web_console",
            Self::QueryEditor => "query_editor",
            Self::CreatePrestashopOrder => "create_prestashop_order",
            Self::Threads => "threads",
            Self::Plugins => "plugins",
            Self::DatabaseEditor => "database_editor",
            Self::FleetDashboard => "fleet_dashboard",
            Self::StressLab => "stress_lab",
            Self::StressTest => "stress_test",
            Self::Terminal => "terminal",
            Self::ShopifyOrders => "shopify_orders",
        }
    }

    pub fn title(self, ctx: TabContext) -> &'static str {
        match self {
            Self::TurSheet => "TUR Sheet",
            Self::PartOrder => "Part Order",
            Self::Koth => "KOTH",
            Self::Scripts => "Scripts",
            Self::MyTools => "My Tools",
            Self::FileBrowser => "File Browser 📂",
            Self::SysInfo => "SysInfo",
            Self::MinidumpAnalysis => "Minidump Analysis",
            Self::Qc => "QC ☑️",
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
            Self::WebConsole => "Web Console",
            Self::QueryEditor => "Query Editor",
            Self::CreatePrestashopOrder => "Create Prestashop Order",
            Self::Threads => "Threads",
            Self::Plugins => "Plugins",
            Self::DatabaseEditor => "Database Editor",
            Self::FleetDashboard => "Fleet Dashboard",
            Self::StressLab => "Stress Lab",
            Self::StressTest => "Stress Test",
            Self::Terminal => "Terminal",
            Self::ShopifyOrders => "Shopify Orders",
        }
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
            "File Browser 📂" => Some(Self::FileBrowser),
            "SysInfo" => Some(Self::ResourceMonitor),
            "Minidump Analysis" => Some(Self::MinidumpAnalysis),
            "QC ☑️" | "Qc" => Some(Self::Qc),
            "Ai" => Some(Self::Ai),
            "Store Tasks" => Some(Self::StoreTasks),
            "My Tasks" => Some(Self::MyTasks),
            "Completed Tasks" => Some(Self::CompletedTasks),
            "Bug Tracker" | "Bug Report" | "BugReport" => Some(Self::BugReport),
            "Websockets" | "Scene Editor" => None,
            "Downloads" => Some(Self::Downloads),
            "Task Audit" | "TaskAudit" => Some(Self::TaskAudit),
            "Inventory" => Some(Self::Inventory),
            "Sales Tracker" => Some(Self::SalesTracker),
            "Logs" => Some(Self::Logs),
            "Resource Monitor" => Some(Self::ResourceMonitor),
            "Admin Console" => Some(Self::AdminConsole),
            "Web Console" => Some(Self::WebConsole),
            "Query Editor" => Some(Self::QueryEditor),
            "Create Prestashop Order" => Some(Self::CreatePrestashopOrder),
            "Threads" => Some(Self::Threads),
            "Plugins" => Some(Self::Plugins),
            "Database Editor" | "Database" => Some(Self::DatabaseEditor),
            "Fleet Dashboard" => Some(Self::FleetDashboard),
            "Stress Lab" => Some(Self::StressLab),
            "Stress Test" => Some(Self::StressTest),
            "Terminal" => Some(Self::Terminal),
            "Shopify Orders" => Some(Self::ShopifyOrders),
            _ => None,
        }
    }

    fn from_slug(s: &str) -> Option<Self> {
        match s {
            "websockets" | "scene_editor" => None,
            "tur_sheet" => Some(Self::TurSheet),
            "part_order" => Some(Self::PartOrder),
            "koth" => Some(Self::Koth),
            "scripts" => Some(Self::Scripts),
            "my_tools" => Some(Self::FileBrowser),
            "file_browser" => Some(Self::FileBrowser),
            "sysinfo" => Some(Self::ResourceMonitor),
            "minidump_analysis" => Some(Self::MinidumpAnalysis),
            "qc" => Some(Self::Qc),
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
            "web_console" => Some(Self::WebConsole),
            "query_editor" => Some(Self::QueryEditor),
            "create_prestashop_order" => Some(Self::CreatePrestashopOrder),
            "threads" => Some(Self::Threads),
            "plugins" => Some(Self::Plugins),
            "database_editor" => Some(Self::DatabaseEditor),
            "fleet_dashboard" => Some(Self::FleetDashboard),
            "stress_lab" => Some(Self::StressLab),
            "stress_test" => Some(Self::StressTest),
            "terminal" => Some(Self::Terminal),
            "shopify_orders" => Some(Self::ShopifyOrders),
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
