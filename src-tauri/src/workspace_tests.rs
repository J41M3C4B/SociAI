//! The crates of the ERP and the tests that run on all of them (ERP-8, ADR-002 D5).
//!
//! **The boundaries.** The kernel and each module are a crate, so a use that the blueprint forbids does not compile:
//! a crate can only use what its `Cargo.toml` names. What stays to be kept is the graph itself, and that is what
//! these tests read from `cargo metadata` (no network): the kernel uses no module, a module uses only the kernel and
//! the `contract` of a module, and a contract uses only the kernel and other contracts. The app (the shell, with the
//! code from before the modules) may use them all, and nothing uses it.
//!
//! The crates of the AI rail keep their own rules: the capability contract uses nothing of the workspace and anyone
//! may use it (ADR-003 A2); the platform uses the contract and the kernel, and only the app uses it (A1); the local
//! detector and the vault (`privacy`) use nothing of the workspace, and only the app and the platform use them
//! (ADR-004).
//!
//! **The conformance test** runs on every registered module, with no line written for each one.

use std::collections::BTreeSet;
use std::path::Path;

/// What a crate of the workspace is, by where it lives.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Crate {
    /// `src-tauri`: the app.
    App,
    /// `crates/kernel`
    Kernel,
    /// `crates/modules/{id}`
    Module(String),
    /// `crates/modules/{id}/contract`
    Contract(String),
    /// `crates/capability`: the capability contract (ADR-001).
    Capability,
    /// `crates/platform`: the AI platform (ADR-003).
    Platform,
    /// `crates/privacy`: the local detector and the vault (ADR-004).
    Privacy,
    /// Anywhere else: not a shape the blueprint knows.
    Outside(String),
}

use Crate::*;

fn kind(root: &Path, dir: &Path) -> Crate {
    let rel = dir.strip_prefix(root).unwrap_or(dir);
    let parts: Vec<String> = rel.components().map(|c| c.as_os_str().to_string_lossy().into_owned()).collect();
    let parts: Vec<&str> = parts.iter().map(String::as_str).collect();
    match parts.as_slice() {
        [] => App,
        ["crates", "kernel"] => Kernel,
        ["crates", "modules", id] => Module(id.to_string()),
        ["crates", "modules", id, "contract"] => Contract(id.to_string()),
        ["crates", "capability"] => Capability,
        ["crates", "platform"] => Platform,
        ["crates", "privacy"] => Privacy,
        _ => Outside(parts.join("/")),
    }
}

/// Whether a crate may depend on another one of the workspace (the ERP blueprint §3, rules 1 to 3).
fn may_use(from: &Crate, to: &Crate) -> bool {
    match (from, to) {
        (_, App | Outside(_)) | (Outside(_), _) => false,
        (App, _) => true,
        // the detector and the vault use nothing of the workspace; the app and the platform use them (ADR-004)
        (Privacy, _) => false,
        // the contract is the lowest crate, and everyone may use it (ADR-003 A2)
        (Capability, _) => false,
        (_, Capability) => true,
        (Platform, Privacy) => true,
        (_, Privacy) => false,
        // the platform uses the contract and the kernel; only the app uses it (ADR-003 A1)
        (Platform, Kernel) => true,
        (Platform, _) | (_, Platform) => false,
        (Kernel, _) => false,
        (Module(_) | Contract(_), Kernel | Contract(_)) => true,
        _ => false,
    }
}

/// Every dependency between crates of the workspace, of any kind (normal, build or for the tests).
fn graph() -> BTreeSet<(Crate, Crate)> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let out = std::process::Command::new(cargo)
        .args(["metadata", "--format-version", "1", "--no-deps", "--offline", "--manifest-path"])
        .arg(&manifest)
        .output()
        .expect("cargo metadata runs");
    assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
    let meta: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let root = Path::new(meta["workspace_root"].as_str().unwrap());
    let mut edges = BTreeSet::new();
    for package in meta["packages"].as_array().unwrap() {
        let from = kind(root, Path::new(package["manifest_path"].as_str().unwrap()).parent().unwrap());
        for dep in package["dependencies"].as_array().unwrap() {
            if let Some(path) = dep["path"].as_str().filter(|p| Path::new(p).starts_with(root)) {
                edges.insert((from.clone(), kind(root, Path::new(path))));
            }
        }
    }
    edges
}

#[test]
fn the_kernel_and_the_modules_only_use_what_they_may() {
    let edges = graph();
    // what the sample module shows, so a graph read wrong does not pass for a clean one
    assert!(edges.contains(&(Module("muestra".into()), Kernel)), "{edges:#?}");
    assert!(edges.contains(&(Module("muestra".into()), Contract("muestra".into()))), "{edges:#?}");
    assert!(edges.contains(&(App, Kernel)), "{edges:#?}");
    assert!(edges.contains(&(App, Platform)) && edges.contains(&(Platform, Capability)), "{edges:#?}");
    assert!(edges.contains(&(Module("muestra".into()), Capability)), "{edges:#?}");
    let forbidden: Vec<_> = edges.iter().filter(|(from, to)| !may_use(from, to)).collect();
    assert!(forbidden.is_empty(), "a dependency the ERP blueprint forbids (§3): {forbidden:#?}");
}

#[test]
fn the_rules_of_the_boundaries() {
    let module = |id: &str| Module(id.into());
    let contract = |id: &str| Contract(id.into());
    assert!(!may_use(&Kernel, &module("contabilidad")), "the kernel knows no module");
    assert!(!may_use(&Kernel, &contract("contabilidad")));
    assert!(may_use(&module("proyectos"), &Kernel));
    assert!(may_use(&module("proyectos"), &contract("proyectos")));
    assert!(may_use(&module("proyectos"), &contract("contabilidad")), "a module uses the published types of another");
    assert!(!may_use(&module("proyectos"), &module("contabilidad")), "never its service, its storage or its tables");
    assert!(may_use(&contract("proyectos"), &contract("contabilidad")));
    assert!(!may_use(&contract("proyectos"), &module("proyectos")), "a contract does not reach into its module");
    assert!(!may_use(&module("proyectos"), &App) && !may_use(&Kernel, &App), "nothing uses the app");
    assert!(may_use(&App, &module("proyectos")));
    assert!(!may_use(&Outside("crates/util".into()), &Kernel) && !may_use(&App, &Outside("crates/util".into())));
    // the two crates of the AI rail (ADR-003)
    assert!(may_use(&Kernel, &Capability) && may_use(&module("proyectos"), &Capability) && may_use(&Platform, &Capability));
    assert!(!may_use(&Capability, &Kernel) && !may_use(&Capability, &module("proyectos")) && !may_use(&Capability, &Platform));
    assert!(may_use(&Platform, &Kernel) && !may_use(&Platform, &module("proyectos")) && !may_use(&Platform, &contract("proyectos")));
    assert!(!may_use(&Kernel, &Platform) && !may_use(&module("proyectos"), &Platform) && !may_use(&contract("proyectos"), &Platform), "only the app uses the platform");
    assert!(may_use(&App, &Platform));
    assert!(may_use(&App, &Privacy) && may_use(&Platform, &Privacy) && !may_use(&Privacy, &Kernel) && !may_use(&Privacy, &Capability));
    assert!(!may_use(&Kernel, &Privacy) && !may_use(&module("proyectos"), &Privacy), "only the app and the platform use the detector");
    let root = Path::new("/w");
    assert_eq!(kind(root, Path::new("/w")), App);
    assert_eq!(kind(root, Path::new("/w/crates/modules/contabilidad/contract")), contract("contabilidad"));
    assert_eq!(kind(root, Path::new("/w/crates/otros")), Outside("crates/otros".into()));
    assert_eq!(kind(root, Path::new("/w/crates/platform")), Platform);
    assert_eq!(kind(root, Path::new("/w/crates/capability")), Capability);
    assert_eq!(kind(root, Path::new("/w/crates/privacy")), Privacy);
}

/// The linker may drop a crate nobody names, and its manifest with it: each module the app links must be in the
/// registry, under the name of its folder.
#[test]
fn every_module_the_app_links_is_in_the_registry() {
    let linked: BTreeSet<String> = graph()
        .into_iter()
        .filter_map(|(from, to)| match (from, to) {
            (App, Module(id)) => Some(id),
            _ => None,
        })
        .collect();
    let registered: BTreeSet<String> = kernel::Modules::discover().unwrap().all().map(|m| m.id.to_string()).collect();
    assert_eq!(registered, linked);
}

/// The conformance test (the ERP blueprint §4). The registry already refused a manifest that breaks the shape, a
/// dependency that does not exist, a cycle and a catalog that breaks the dictionary, such as a field without a class
/// (ERP-2); each module then migrates alone on an empty encrypted database, its tables carry its prefix and none of
/// them points at another module's. And its capabilities (ERP-1): each one passed the review of the registry (IA-1),
/// asks for a permission the module itself declares (ADR-003 A2), prepares the kernel's proposal when it changes
/// something (ERP-9), and gives only fields of the dictionary of a class its `data` allows (ADR-003 A5).
#[test]
fn every_registered_module_conforms() {
    let modules = kernel::Modules::discover().unwrap_or_else(|errors| panic!("{errors:#?}"));
    let registry = capability::Registry::discover().unwrap_or_else(|errors| panic!("{errors:#?}"));
    assert!(modules.all().count() > 0, "the sample module is registered");
    for m in modules.all() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = rusqlite::Connection::open(dir.path().join("m.db")).unwrap();
        conn.execute_batch("PRAGMA key = 'conformance'; PRAGMA foreign_keys = ON;").unwrap();
        let breaches = kernel::conform(m.id, m.migrations, &mut conn);
        assert!(breaches.is_empty(), "module `{}`: {:#?}", m.id, breaches.iter().map(ToString::to_string).collect::<Vec<_>>());

        let own = capability::Registry::of(registry.of_module(m.id)).unwrap();
        for c in own.all() {
            assert!(m.permissions.iter().any(|p| p.id == c.permission), "{}: asks for `{}`, which `{}` does not declare", c.id, c.permission, m.id);
        }
        assert_eq!(platform::changes_without_proposal(&own), vec![], "module `{}`", m.id);
        assert_eq!(platform::outputs_outside_dictionary(&own, &modules), vec![], "module `{}`", m.id);
    }
    // Mi institución publishes what is missing, a window and saving a window
    let mine: Vec<&str> = registry.of_module("mi_institucion").map(|c| c.id).collect();
    for id in ["mi_institucion.que_falta", "mi_institucion.ver_ventana", "mi_institucion.guardar_ventana"] {
        assert!(mine.contains(&id), "{id}: {mine:?}");
    }
}
