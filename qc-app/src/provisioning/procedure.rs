//! Sequential provisioning procedure (QCWizard NEW_BUILD / SERVICE / QC_CHECK port).
//! Resolves an ordered, self-contained step list from the company manifest; the panel
//! runs each step on a worker thread one at a time.

use std::path::PathBuf;

use database::orders::BuildSpec;

use super::company::Company;
use super::manifest::{CompanyManifest, SoftwareSpec};
use super::{dmi, install_chipset, install_display, osconfig, software, vendor_steps};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcedureKind {
    NewBuild,
    Service,
    QcCheck,
}

impl ProcedureKind {
    pub const ALL: [ProcedureKind; 3] = [Self::NewBuild, Self::Service, Self::QcCheck];

    pub fn label(self) -> &'static str {
        match self {
            Self::NewBuild => "New Build",
            Self::Service => "Service",
            Self::QcCheck => "QC Check",
        }
    }
}

/// One resolved, owned step ready to run with no outstanding borrows.
pub enum ResolvedStep {
    Branding,
    Dmi { tool: PathBuf, cmds: Vec<(&'static str, String)> },
    CoreIsolation,
    Timezone,
    OpenTools,
    ChipsetDriver,
    DisplayDriver,
    Software(SoftwareSpec),
    RemoveAtHomeSupport,
    RemoveEdgeFavorites,
    VrChatInstaller { asset_tag: String },
    CaptureSystemInfo,
    OpenQaReport,
}

impl ResolvedStep {
    pub fn label(&self) -> String {
        match self {
            Self::Branding => "Branding".into(),
            Self::Dmi { .. } => "DMI write".into(),
            Self::CoreIsolation => "Core isolation".into(),
            Self::Timezone => "Timezone".into(),
            Self::OpenTools => "Open system tools".into(),
            Self::ChipsetDriver => "Chipset driver".into(),
            Self::DisplayDriver => "Display driver".into(),
            Self::Software(s) => format!("Software: {}", s.id),
            Self::RemoveAtHomeSupport => "Remove At-Home Support".into(),
            Self::RemoveEdgeFavorites => "Remove Edge favorites".into(),
            Self::VrChatInstaller { .. } => "VRChat installer".into(),
            Self::CaptureSystemInfo => "Capture system info".into(),
            Self::OpenQaReport => "Open QA report".into(),
        }
    }

    pub fn run(self, sqlite_path: &str) -> anyhow::Result<String> {
        match self {
            Self::Branding => Ok("Branding (.bat) — later phase.".into()),
            Self::Dmi { tool, cmds } => dmi::run(&tool, &cmds),
            Self::CoreIsolation => osconfig::enable_core_isolation(),
            Self::Timezone => osconfig::set_timezone_mountain(),
            Self::OpenTools => osconfig::open_system_tools(),
            Self::ChipsetDriver => install_chipset(sqlite_path),
            Self::DisplayDriver => install_display(sqlite_path),
            Self::Software(spec) => software::install(&spec),
            Self::RemoveAtHomeSupport => vendor_steps::remove_at_home_support(),
            Self::RemoveEdgeFavorites => vendor_steps::remove_edge_favorites(),
            Self::VrChatInstaller { asset_tag } => vendor_steps::install_vrchat_custom(&asset_tag),
            Self::CaptureSystemInfo => Ok("System info captured via QC report submission.".into()),
            Self::OpenQaReport => Ok("QA report is the right-hand checklist panel.".into()),
        }
    }
}

/// Tool path + resolved AMIDEWIN commands for a procedure's DMI step.
pub struct DmiInputs {
    pub tool: PathBuf,
    pub cmds: Vec<(&'static str, String)>,
}

/// Ordered step list for a procedure kind, including manifest software + vendor steps.
pub fn resolve(
    kind: ProcedureKind,
    manifest: &CompanyManifest,
    company: Company,
    build: &BuildSpec,
    dmi: Option<DmiInputs>,
    asset_tag: &str,
) -> Vec<ResolvedStep> {
    let mut steps = Vec::new();
    match kind {
        ProcedureKind::NewBuild => {
            steps.push(ResolvedStep::Branding);
            if let Some(d) = dmi {
                steps.push(ResolvedStep::Dmi { tool: d.tool, cmds: d.cmds });
            }
            steps.push(ResolvedStep::CoreIsolation);
            steps.push(ResolvedStep::Timezone);
            steps.push(ResolvedStep::OpenTools);
            steps.push(ResolvedStep::ChipsetDriver);
            steps.push(ResolvedStep::DisplayDriver);
            for spec in software::plan(manifest, build) {
                steps.push(ResolvedStep::Software(spec.clone()));
            }
            push_vendor_steps(company, asset_tag, &mut steps);
            steps.push(ResolvedStep::CaptureSystemInfo);
            steps.push(ResolvedStep::OpenQaReport);
        }
        ProcedureKind::Service => {
            if let Some(d) = dmi {
                steps.push(ResolvedStep::Dmi { tool: d.tool, cmds: d.cmds });
            }
            steps.push(ResolvedStep::CoreIsolation);
            steps.push(ResolvedStep::ChipsetDriver);
            steps.push(ResolvedStep::DisplayDriver);
            steps.push(ResolvedStep::CaptureSystemInfo);
        }
        ProcedureKind::QcCheck => {
            steps.push(ResolvedStep::OpenTools);
            steps.push(ResolvedStep::OpenQaReport);
        }
    }
    steps
}

fn push_vendor_steps(company: Company, asset_tag: &str, steps: &mut Vec<ResolvedStep>) {
    match company {
        Company::Bimbox => {
            steps.push(ResolvedStep::RemoveAtHomeSupport);
            steps.push(ResolvedStep::RemoveEdgeFavorites);
        }
        Company::VrChat if !asset_tag.trim().is_empty() => {
            steps.push(ResolvedStep::VrChatInstaller { asset_tag: asset_tag.to_string() });
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn manifest() -> CompanyManifest {
        CompanyManifest::default_for(Company::Pcl)
    }

    #[test]
    fn qc_check_is_minimal() {
        let steps = resolve(ProcedureKind::QcCheck, &manifest(), Company::Pcl, &BuildSpec::default(), None, "");
        assert_eq!(steps.len(), 2);
    }

    #[test]
    fn service_includes_drivers() {
        let steps = resolve(ProcedureKind::Service, &manifest(), Company::Pcl, &BuildSpec::default(), None, "");
        assert!(steps.iter().any(|s| s.label() == "Chipset driver"));
        assert!(steps.iter().any(|s| s.label() == "Display driver"));
    }

    #[test]
    fn bimbox_new_build_adds_vendor_steps() {
        let steps = resolve(ProcedureKind::NewBuild, &CompanyManifest::default_for(Company::Bimbox), Company::Bimbox, &BuildSpec::default(), None, "");
        assert!(steps.iter().any(|s| s.label() == "Remove At-Home Support"));
    }
}
