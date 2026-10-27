//! The windows of the profile: how the screen groups its fields. Each one is a view of the one entity of the catalog
//! (`catalog::PROFILE`), by the id of each field, with its sections and their columns; saving a window changes only
//! its fields. The words live in `src/i18n/es-MX.ts`.

/// A window of the profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Window {
    /// `identity`, `contact`, `legal` or `capacity`.
    pub id: &'static str,
    pub sections: &'static [Section],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Section {
    pub id: &'static str,
    /// How many columns the screen uses for its fields.
    pub columns: u8,
    pub fields: &'static [&'static str],
}

impl Window {
    /// Its fields, in order.
    pub fn fields(&self) -> impl Iterator<Item = &'static str> {
        self.sections.iter().flat_map(|s| s.fields.iter().copied())
    }

    pub fn has(&self, field: &str) -> bool {
        self.fields().any(|f| f == field)
    }
}

/// «Su institución»: who it is and whom and how it serves.
pub const IDENTITY: Window = Window {
    id: "identity",
    sections: &[
        Section { id: "who", columns: 1, fields: &["name", "mission"] },
        Section { id: "attention", columns: 1, fields: &["populations", "sex_served", "modalities", "care_areas", "services"] },
        Section { id: "admission", columns: 2, fields: &["age_min", "age_max"] },
        Section { id: "admission_rules", columns: 1, fields: &["admission_criteria"] },
    ],
};

/// «Contacto y ubicación»: how to reach the institution and where it is.
pub const CONTACT: Window = Window {
    id: "contact",
    sections: &[
        Section { id: "contact", columns: 2, fields: &["contact_phone", "contact_email"] },
        Section { id: "address", columns: 2, fields: &["street", "ext_number", "int_number", "neighborhood", "postal_code", "municipality", "state"] },
    ],
};

/// «Datos legales y fiscales»: how the institution stands before the law, the SAT and its Junta.
pub const LEGAL: Window = Window {
    id: "legal",
    sections: &[
        Section { id: "legal_identity", columns: 1, fields: &["legal_name", "purpose"] },
        Section { id: "constitution", columns: 2, fields: &["legal_form", "founded_year", "junta_folio"] },
        Section { id: "fiscal", columns: 2, fields: &["legal_rfc", "tax_regime", "fiscal_postal_code"] },
        Section {
            id: "registries",
            columns: 2,
            fields: &["authorized_donee", "cluni", "donee_category", "donee_letter_number", "donee_letter_date", "cluni_key"],
        },
        Section { id: "legal_rep", columns: 2, fields: &["legal_rep_name", "legal_rep_valid_until"] },
    ],
};

/// «Capacidad y cifras rápidas»: how many fit, and how many are served and work there while their records are not
/// there yet.
pub const CAPACITY: Window = Window {
    id: "capacity",
    sections: &[
        Section { id: "capacity_people", columns: 2, fields: &["capacity_total", "served_estimate", "served_last_year"] },
        Section { id: "capacity_staff", columns: 2, fields: &["staff_paid_estimate", "staff_volunteer_estimate"] },
        Section { id: "capacity_notes", columns: 1, fields: &["notes"] },
    ],
};

pub const WINDOWS: &[Window] = &[IDENTITY, CONTACT, LEGAL, CAPACITY];

pub fn window(id: &str) -> Option<&'static Window> {
    WINDOWS.iter().find(|w| w.id == id)
}

/// The window a field is in.
pub fn window_of(field: &str) -> Option<&'static Window> {
    WINDOWS.iter().find(|w| w.has(field))
}

/// What the program cannot work well without, so the first start asks it.
pub const ESSENTIAL: &[&str] = &[
    "name",
    "mission",
    "populations",
    "modalities",
    "municipality",
    "state",
    "legal_form",
    "founded_year",
    "authorized_donee",
    "cluni",
    "capacity_total",
    "served_estimate",
    "served_last_year",
    "staff_paid_estimate",
    "staff_volunteer_estimate",
];

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog::PROFILE;

    /// The windows show every field of the profile once, in the order of the catalog, and what is indispensable is
    /// in the catalog and listed as needed.
    #[test]
    fn the_windows_show_the_whole_profile_once() {
        let shown: Vec<&str> = WINDOWS.iter().flat_map(Window::fields).collect();
        let catalog: Vec<&str> = PROFILE.fields.iter().map(|f| f.id).collect();
        assert_eq!(shown, catalog);
        for f in ESSENTIAL {
            let field = PROFILE.field(f).unwrap_or_else(|| panic!("{f}"));
            assert_ne!(field.presence, kernel::Presence::Optional, "{f}");
        }
        assert_eq!(window_of("legal_rfc").map(|w| w.id), Some("legal"));
        assert!(window("board").is_none());
    }
}
