//! What the core says about the institution to the rest of the app.

pub mod catalog;
pub mod forms;

use rusqlite::{Connection, OptionalExtension};

/// The kind of institution (`elderly_home`, `children_home`, …), from whom it serves in the profile of Mi institución
/// (ERP-1), or `None` before the profile is written. Each module turns it into its own `Flavor` (`Flavor::from_kind`).
pub fn kind(conn: &Connection) -> Option<String> {
    let profile = mi_institucion::service::profile(conn).ok().filter(|p| p.version > 0)?;
    let populations: Vec<String> = serde_json::from_value(profile.values.get("populations").cloned().unwrap_or_default()).unwrap_or_default();
    Some(catalog::kind_of(&populations).to_string())
}

/// The id of the institution, which is the organization that owns this database (D4), or `None` before it is written.
#[allow(dead_code)]
pub fn id(conn: &Connection) -> Option<String> {
    conn.query_row("SELECT id FROM institution LIMIT 1", [], |r| r.get(0)).optional().ok().flatten()
}

/// The kind of institution as the staff module understands it (the positions it suggests).
pub fn hr_flavor(conn: &Connection) -> crate::modules::hr::Flavor {
    crate::modules::hr::Flavor::from_kind(kind(conn).as_deref())
}

/// The kind of institution as the module of the people served understands it (extra data and age bands).
pub fn care_flavor(conn: &Connection) -> crate::modules::care::Flavor {
    crate::modules::care::Flavor::from_kind(kind(conn).as_deref())
}

/// The kind of institution as the facilities module understands it (the spaces and equipment it suggests first).
pub fn facilities_flavor(conn: &Connection) -> crate::modules::facilities::Flavor {
    crate::modules::facilities::Flavor::from_kind(kind(conn).as_deref())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_kind_is_read_once_for_every_module() {
        let dir = tempfile::tempdir().unwrap();
        let mut conn = crate::storage::open_encrypted(&dir.path().join("t.db"), "k").unwrap();
        assert_eq!(kind(&conn), None);
        assert_eq!(hr_flavor(&conn), crate::modules::hr::Flavor::Other);
        let identity = serde_json::json!({ "name": "Casa", "populations": ["childhood", "adolescence"] });
        let origin = kernel::Origin::Calculated { rule: "fixtures".into() };
        crate::core::profile::storage::save_window(&mut conn, "identity", identity.as_object().unwrap().clone(), &origin).unwrap();
        assert_eq!(kind(&conn).as_deref(), Some("children_home"));
        assert_eq!(care_flavor(&conn), crate::modules::care::Flavor::ChildrenHome);
        assert_eq!(facilities_flavor(&conn), crate::modules::facilities::Flavor::ChildrenHome);
    }
}
