-- The profile of the institution: one row, one column per field of the catalog (`catalog::PROFILE`), with its version
-- (it rises with every change). Texts as written, codes of the contract, lists of codes as a JSON array, dates
-- `YYYY-MM-DD`. Its history and the origin of each datum are the kernel's (K5).
CREATE TABLE mi_institucion_profile (
    id TEXT PRIMARY KEY,
    version INTEGER NOT NULL,
    -- who it is, and whom and how it serves
    name TEXT,
    mission TEXT,
    populations TEXT,
    sex_served TEXT,
    modalities TEXT,
    care_areas TEXT,
    services TEXT,
    age_min INTEGER,
    age_max INTEGER,
    admission_criteria TEXT,
    -- how to reach it and where it is
    contact_phone TEXT,
    contact_email TEXT,
    street TEXT,
    ext_number TEXT,
    int_number TEXT,
    neighborhood TEXT,
    postal_code TEXT,
    municipality TEXT,
    state TEXT,
    -- before the law, the SAT and its Junta
    legal_name TEXT,
    purpose TEXT,
    legal_form TEXT,
    founded_year INTEGER,
    junta_folio TEXT,
    legal_rfc TEXT,
    tax_regime TEXT,
    fiscal_postal_code TEXT,
    authorized_donee TEXT,
    cluni TEXT,
    donee_category TEXT,
    donee_letter_number TEXT,
    donee_letter_date TEXT,
    cluni_key TEXT,
    legal_rep_name TEXT,
    legal_rep_valid_until TEXT,
    -- how many fit, are served and work there
    capacity_total INTEGER,
    served_estimate INTEGER,
    served_last_year INTEGER,
    staff_paid_estimate INTEGER,
    staff_volunteer_estimate INTEGER,
    notes TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
