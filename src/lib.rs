//! Rainmill Google Analytics: a Google Analytics 4 property on the admin's
//! report dashboards, through `rainmill-reporting`.
//!
//! The service account's key and the property ID live in the vault
//! (`rainmill-vault`) as a "Google Analytics" credential attached to the
//! plugin's slot. The data source answers report queries with the Data API's
//! `runReport`, and the plugin ships a "Google Analytics" dashboard.

mod check;
pub mod google;
mod source;
mod tracker;

use std::sync::Arc;

use laterite_admin::form::FormField;
use laterite_admin::{AdminRegistry, Permission};
use laterite_core::serde_json::json;
use laterite_core::{t, Module, ModuleId, Registry};
use rainmill_reporting::{DashboardReg, DataSourceReg};
use rainmill_vault::{ActionReg, KindReg, SlotReg};

pub use source::AnalyticsSource;
pub use tracker::{COMPONENT, SCRIPT_PATH};

/// The module's identity, and the data source's code.
pub const MODULE_ID: &str = "rainmill.googleanalytics";
/// The kind of credential the plugin reads from the vault.
pub const KIND: &str = "rainmill.googleanalytics.service_account";
/// The slot the credential is attached to.
pub const SLOT: &str = "rainmill.googleanalytics";
/// The permission to read the property's reports.
pub const VIEW: &str = "rainmill.googleanalytics.view";

/// The plugin as a module, for the application's module list.
pub fn module() -> Box<dyn Module> {
    Box::new(GoogleAnalyticsModule)
}

/// The Google Analytics plugin.
pub struct GoogleAnalyticsModule;

#[laterite_core::strata::async_trait]
impl Module for GoogleAnalyticsModule {
    fn id(&self) -> ModuleId {
        ModuleId::new(MODULE_ID)
    }

    fn requires(&self) -> &'static [ModuleId] {
        const REQUIRES: &[ModuleId] = &[
            ModuleId::new("rainmill.vault"),
            ModuleId::new("rainmill.reporting"),
        ];
        REQUIRES
    }

    fn register(&self, registry: &mut Registry) {
        registry.add_permission(
            Permission::new(
                VIEW,
                t!("Read the Google Analytics reports"),
                t!("Google Analytics"),
            )
            .roles([laterite_admin::ROLE_ADMIN]),
        );
        registry.add(
            KindReg::new(KIND, t!("Google Analytics"))
                .category(rainmill_vault::Category::Data)
                .help(t!(
                    "A Google Cloud service account with the Google Analytics Data API enabled, added as a Viewer under the property's access management."
                ))
                .field(
                    FormField::of("key", t!("Service account key"), "sensitive")
                        .options(json!({ "file": true, "accept": [".json"], "max_size": "16kb" }))
                        .required()
                        .help(t!("The JSON key file Google Cloud downloads under the service account's Keys.")),
                )
                .field(
                    FormField::text("property_id", t!("Property ID"))
                        .required()
                        .help(t!("The number on Admin, Property settings in Google Analytics.")),
                ),
        );
        registry.add(
            SlotReg::single(SLOT, KIND)
                .label(t!("Google Analytics reports"))
                .reason(t!(
                    "Reads the property's traffic for the report dashboards."
                )),
        );
        registry.add(
            ActionReg::new(KIND, "test", t!("Test connection"), Arc::new(check::Test)).icon("plug"),
        );
        registry.add(DataSourceReg::new(Arc::new(AnalyticsSource)));
        registry.add(laterite_web::theme::component::ComponentReg::new(
            tracker::COMPONENT,
            "Google Analytics tracker",
            Arc::new(tracker::Tracker),
        ));
        registry.add_public_route(laterite_admin::routes::PublicRouteReg::new(
            tracker::SCRIPT_PATH,
            tracker::script(),
        ));
        registry.add(
            DashboardReg::new(
                MODULE_ID,
                t!("Google Analytics"),
                include_str!("../assets/dashboard.yaml"),
            )
            .icon("chart-line")
            .permission(VIEW),
        );
    }
}
