mod ai;
mod audit;
mod commands;
mod common;
mod core;
mod documents;
mod error;
mod modules;
mod scanner;
mod storage;
#[cfg(test)]
mod architecture_tests;
#[cfg(test)]
mod workspace_tests;
#[cfg(test)]
mod platform_tests;
// the sample module, named once so the linker keeps its manifest for the registry (ERP-8)
#[cfg(test)]
extern crate muestra;
// the modules of the program, named so the linker keeps their manifests and capabilities
extern crate mi_institucion;

use std::sync::{Arc, Mutex};
use tauri::Manager;

/// Shared database connection, opened at startup.
pub struct Db(pub Arc<Mutex<rusqlite::Connection>>);

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        // the native window to choose a file: the record keeps where its original is (N4)
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| {
            // every capability of the program, checked once (ADR-001): with an invalid contract the app does not start
            let registry = capability::Registry::discover()
                .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))?;
            // the modules of the ERP, and the catalog of permissions they and the kernel define (K2, K12): a
            // capability that asks for a permission nobody defines stops the app too (ADR-003 A2), and so does a
            // modification that does not prepare the kernel's proposal (ERP-9), and an output that names a field the
            // dictionary does not have or one of a class outside its `data` (ADR-003 A5)
            let modules = kernel::Modules::discover()
                .map_err(|errors| errors.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n"))?;
            let faults = [
                platform::unknown_permissions(&registry, &modules),
                platform::changes_without_proposal(&registry),
                platform::outputs_outside_dictionary(&registry, &modules),
                // only the platform declares an operator `ia.…` (ADR-003 A6)
                platform::capabilities_named_as_operators(&registry),
            ]
            .concat();
            if !faults.is_empty() {
                return Err(faults.iter().map(ToString::to_string).collect::<Vec<_>>().join("\n").into());
            }
            app.manage(registry);
            app.manage(modules);
            // what reads scanned pages and photos (OCR, ADR-035) ships with the program; the screen hears how many
            // pages are read
            documents::ocr::set_home(app.path().resource_dir()?.join("ocr"));
            let handle = app.handle().clone();
            documents::reading::on_progress(move |key, done, total| {
                let _ = tauri::Emitter::emit(&handle, "reading-progress", serde_json::json!({ "key": key, "done": done, "total": total }));
            });
            #[allow(unused_mut)]
            let mut dir = app.path().app_data_dir()?;
            // development only: another folder, to try the first start from zero without touching the examples
            #[cfg(debug_assertions)]
            if let Some(other) = std::env::var_os("SOCIAI_DATA_DIR").filter(|d| !d.is_empty()) {
                dir = other.into();
            }
            std::fs::create_dir_all(&dir)?;
            let key = storage::get_or_create_db_key()?;
            let mut conn = storage::open_encrypted(&dir.join("sociai.db"), &key)?;
            // a database of before the modules hands its profile and its history to Mi institución, once (ERP-1)
            core::profile::adopt::adopt(&mut conn)?;
            // the tables of the platform: its tasks, their steps, approvals and trail (ADR-003 A3)
            platform::migrate(&mut conn)?;
            // the photo of this month (ADR-033 §4): taken again at each opening until the month ends
            let _ = core::snapshot::take(&conn);
            let db = Arc::new(Mutex::new(conn));
            // who is using the app (ADR-028): nobody until they enter
            app.manage(core::access::service::Session::new(db.clone()));
            app.manage(Db(db));
            app.manage(core::security::Attempts::default());
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::access::access_status,
            commands::access::access_setup_admin,
            commands::access::access_login,
            commands::access::access_recover,
            commands::access::access_unlock,
            commands::access::access_lock,
            commands::access::access_logout,
            commands::access::access_change_password,
            commands::access::access_team,
            commands::access::admin_overview,
            commands::access::admin_user_create,
            commands::access::admin_user_update,
            commands::access::admin_user_reset_password,
            commands::access::admin_request_resolve,
            commands::access::admin_audit,
            commands::access::admin_recovery_code_new,
            commands::profile_get,
            commands::form_get,
            commands::institution_overview,
            commands::form_save,
            commands::institution::institution_lists,
            commands::institution::institution_list_form,
            commands::institution::institution_list_save,
            commands::institution::institution_list_set_active,
            commands::institution::institution_list_delete,
            commands::records::institution_records,
            commands::records::institution_record_form,
            commands::records::institution_record_save,
            commands::records::institution_record_attach,
            commands::records::institution_record_place,
            commands::records::institution_record_detach,
            commands::records::institution_record_open,
            commands::institution::institution_clabe_reveal,
            commands::manual_entry,
            commands::app_places,
            commands::proposals_reject,
            commands::manual_search,
            commands::documents_list,
            commands::documents_search,
            commands::document_emergency_delete,
            commands::dev_load_fixture,
            commands::care::care_overview,
            commands::care::care_person_get,
            commands::care::care_person_save,
            commands::care::care_person_delete,
            commands::care::care_person_reveal,
            commands::care::care_group_save,
            commands::care::care_field_save,
            commands::care::care_field_delete,
            commands::care::care_waitlist_save,
            commands::care::care_waitlist_admit,
            commands::care::care_waitlist_delete,
            commands::onboarding::onboarding_status,
            commands::onboarding::onboarding_save,
            commands::onboarding::onboarding_finish,
            commands::onboarding::onboarding_welcome_done,
            commands::facilities::facilities_overview,
            commands::facilities::facilities_site_save,
            commands::facilities::facilities_space_save,
            commands::facilities::facilities_space_delete,
            commands::facilities::facilities_equipment_save,
            commands::facilities::facilities_equipment_delete,
            commands::finance::finance_get,
            commands::finance::finance_save,
            commands::hr::hr_overview,
            commands::hr::hr_person_get,
            commands::hr::hr_person_save,
            commands::hr::hr_person_delete,
            commands::hr::hr_person_reveal,
            commands::hr::hr_position_save,
            commands::hr::hr_position_set_active,
            commands::hr::hr_modality_create,
            commands::hr::hr_field_save,
            commands::hr::hr_field_delete,
            commands::ai::ai_status,
            commands::ai::ai_set_provider,
            commands::ai::ai_models,
            commands::ai::ai_set_models,
            commands::ai::ai_set_key,
            commands::ai::ai_clear_key,
            commands::ai::ai_check,
            commands::ai::ai_usage_report,
            commands::ai::ai_set_cap,
            commands::security::security_scan,
            commands::security::backup_create,
            commands::security::backup_restore,
            commands::runtime::runtime_submit,
            commands::runtime::runtime_task,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
