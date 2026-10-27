//! The sheet of the institution that the AI reads: everything «Mi institución» says, as aggregates, in plain Spanish.
//!
//! The AI knows the institution only through this text, so what it leaves out the AI cannot know and ends up
//! guessing. Hence three rules: (1) it carries every datum the person captured that is allowed to reach the AI;
//! (2) a section with nothing captured says so («no capturado»), because «no sé» is not «cero»; (3) the sums are
//! made here, by code. Never in it: names, contact data, RFC, pay or the amount any person pays (ADR-020), nor an
//! age range of a group of one person. Neither the payroll nor the fees of the roster go as a figure, not even
//! added up: with one paid person the total is that person's pay. That is also why the balance goes as words
//! (whether the income covers the expenses), never as a figure the payroll could be worked out from (ADR-026).
//! It carries no incidental numbers either (versions, dates): the figure check treats every number of this text
//! as something the person said.

use crate::core::profile::domain::InstitutionKind;
use crate::modules::finance::domain::balance::{self, ExpenseBasis, BENEFICIARY_FEES, EXPENSE};
use crate::modules::finance::domain::lines::FinanceInput;
use crate::modules::finance::domain::money::{IncomeKind, Period};
use crate::modules::care::domain::aggregate::CareSummary;
use crate::core::insights::facilities::FacilityBoard;
use crate::core::insights::facility_text;
use crate::core::insights::people::Insight;
use crate::modules::facilities::domain::aggregate::SiteSummary;
use crate::modules::hr::api::{Count, MIN_GROUP};
use crate::modules::hr::domain::aggregate::StaffSummary;
use crate::core::error::ServiceError;
use crate::core::profile::storage::{self as profile_store, StoredProfile};
use crate::core::protected::BoardSheet;
use rusqlite::Connection;

/// The people served as their module tells them now (ADR-029): counts, attributes of groups of three or more, and
/// the findings of the board that carry no money.
pub struct PeopleSheet {
    pub summary: CareSummary,
    pub findings: Vec<Insight>,
}

pub fn people_sheet(conn: &Connection) -> Result<PeopleSheet, ServiceError> {
    let summary = crate::modules::care::api::ai_summary(conn, crate::core::institution::care_flavor(conn))?;
    let findings = crate::core::bridge::care::board(conn)?.insights.into_iter().filter(|i| i.for_ai).collect();
    Ok(PeopleSheet { summary, findings })
}

/// The facilities as their module tells them now (ADR-030): the building, its services and safety, every group of
/// spaces and equipment, and the findings of their board that may reach the AI.
pub struct FacilitiesSheet {
    pub sites: Vec<SiteSummary>,
    pub board: FacilityBoard,
}

pub fn facilities_sheet(conn: &Connection) -> Result<FacilitiesSheet, ServiceError> {
    Ok(FacilitiesSheet { sites: crate::modules::facilities::api::summaries(conn)?, board: crate::core::bridge::facilities::board(conn)? })
}

/// The sheet of the current profile (the latest version, confirmed or draft), with the staff (ADR-027), the people
/// served (ADR-029) and the facilities (ADR-030) as their modules tell them now.
pub fn profile_context(conn: &Connection) -> Result<String, ServiceError> {
    Ok(match profile_store::load_current(conn)? {
        Some(p) => render(
            &p,
            &crate::modules::finance::api::lines(conn)?,
            &crate::modules::hr::api::ai_summary(conn)?,
            &people_sheet(conn)?,
            &facilities_sheet(conn)?,
            &crate::core::protected::ai_board(conn)?,
        ),
        None => "Perfil: sin datos. Todavía no hay nada capturado en «Mi institución»; de la institución solo se sabe lo que la persona diga.".into(),
    })
}

fn kind_text(k: InstitutionKind) -> &'static str {
    match k {
        InstitutionKind::ElderlyHome => "asilo",
        InstitutionKind::ChildrenHome => "casa hogar",
        InstitutionKind::Other => "institución de asistencia",
    }
}


fn relation_text(code: &str) -> &'static str {
    match code {
        "employee" => "con sueldo",
        "fees" => "por honorarios o asimilados",
        "religious" => "religiosas o religiosos de la congregación",
        "volunteer" => "de voluntariado",
        "trainee" => "en servicio social o prácticas",
        _ => "de una empresa externa",
    }
}

fn area_text(code: &str) -> &'static str {
    match code {
        "care" => "cuidado",
        "health" => "salud",
        "kitchen" => "cocina",
        "cleaning" => "limpieza",
        "laundry" => "lavandería",
        "administration" => "administración",
        "social_work" => "trabajo social",
        "psychology" => "psicología",
        "rehabilitation" => "rehabilitación",
        "education" => "educación",
        "pastoral" => "pastoral",
        "maintenance" => "mantenimiento",
        "security" => "vigilancia",
        _ => "otra área",
    }
}

fn schedule_text(code: &str) -> &'static str {
    match code {
        "full_time" => "tiempo completo",
        "part_time" => "medio tiempo",
        "hourly" => "por horas",
        _ => "fines de semana",
    }
}

fn shift_text(code: &str) -> &'static str {
    match code {
        "morning" => "matutino",
        "afternoon" => "vespertino",
        "night" => "nocturno",
        "rotating" => "por turnos",
        _ => "24 horas",
    }
}

fn education_text(code: &str) -> &'static str {
    match code {
        "basic" => "hasta secundaria",
        "high_school_or_technical" => "con preparatoria o carrera técnica",
        _ => "con licenciatura o posgrado",
    }
}

fn seniority_text(code: &str) -> &'static str {
    match code {
        "under_1" => "con menos de 1 año",
        "1_to_4" => "de 1 a 4 años",
        "5_to_9" => "de 5 a 9 años",
        _ => "de 10 años o más",
    }
}

fn people(n: i64) -> String {
    format!("{n} {}", if n == 1 { "persona" } else { "personas" })
}

/// «4 con sueldo, 3 de voluntariado».
fn listed(counts: &[Count], words: fn(&str) -> &'static str) -> String {
    counts.iter().map(|c| format!("{} {}", c.count, words(c.code))).collect::<Vec<_>>().join(", ")
}

/// The staff, by position: how many, their relation, schedule and shift, the seats and what the position does.
/// Schooling and seniority go only for groups of `MIN_GROUP` people or more.
fn render_staff(s: &mut String, staff: &StaffSummary, estimate: (Option<i64>, Option<i64>), missing: &mut Vec<&str>) {
    if staff.total == 0 && staff.positions.is_empty() {
        // no records yet: the quick figures of the onboarding, said as approximate (ADR-031)
        match estimate {
            (None, None) => missing.push("el personal"),
            (paid, volunteer) => s.push_str(&format!(
                "Personal (cifra aproximada que dio la persona; todavía sin registros): {} con sueldo y {} de voluntariado.\n",
                paid.map_or("sin dato".to_string(), |n| n.to_string()),
                volunteer.map_or("sin dato".to_string(), |n| n.to_string())
            )),
        }
        return;
    }
    for p in &staff.positions {
        let area = p.area.as_deref().map(|a| format!(" ({})", area_text(a))).unwrap_or_default();
        if p.people == 0 {
            let seats = p.authorized_seats.unwrap_or(0);
            s.push_str(&format!(
                "Puesto sin cubrir: {}{area} — {seats} {} y ninguna cubierta.\n",
                p.title,
                if seats == 1 { "plaza autorizada" } else { "plazas autorizadas" }
            ));
        } else {
            let mut details = vec![listed(&p.by_relation, relation_text)];
            if !p.schedules.is_empty() {
                details.push(format!("jornada: {}", listed(&p.schedules, schedule_text)));
            }
            if !p.shifts.is_empty() {
                details.push(format!("turno: {}", listed(&p.shifts, shift_text)));
            }
            s.push_str(&format!("Personal: {}{area} — {} ({}).", p.title, people(p.people), details.join("; ")));
            if let Some(seats) = p.authorized_seats {
                s.push_str(&format!(" Plazas autorizadas: {seats}; sin cubrir: {}.", p.vacancies));
            }
            s.push('\n');
        }
        if let Some(d) = &p.duties {
            s.push_str(&format!("Funciones del puesto {}: {d}\n", p.title));
        }
    }
    if staff.total > 0 {
        s.push_str(&format!("Total del personal: {}: {}.\n", people(staff.total), listed(&staff.by_relation, relation_text)));
    }
    if !staff.education.is_empty() {
        s.push_str(&format!("Escolaridad del personal (solo grupos de {MIN_GROUP} o más personas): {}.\n", listed(&staff.education, education_text)));
    }
    if !staff.seniority.is_empty() {
        s.push_str(&format!("Antigüedad en la institución (solo grupos de {MIN_GROUP} o más personas): {}.\n", listed(&staff.seniority, seniority_text)));
    }
}

/// `1800000` -> `$1,800,000` (the figure check reads the commas as thousands).
fn mxn(n: i64) -> String {
    let digits = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(c);
    }
    format!("{}${out}", if n < 0 { "-" } else { "" })
}

fn income_kind_text(k: IncomeKind) -> &'static str {
    match k {
        IncomeKind::FeeEstimate => "cuotas de los beneficiarios (aproximado escrito a mano)",
        IncomeKind::RecurringDonor => "donante fijo",
        IncomeKind::OccasionalDonation => "donativo ocasional",
        IncomeKind::ProjectGrant => "donativo ganado con un proyecto",
        IncomeKind::Other => "otro ingreso",
    }
}

/// `$5,000 al mes ($60,000 al año)` or `$680,000 al año`; the yearly figure is made here.
fn amount_text(amount: Option<i64>, period: Period) -> String {
    match (amount, period) {
        (None, _) => "monto no capturado".into(),
        (Some(a), Period::Monthly) => format!("{} pesos al mes ({} al año)", mxn(a), mxn(a * 12)),
        (Some(a), Period::Annual) => format!("{} pesos al año", mxn(a)),
    }
}

fn text(o: &Option<String>) -> Option<&str> {
    o.as_deref().map(str::trim).filter(|s| !s.is_empty())
}

fn care_word(code: &str) -> &'static str {
    match code {
        "low" => "poco apoyo",
        "medium" => "apoyo regular",
        "high" => "mucho apoyo",
        "total" => "apoyo en todo",
        "independent" => "caminan sin ayuda",
        "cane_walker" => "usan bastón o andadera",
        "wheelchair" => "usan silla de ruedas",
        "bedridden" => "están en cama",
        "motor" => "con discapacidad motriz",
        "visual" => "con discapacidad visual",
        "hearing" => "con discapacidad auditiva",
        "intellectual" => "con discapacidad intelectual",
        "psychosocial" => "con discapacidad psicosocial",
        "diabetes" => "con diabetes",
        "hypertension" => "con hipertensión",
        "dementia" => "con demencia",
        "copd" => "con enfermedad pulmonar",
        "heart_disease" => "con enfermedad del corazón",
        "arthritis" => "con artritis",
        "kidney_disease" => "con enfermedad renal",
        "cancer" => "con cáncer",
        "permanent" => "residentes permanentes",
        "temporary" => "de estancia temporal",
        "day_care" => "de estancia de día",
        "respite" => "en respiro familiar",
        "no_family_network" => "sin red familiar",
        "abandonment" => "por abandono",
        "dependency" => "por dependencia",
        "violence" => "por violencia",
        "orphanhood" => "por orfandad",
        "poverty" => "por pobreza",
        "health" => "por salud",
        "family_custody" => "con su familia como responsable",
        "dif_custody" => "bajo tutela del DIF",
        "adoption_process" => "en proceso de adopción",
        "other_process" => "en otro proceso legal",
        _ => "por otra razón",
    }
}

fn care_list(counts: &[crate::modules::care::api::Count]) -> String {
    counts.iter().map(|c| format!("{} {}", c.count, care_word(&c.code))).collect::<Vec<_>>().join(", ")
}

/// A finding of the board in one sentence (only those that may reach the AI come here).
fn finding_text(i: &Insight) -> Option<String> {
    let v = |k: &str| i.values.get(k).copied().unwrap_or(0);
    Some(match i.code {
        "mobility_vs_access" => format!(
            "{} personas usan silla de ruedas o están en cama, y {}: {}.",
            v("people"),
            if v("spaces") == 1 { "1 espacio no se puede usar en silla de ruedas".to_string() } else { format!("{} espacios no se pueden usar en silla de ruedas", v("spaces")) },
            i.items.join("; ")
        ),
        "waitlist_vs_seats" if v("free") >= 0 => format!("Hay {} solicitudes en lista de espera y {} plazas libres.", v("waiting"), v("free")),
        "waitlist_vs_seats" => format!("Hay {} solicitudes en lista de espera.", v("waiting")),
        "occupancy_high" => format!("La ocupación es de {} % de la capacidad.", v("percent")),
        "dependency_vs_carers" => format!(
            "{} personas necesitan mucho apoyo o apoyo en todo, y hay {} personas del personal en puestos de cuidado y salud.",
            v("dependent"),
            v("carers")
        ),
        "few_visits" => format!("{} personas reciben pocas o ninguna visita de su familia.", v("people")),
        "no_program" => format!("{} personas de 65 años o más no tienen pensión ni programa social.", v("people")),
        "school_lag" => format!("{} niñas, niños o adolescentes tienen rezago escolar.", v("people")),
        _ => return None,
    })
}

/// The people served: how many per group and, for groups of `MIN_GROUP` or more, their support, mobility, health,
/// stay and family; then the findings that cross them with the rest of the institution. Never who.
fn render_people(s: &mut String, people: &PeopleSheet, fee_payers: i64, estimate: Option<i64>, missing: &mut Vec<&str>) {
    let c = &people.summary;
    if c.served == 0 {
        // no records yet: the quick figure of the onboarding, said as approximate (ADR-031)
        match estimate {
            Some(n) => s.push_str(&format!("Personas atendidas (cifra aproximada que dio la persona; todavía sin registros): {n}.\n")),
            None => missing.push("a quiénes atiende"),
        }
        if c.waiting > 0 {
            s.push_str(&format!("Lista de espera: {} solicitudes.\n", c.waiting));
        }
        return;
    }
    for g in &c.by_group {
        s.push_str(&format!("Población: {} — {} {}.\n", g.code, g.count, if g.count == 1 { "persona" } else { "personas" }));
    }
    s.push_str(&format!("Total de personas atendidas: {}.\n", c.served));
    if fee_payers > 0 {
        s.push_str(&format!("De ellas, {fee_payers} pagan cuota de estancia.\n"));
    }
    if let Some(a) = c.average_age {
        s.push_str(&format!("Edad promedio: {a} años.\n"));
    }
    let only = format!("(solo grupos de {MIN_GROUP} o más personas)");
    for (title, list) in [
        ("Nivel de apoyo", &c.dependency),
        ("Movilidad", &c.mobility),
        ("Discapacidad", &c.disabilities),
        ("Condiciones de salud", &c.chronic),
        ("Modalidad de estancia", &c.stay_modes),
        ("Motivos de ingreso", &c.admission_reasons),
        ("Situación legal", &c.legal),
    ] {
        if !list.is_empty() {
            s.push_str(&format!("{title} {only}: {}.\n", care_list(list)));
        }
    }
    for (text, n) in [
        ("Reciben pocas o ninguna visita", c.few_visits),
        ("Tienen pensión o programa social", c.with_program),
        ("Hablan una lengua indígena", c.indigenous_language),
        ("Van a la escuela", c.attending_school),
        ("Tienen rezago escolar", c.school_lag),
    ] {
        if let Some(n) = n {
            s.push_str(&format!("{text}: {n}.\n"));
        }
    }
    if c.admitted_this_year > 0 || c.discharged_this_year > 0 {
        s.push_str(&format!("En el año ingresaron {} y egresaron {}.\n", c.admitted_this_year, c.discharged_this_year));
    }
    if c.waiting > 0 {
        s.push_str(&format!("Lista de espera: {} solicitudes.\n", c.waiting));
    }
    for f in people.findings.iter().filter_map(finding_text) {
        s.push_str(&format!("Hallazgo: {f}\n"));
    }
}

/// A finding of the board of the facilities in one sentence (only those that may reach the AI come here).
fn facility_finding_text(i: &Insight) -> Option<String> {
    let v = |k: &str| i.values.get(k).copied().unwrap_or(0);
    let often = |k: &str| if v(k) == 1 { "seguido" } else { "a veces" };
    Some(match i.code {
        "broken_spaces" => format!("Espacios en mal estado o que no se pueden usar: {}.", i.items.join("; ")),
        "broken_equipment" => format!("Equipo en mal estado o que no se puede usar: {}.", i.items.join("; ")),
        "structural" if v("groups") == 1 => "1 grupo de espacios tiene grietas o fallas en la instalación eléctrica: conviene revisar la seguridad del edificio.".to_string(),
        "structural" => format!("{} grupos de espacios tienen grietas o fallas en la instalación eléctrica: conviene revisar la seguridad del edificio.", v("groups")),
        "only_stairs" if v("people") > 0 => format!(
            "El inmueble tiene varios pisos y solo escaleras entre ellos; hay {} espacios arriba y {} personas usan silla de ruedas o están en cama.",
            v("spaces"),
            v("people")
        ),
        "only_stairs" => format!("El inmueble tiene varios pisos y solo escaleras entre ellos; hay {} espacios arriba.", v("spaces")),
        "bathrooms_without_bars" => format!("{} de {} baños no tienen barras de apoyo.", v("without"), v("bathrooms")),
        "beds_short" if v("capacity") >= 0 => format!("Hay {} camas para {} personas atendidas y una capacidad de {}.", v("beds"), v("served"), v("capacity")),
        "beds_short" => format!("Hay {} camas para {} personas atendidas.", v("beds"), v("served")),
        "tenure_weak" => "El inmueble no es propio ni está en comodato: muchas convocatorias de obra piden que lo sea.".to_string(),
        "tenure_ending" => format!("El comodato del inmueble termina en {} (faltan {} años).", v("until"), v("years")),
        "tenure_undocumented" => "No hay papeles que acrediten la posesión del inmueble (escritura o contrato).".to_string(),
        "civil_protection_gap" => {
            let parts: Vec<&str> = i
                .items
                .iter()
                .map(|x| match x.as_str() {
                    "internal_program_no" => "no tiene programa interno de protección civil",
                    "internal_program_in_progress" => "el programa interno de protección civil está en trámite",
                    _ => "no tiene dictamen o visto bueno de protección civil",
                })
                .collect();
            format!("Protección civil: {}.", parts.join("; "))
        }
        "fire_safety_gap" => {
            let parts: Vec<&str> = i
                .items
                .iter()
                .map(|x| match x.as_str() {
                    "no_extinguishers" => "no hay extintores",
                    "extinguishers_expired" => "los extintores no tienen la recarga al día",
                    _ => "no hay detectores de humo",
                })
                .collect();
            format!("Seguridad contra incendios: {}.", parts.join("; "))
        }
        "water_shortage" => format!("Falta el agua {}.", often("often")),
        "power_without_backup" => format!("Se va la luz {} y no hay una planta de luz de emergencia que funcione.", often("often")),
        _ => return None,
    })
}

/// The facilities: the building, its services and safety, each group of spaces and equipment with how many are in
/// each state, a few ratios made by code, and the findings of their board.
fn render_facilities(s: &mut String, f: &FacilitiesSheet, missing: &mut Vec<&str>) {
    for site in &f.sites {
        for line in facility_text::site_lines(&site.site) {
            s.push_str(&format!("{line}\n"));
        }
        for g in &site.spaces {
            s.push_str(&format!("Espacio: {}\n", facility_text::space_line(g)));
        }
        for g in &site.equipment {
            s.push_str(&format!("Equipo: {}\n", facility_text::equipment_line(g)));
        }
    }
    if !f.sites.iter().any(|x| !x.spaces.is_empty()) {
        missing.push("las instalaciones");
        return;
    }
    let b = &f.board;
    let mut ratios = Vec::new();
    if let Some(m) = b.built_m2_per_person {
        ratios.push(format!("{m} m² construidos por persona atendida"));
    }
    if let Some(p) = b.people_per_bathroom {
        // «4 personas por baño», «4.5 personas por baño»: no «.0» the figure check would read as a 0
        ratios.push(if p.fract() == 0.0 { format!("{p:.0} personas por baño") } else { format!("{p:.1} personas por baño") });
    }
    if let Some(beds) = b.indicators.beds {
        ratios.push(format!("{beds} camas en total"));
    }
    if !ratios.is_empty() {
        s.push_str(&format!("Instalaciones, calculado: {}.\n", ratios.join("; ")));
    }
    for line in b.insights.iter().filter(|i| i.for_ai).filter_map(facility_finding_text) {
        s.push_str(&format!("Hallazgo: {line}\n"));
    }
}

fn attention_word(code: &str) -> &'static str {
    match code {
        "early_childhood" => "primera infancia",
        "childhood" => "niñez",
        "adolescence" => "adolescencia",
        "youth" => "juventud",
        "adults" => "personas adultas",
        "older_adults" => "personas mayores",
        "residential" => "residencial (viven en la institución)",
        "day_care" => "estancia de día",
        "outpatient" => "ambulatoria o de consulta",
        "community" => "comunitaria",
        "home_care" => "en el domicilio",
        "care" => "cuidado",
        "health" => "salud",
        "disability" => "discapacidad",
        "education" => "educación",
        "food" => "alimentación",
        "violence" => "atención a la violencia",
        "addictions" => "adicciones",
        "street" => "situación de calle",
        "migration" => "migración",
        "mental_health" => "salud mental",
        _ => "otra",
    }
}

/// Whom and how it serves (ADR-033 §2): calls filter by population and by kind of service.
fn render_attention(s: &mut String, attention: Option<&crate::core::profile::domain::Attention>, missing: &mut Vec<&str>) {
    let words = |codes: &[String]| codes.iter().map(|c| attention_word(c)).collect::<Vec<_>>().join(", ");
    let Some(a) = attention.filter(|a| !a.populations.is_empty()) else {
        missing.push("a quién atiende");
        return;
    };
    let sex = match a.sex_served.as_deref() {
        Some("women") => " (solo mujeres)",
        Some("men") => " (solo hombres)",
        Some(_) => " (mujeres y hombres)",
        None => "",
    };
    s.push_str(&format!("A quién atiende: {}{sex}.\n", words(&a.populations)));
    if !a.modalities.is_empty() {
        s.push_str(&format!("Cómo atiende: {}.\n", words(&a.modalities)));
    }
    if !a.care_areas.is_empty() {
        s.push_str(&format!("Áreas de atención: {}.\n", words(&a.care_areas)));
    }
}

/// A datum kept as a field of the catalog (ADR-033), if it may reach the AI and is written. A field the catalog
/// keeps from the AI never comes out of here, whatever the caller asks.
fn detail<'a>(inst: &'a crate::core::profile::domain::InstitutionInput, id: &str) -> Option<&'a serde_json::Value> {
    use crate::common::forms::AiUse;
    let allowed = crate::core::institution::forms::forms().iter().find_map(|f| f.field(id)).is_some_and(|f| f.ai != AiUse::Never);
    inst.details.as_ref().and_then(|d| d.get(id)).filter(|v| allowed && crate::common::forms::is_filled(Some(v)))
}

fn detail_text<'a>(inst: &'a crate::core::profile::domain::InstitutionInput, id: &str) -> Option<&'a str> {
    detail(inst, id).and_then(serde_json::Value::as_str).map(str::trim)
}

fn tax_regime_text(code: &str) -> &'static str {
    match code {
        "non_profit" => "persona moral con fines no lucrativos",
        "general" => "régimen general de ley",
        _ => "otro",
    }
}

fn donee_category_text(code: &str) -> &'static str {
    match code {
        "assistance" => "asistencial",
        "education" => "educativa",
        "research" => "investigación científica o tecnológica",
        "culture" => "cultural",
        "scholarships" => "becante",
        "ecology" => "ecológica",
        "species" => "protección de especies en peligro",
        "support_donees" => "apoyo económico a otras donatarias",
        "public_works" => "obras o servicios públicos",
        "libraries" => "bibliotecas privadas",
        "museums" => "museos privados",
        _ => "desarrollo social",
    }
}

/// Whom it takes in, beyond whom it serves: the ages, the services and the rules of admission (ADR-033).
fn render_admission(s: &mut String, inst: &crate::core::profile::domain::InstitutionInput) {
    let age = |id| detail(inst, id).and_then(serde_json::Value::as_i64);
    match (age("institution.age_min"), age("institution.age_max")) {
        (Some(a), Some(b)) => s.push_str(&format!("Edades que recibe: de {a} a {b} años.\n")),
        (Some(a), None) => s.push_str(&format!("Edades que recibe: desde {a} años.\n")),
        (None, Some(b)) => s.push_str(&format!("Edades que recibe: hasta {b} años.\n")),
        (None, None) => {}
    }
    if let Some(x) = detail_text(inst, "institution.services") {
        s.push_str(&format!("Servicios o programas: {x}\n"));
    }
    if let Some(x) = detail_text(inst, "institution.admission_criteria") {
        s.push_str(&format!("Criterios de ingreso: {x}\n"));
    }
}

/// Where the institution is and what it is, legally (ADR-031): most calls filter by these. None of it is personal:
/// the address, the RFC, the folios and the keys never come here.
/// The board of the institution: how many people and which roles, never a name (ADR-033 §3).
fn render_board(s: &mut String, board: &BoardSheet, missing: &mut Vec<&str>) {
    if board.total == 0 {
        missing.push("su patronato u órgano de gobierno");
        return;
    }
    let roles: Vec<String> = board
        .roles
        .iter()
        .map(|(code, n)| {
            let word = match *code {
                "president" => "presidencia",
                "vice_president" => "vicepresidencia",
                "secretary" => "secretaría",
                "treasurer" => "tesorería",
                "member" => "vocal",
                _ => "otro cargo",
            };
            if *n == 1 {
                word.to_string()
            } else {
                format!("{word} ({n} personas)")
            }
        })
        .collect();
    s.push_str(&format!("Patronato u órgano de gobierno: {} — {}. Sus nombres no se comparten.\n", people(board.total as i64), roles.join(", ")));
}

fn render_identity(s: &mut String, inst: &crate::core::profile::domain::InstitutionInput, year: i64, missing: &mut Vec<&str>) {
    use crate::core::institution::catalog::{state_name, UNDER_A_JUNTA};
    match detail_text(inst, "institution.legal_name") {
        Some(n) => s.push_str(&format!("Nombre legal: {n}.\n")),
        None => missing.push("el nombre legal"),
    }
    match detail_text(inst, "institution.purpose") {
        Some(p) => s.push_str(&format!("Objeto social (lo que dicen sus estatutos): {p}\n")),
        None => missing.push("el objeto social"),
    }
    render_attention(s, inst.attention.as_ref(), missing);
    render_admission(s, inst);
    let state = inst.state.as_deref().and_then(state_name);
    match (text(&inst.municipality), state) {
        (Some(m), Some(st)) => s.push_str(&format!("Ubicación: {m}, {st}.\n")),
        (None, Some(st)) => s.push_str(&format!("Ubicación: {st}.\n")),
        (Some(m), None) => s.push_str(&format!("Ubicación: {m}.\n")),
        (None, None) => missing.push("dónde está"),
    }
    match inst.founded_year {
        Some(y) if y < year => s.push_str(&format!("Fundada en {y} ({} años de operación).\n", year - y)),
        Some(y) => s.push_str(&format!("Fundada en {y}.\n")),
        None => missing.push("el año de fundación"),
    }
    match inst.legal_form.as_deref() {
        Some(f) => s.push_str(&format!(
            "Figura jurídica: {}.\n",
            match f {
                "ac" => "asociación civil (A.C.)",
                "iap" => "institución de asistencia privada (I.A.P.)",
                "ibp" => "institución de beneficencia privada (I.B.P.)",
                "sc" => "sociedad civil (S.C.)",
                "abp" => "asociación de beneficencia privada (A.B.P.)",
                "religious" => "asociación religiosa",
                _ => "otra",
            }
        )),
        None => missing.push("la figura jurídica"),
    }
    let registry = |v: &Option<String>| match v.as_deref() {
        Some("yes") => Some("sí"),
        Some("in_progress") => Some("en trámite"),
        Some("no") => Some("no"),
        _ => None,
    };
    if inst.legal_form.as_deref().is_some_and(|f| UNDER_A_JUNTA.contains(&f)) {
        // that it is registered, never its folio
        let registered = inst.details.as_ref().is_some_and(|d| crate::common::forms::is_filled(d.get("institution.junta_folio")));
        if registered {
            s.push_str("Registrada ante la Junta de Asistencia Privada de su estado.\n");
        } else {
            missing.push("su registro ante la Junta de Asistencia Privada");
        }
    }
    match detail_text(inst, "institution.tax_regime") {
        Some(r) => s.push_str(&format!("Régimen fiscal: {}.\n", tax_regime_text(r))),
        None => missing.push("el régimen fiscal"),
    }
    match registry(&inst.authorized_donee) {
        Some(w) => s.push_str(&format!("Donataria autorizada por el SAT: {w}.\n")),
        None => missing.push("si es donataria autorizada"),
    }
    if inst.authorized_donee.as_deref() == Some("yes") {
        if let Some(c) = detail_text(inst, "institution.donee_category") {
            s.push_str(&format!("Rubro autorizado como donataria: {}.\n", donee_category_text(c)));
        }
    }
    match registry(&inst.cluni) {
        Some(w) => s.push_str(&format!("CLUNI (registro federal de organizaciones de la sociedad civil): {w}.\n")),
        None => missing.push("si tiene CLUNI"),
    }
}

/// The parts of the sheet an agent may ask for one at a time (ADR-034 §2), in the order of the whole sheet.
pub const SECTIONS: &[&str] = &["institution", "money", "people", "staff", "facilities"];

/// One part of the sheet: where its text and what it misses are inside the whole.
struct Part {
    key: &'static str,
    text: std::ops::Range<usize>,
    missing: std::ops::Range<usize>,
}

/// The whole sheet, with where each part begins and ends, before the line of what is missing.
struct Composed {
    text: String,
    /// The line that says whether the data are confirmed.
    header: std::ops::Range<usize>,
    parts: Vec<Part>,
    notes: std::ops::Range<usize>,
    missing: Vec<String>,
}

pub fn render(p: &StoredProfile, fin: &FinanceInput, staff: &StaffSummary, people: &PeopleSheet, facilities: &FacilitiesSheet, board: &BoardSheet) -> String {
    let c = compose(p, fin, staff, people, facilities, board);
    let mut s = c.text;
    if !c.missing.is_empty() {
        s.push_str(&format!(
            "No capturado en «Mi institución»: {}. «No capturado» quiere decir que no se sabe, no que sea cero.\n",
            c.missing.join(", ")
        ));
    }
    s
}

/// One part of the sheet (`SECTIONS`), with the line of whether it is confirmed and what is missing in it. The
/// notes of the institution go with its own part. `None` for a part that does not exist.
pub fn render_section(
    key: &str,
    p: &StoredProfile,
    fin: &FinanceInput,
    staff: &StaffSummary,
    people: &PeopleSheet,
    facilities: &FacilitiesSheet,
    board: &BoardSheet,
) -> Option<String> {
    let c = compose(p, fin, staff, people, facilities, board);
    let part = c.parts.iter().find(|x| x.key == key)?;
    let mut s = format!("{}{}", &c.text[c.header.clone()], &c.text[part.text.clone()]);
    if key == "institution" {
        s.push_str(&c.text[c.notes.clone()]);
    }
    let missing = &c.missing[part.missing.clone()];
    if !missing.is_empty() {
        s.push_str(&format!("No capturado en esta parte: {}. «No capturado» quiere decir que no se sabe, no que sea cero.\n", missing.join(", ")));
    }
    Some(s)
}

/// The sheet of the current profile, one part (`render_section`). Without a profile, it says there is nothing.
pub fn section_context(conn: &Connection, key: &str) -> Result<Option<String>, ServiceError> {
    if !SECTIONS.contains(&key) {
        return Ok(None);
    }
    Ok(Some(match profile_store::load_current(conn)? {
        Some(p) => render_section(
            key,
            &p,
            &crate::modules::finance::api::lines(conn)?,
            &crate::modules::hr::api::ai_summary(conn)?,
            &people_sheet(conn)?,
            &facilities_sheet(conn)?,
            &crate::core::protected::ai_board(conn)?,
        )
        .unwrap_or_default(),
        None => "Perfil: sin datos. Todavía no hay nada capturado en «Mi institución».".into(),
    }))
}

fn compose(p: &StoredProfile, fin: &FinanceInput, staff: &StaffSummary, people: &PeopleSheet, facilities: &FacilitiesSheet, board: &BoardSheet) -> Composed {
    let i = &p.input;
    let t = i.totals(p.as_of_year);
    let money = balance::finances(fin, &crate::core::bridge::finance::derived_from(&t));
    let mut s = String::new();
    let mut missing: Vec<&str> = Vec::new();
    let mut parts: Vec<Part> = Vec::new();
    // closes the part that began at the last cut
    let cut = |parts: &mut Vec<Part>, key: &'static str, s: &String, missing: &Vec<&str>, from: (usize, usize)| {
        parts.push(Part { key, text: from.0..s.len(), missing: from.1..missing.len() });
        (s.len(), missing.len())
    };

    s.push_str(if p.confirmed_at.is_some() {
        "Datos de «Mi institución», confirmados por la persona.\n"
    } else {
        "Datos de «Mi institución». Es un BORRADOR: la persona todavía no lo confirma.\n"
    });
    let header = 0..s.len();
    let mut from = (s.len(), 0);
    s.push_str(&format!("Institución: {} ({}).\n", i.institution.name, kind_text(i.institution.kind)));
    match text(&i.institution.mission) {
        Some(m) => s.push_str(&format!("A qué se dedica: {m}\n")),
        None => missing.push("a qué se dedica"),
    }
    render_identity(&mut s, &i.institution, p.as_of_year, &mut missing);
    render_board(&mut s, board, &mut missing);
    match i.capacity_total {
        Some(c) => s.push_str(&format!("Capacidad total: {c} personas.\n")),
        None => missing.push("capacidad total"),
    }
    from = cut(&mut parts, "institution", &s, &missing, from);

    // income: what the person wrote, by kind; the roster fees only as how many pay
    let roster_fees = money.income.iter().any(|l| l.kind == BENEFICIARY_FEES);
    if roster_fees {
        s.push_str(&format!(
            "Ingreso — cuotas de los beneficiarios (del padrón): las pagan {} {}; el monto no se comparte.\n",
            t.fee_payers,
            if t.fee_payers == 1 { "persona" } else { "personas" }
        ));
    }
    let written: Vec<_> = money.income.iter().filter(|l| l.counted && l.index.is_some()).collect();
    for l in &written {
        let inc = &fin.income[l.index.unwrap_or_default()];
        s.push_str(&format!("Ingreso — {}: {} — {}.\n", income_kind_text(inc.kind), inc.label, amount_text(inc.amount_mxn, inc.period)));
    }
    let written_sum: i64 = written.iter().filter_map(|l| l.annual_mxn).sum();
    if written_sum > 0 {
        s.push_str(&format!(
            "Suma de los ingresos escritos a mano: {} pesos al año{}.\n",
            mxn(written_sum),
            if roster_fees { " (sin las cuotas del padrón)" } else { "" }
        ));
    }
    if !roster_fees && written.is_empty() {
        missing.push("de dónde vienen sus ingresos");
    }

    // expenses: the approximate figure, the list, and the payroll only as words
    if let Some(b) = fin.annual_budget_mxn {
        s.push_str(&format!("Gasto anual aproximado (cifra a ojo de la persona, todo incluido): {} pesos.\n", mxn(b)));
    }
    let listed: Vec<_> = money.expenses.iter().filter(|l| l.kind == EXPENSE).collect();
    for l in &listed {
        let e = &fin.expenses[l.index.unwrap_or_default()];
        s.push_str(&format!("Egreso: {} — {}.\n", e.label, amount_text(e.amount_mxn, e.period)));
    }
    if t.payroll_cost_annual_mxn > 0 {
        s.push_str(match money.expenses_basis {
            ExpenseBasis::Estimate => "La nómina del personal (con aguinaldo y prima vacacional) va dentro del gasto aproximado; el monto no se comparte.\n",
            _ => "Egreso: nómina del personal con aguinaldo y prima vacacional (del padrón); el monto no se comparte.\n",
        });
    }
    if t.staff_support_annual_mxn > 0 {
        s.push_str("Egreso: aportaciones a la congregación y apoyos de servicio social (del padrón); el monto no se comparte.\n");
    }
    if t.external_staff_annual_mxn > 0 {
        s.push_str("Egreso: personal de empresas externas (del padrón); el monto no se comparte.\n");
    }
    let listed_sum: i64 = listed.iter().filter_map(|l| l.annual_mxn).sum();
    if listed_sum > 0 {
        s.push_str(&format!(
            "Suma de los egresos escritos a mano: {} pesos al año{}.\n",
            mxn(listed_sum),
            if t.payroll_cost_annual_mxn > 0 { " (sin la nómina)" } else { "" }
        ));
    }
    if money.expenses_basis == ExpenseBasis::Unknown {
        missing.push("cuánto gasta al año");
    }
    if let Some(b) = money.balance_annual_mxn {
        let against = if money.expenses_basis == ExpenseBasis::List { "la lista de egresos" } else { "el gasto anual aproximado" };
        let verdict = match b {
            b if b < 0 => "NO alcanzan a cubrir los egresos: hay déficit",
            0 => "alcanzan justo para cubrir los egresos",
            _ => "alcanzan a cubrir los egresos y sobra algo",
        };
        s.push_str(&format!("Balance del año, calculado con lo capturado y {against}: los ingresos {verdict}.\n"));
    }
    from = cut(&mut parts, "money", &s, &missing, from);

    // people served: from their module, as counts; never who (ADR-029)
    render_people(&mut s, people, t.fee_payers, i.served_estimate, &mut missing);
    from = cut(&mut parts, "people", &s, &missing, from);

    // staff: from the staff module, as positions and counts; never a person, a pay or a date (ADR-027)
    render_staff(&mut s, staff, (i.staff_paid_estimate, i.staff_volunteer_estimate), &mut missing);
    from = cut(&mut parts, "staff", &s, &missing, from);

    // facilities: from their module, as groups that count how many are in each state (ADR-030)
    render_facilities(&mut s, facilities, &mut missing);
    cut(&mut parts, "facilities", &s, &missing, from);

    let notes_from = s.len();
    if let Some(n) = text(&i.notes) {
        s.push_str(&format!("Notas de la institución: {n}\n"));
    }
    let notes = notes_from..s.len();
    let missing = missing.into_iter().map(str::to_string).collect();
    Composed { text: s, header, parts, notes, missing }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::core::profile::domain::*;
    use crate::modules::finance::domain::lines::{ExpenseItemInput, IncomeSourceInput};
    use crate::storage::open_encrypted;

    const KEY: &str = "00112233445566778899aabbccddeeff00112233445566778899aabbccddeeff";

    fn db() -> (tempfile::TempDir, Connection) {
        let dir = tempfile::tempdir().unwrap();
        let c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        (dir, c)
    }

    /// A profile with every kind of datum, for the tests that check the AI gets all of it.
    pub(crate) fn rich() -> ProfileInput {
        ProfileInput {
            institution: InstitutionInput {
                name: "Asilo Ficticio".into(),
                kind: InstitutionKind::ElderlyHome,
                mission: Some("Un hogar digno para adultos mayores.".into()),
                legal_rfc: Some("AFI200101AB1".into()),
                contact_phone: Some("55 5555 0101".into()),
                legal_rep_name: Some("Rosa Representante".into()),
                state: Some("jal".into()),
                municipality: Some("Zapopan".into()),
                founded_year: Some(1987),
                legal_form: Some("ac".into()),
                authorized_donee: Some("yes".into()),
                cluni: Some("no".into()),
                details: Some(
                    serde_json::from_value(serde_json::json!({
                        "institution.legal_name": "Asilo Ficticio, A.C.",
                        "institution.purpose": "Dar asistencia a personas mayores sin familia.",
                        "institution.services": "Residencia, comedor y terapia física.",
                        "institution.age_min": 60,
                        "institution.age_max": 95,
                        "institution.admission_criteria": "Personas mayores que no pueden vivir solas.",
                        "institution.tax_regime": "non_profit",
                        "institution.donee_category": "assistance",
                        // the institution's own: never to the AI
                        "institution.street": "Avenida Escondida",
                        "institution.ext_number": "4321",
                        "institution.postal_code": "45010",
                        "institution.fiscal_postal_code": "45011",
                        "institution.donee_letter_number": "600-04-02-777",
                        "institution.donee_letter_date": "2015-05-04",
                        "institution.legal_rep_valid_until": "2030-01-31",
                    }))
                    .unwrap(),
                ),
                ..Default::default()
            },
            capacity_total: Some(25),
            notes: Some("Perfil ficticio para pruebas.".into()),
            // the records of the modules win while the modules are shown; hidden (ADR-043), these are the figures
            served_estimate: Some(40),
            staff_paid_estimate: Some(30),
            staff_volunteer_estimate: Some(10),
            population: vec![
                PopulationGroupInput { label: "Adultos mayores".into(), count: 8, age_min: Some(70), age_max: Some(95), dependency_level: Some(DependencyLevel::High), paying_count: Some(5), monthly_fee_mxn: Some(3_333), ..Default::default() },
                PopulationGroupInput { label: "Adultos mayores".into(), count: 3, age_min: Some(66), age_max: Some(80), dependency_level: Some(DependencyLevel::High), ..Default::default() },
                PopulationGroupInput { label: "Hombres".into(), count: 1, age_min: Some(93), age_max: Some(93), dependency_level: Some(DependencyLevel::Total), ..Default::default() },
            ],
            staff: vec![
                StaffGroupInput { role: "Cuidadora".into(), count: 4, paid: true, monthly_salary_mxn: Some(7_777), shift: Some("noche".into()), contract: Some(ContractKind::Permanent), start_year: Some(2019), ..Default::default() },
                StaffGroupInput { role: "Voluntaria".into(), count: 3, paid: false, ..Default::default() },
            ],
        }
    }

    /// The money of `rich()`, in the finance module (ADR-032).
    pub(crate) fn rich_money() -> FinanceInput {
        FinanceInput {
            annual_budget_mxn: Some(1_800_000),
            income: vec![
                IncomeSourceInput { label: "Padrinos".into(), kind: IncomeKind::RecurringDonor, amount_mxn: Some(10_000), period: Period::Monthly },
                IncomeSourceInput { label: "Donativos".into(), kind: IncomeKind::OccasionalDonation, amount_mxn: Some(680_000), period: Period::Annual },
                // the roster has the real fees: this one would count them twice and is left out
                IncomeSourceInput { label: "Cuotas aprox.".into(), kind: IncomeKind::FeeEstimate, amount_mxn: Some(450_000), period: Period::Annual },
            ],
            expenses: vec![
                ExpenseItemInput { label: "Alimentos".into(), amount_mxn: Some(15_000), period: Period::Monthly },
                ExpenseItemInput { label: "Luz y agua".into(), amount_mxn: Some(36_000), period: Period::Annual },
            ],
        }
    }

    /// The staff of `rich()` in the staff module: 4 night caregivers with pay and 3 volunteers.
    pub(crate) fn seed_rich_staff(c: &mut Connection) {
        use crate::modules::hr::domain::person::PersonData;
        use crate::modules::hr::domain::position::PositionInput;
        use crate::modules::hr::storage as hr;
        let carer = hr::insert_position(c, &PositionInput {
            title: "Cuidadora".into(), area: Some("care".into()), duties: Some("Atiende a los residentes de noche.".into()), authorized_seats: Some(5), ..Default::default()
        }).unwrap();
        let volunteer = hr::insert_position(c, &PositionInput { title: "Voluntaria".into(), ..Default::default() }).unwrap();
        let year: i64 = hr::today(c).unwrap()[..4].parse().unwrap();
        for i in 0..4 {
            let d = PersonData {
                first_names: format!("Cuidadora Secreta {i}"), position_id: Some(carer.clone()), modality: "indefinite".into(), status: "active".into(),
                pay_amount_mxn: Some(7_777), pay_period: Some("monthly".into()), shift: Some("night".into()), schedule: Some("full_time".into()),
                start_date: Some(format!("{}-01-01", year - 7)), education: Some("high_school".into()), curp: Some("HEGG560427MVZRRL04".into()),
                ..Default::default()
            };
            hr::save_person(c, None, &d, false).unwrap();
        }
        for i in 0..3 {
            let d = PersonData { first_names: format!("Voluntaria Oculta {i}"), position_id: Some(volunteer.clone()), modality: "volunteer".into(), status: "active".into(), ..Default::default() };
            hr::save_person(c, None, &d, false).unwrap();
        }
    }

    /// The people served of `rich()` in their module: 11 women and 1 man, with the data the findings cross.
    pub(crate) fn seed_rich_people(c: &mut Connection) {
        use crate::modules::care::domain::catalog::Flavor;
        use crate::modules::care::domain::person::BeneficiaryData;
        let year: i64 = crate::modules::care::storage::today(c).unwrap()[..4].parse().unwrap();
        let mut people = Vec::new();
        for i in 0..4 {
            people.push(BeneficiaryData { first_names: format!("Abuelita Reservada {i}"), sex: Some("female".into()), birth_date: Some(format!("{}-01-01", year - 84)), dependency: Some("high".into()), mobility: Some("wheelchair".into()), visits: Some("never".into()), curp: Some("HEGG560427MVZRRL04".into()), ..Default::default() });
        }
        for i in 0..4 {
            people.push(BeneficiaryData { first_names: format!("Señora Privada {i}"), sex: Some("female".into()), birth_date: Some(format!("{}-01-01", year - 72)), dependency: Some("high".into()), mobility: Some("independent".into()), ..Default::default() });
        }
        for i in 0..3 {
            people.push(BeneficiaryData { first_names: format!("Doña Callada {i}"), sex: Some("female".into()), birth_date: Some(format!("{}-01-01", year - 67)), dependency: Some("medium".into()), ..Default::default() });
        }
        people.push(BeneficiaryData { first_names: "Don Discreto".into(), sex: Some("male".into()), birth_date: Some(format!("{}-01-01", year - 93)), dependency: Some("total".into()), mobility: Some("bedridden".into()), ..Default::default() });
        for mut d in people {
            d.status = "active".into();
            let crate::modules::care::service::SaveOutcome::Saved { .. } = crate::modules::care::service::save_person(c, Flavor::ElderlyHome, None, d).unwrap() else { panic!("saved") };
        }
    }

    /// The facilities of `rich()` in their module: a house of two floors with only stairs, three bathrooms on the
    /// ground floor (one in poor state, none with grab bars), a kitchen, six bedrooms upstairs and two washers.
    pub(crate) fn seed_rich_board(c: &mut Connection) {
        use crate::core::protected::{save, ListKind};
        for (name, role) in [("Presidenta Ficticia", "president"), ("Tesorero Ficticio", "treasurer"), ("Vocal Ficticia", "member")] {
            let v = serde_json::from_value(serde_json::json!({ "institution.board.name": name, "institution.board.role": role })).unwrap();
            save(c, ListKind::BoardMember, None, v).unwrap();
        }
        let v = serde_json::from_value(serde_json::json!({ "institution.bank.clabe": "002010077777777771", "institution.bank.purpose": ["donations"] })).unwrap();
        save(c, ListKind::BankAccount, None, v).unwrap();
    }

    pub(crate) fn seed_rich_facilities(c: &Connection) {
        use crate::modules::facilities::domain::group::{EquipmentData, SpaceData, States};
        use crate::modules::facilities::domain::site::SiteData;
        use crate::modules::facilities::service::{self as fac, SaveOutcome};
        let ok = |o: SaveOutcome| assert!(matches!(o, SaveOutcome::Saved), "{o:?}");
        ok(fac::save_site(c, SiteData {
            name: "Casa principal".into(),
            built_m2: Some(600),
            floors: Some(2),
            floor_access: vec!["none".into()],
            tenure: Some("own".into()),
            tenure_documented: Some(true),
            water_sources: vec!["network".into()],
            extinguishers: Some(4),
            extinguishers_current: Some(true),
            internal_program: Some("yes".into()),
            ..Default::default()
        }).unwrap());
        ok(fac::save_space(c, None, SpaceData {
            kind: "bathroom".into(), count: 3, states: States { good: 2, poor: 1, ..Default::default() }, problems: vec!["floor".into(), "grab_bars".into()],
            accessible: Some(false), grab_bars: Some(false), notes: Some("Piso resbaloso, sin barras.".into()), ..Default::default()
        }).unwrap());
        ok(fac::save_space(c, None, SpaceData { kind: "kitchen".into(), count: 1, states: States { good: 1, ..Default::default() }, ..Default::default() }).unwrap());
        ok(fac::save_space(c, None, SpaceData {
            kind: "bedroom".into(), floor: 1, count: 6, states: States { good: 6, ..Default::default() }, beds: Some(20), hospital_beds: Some(2), ..Default::default()
        }).unwrap());
        ok(fac::save_equipment(c, None, EquipmentData { kind: "washer".into(), count: 2, states: States { good: 1, unusable: 1, ..Default::default() }, ..Default::default() }).unwrap());
    }

    fn saved(input: &ProfileInput, confirm: bool) -> (tempfile::TempDir, Connection) {
        saved_with(input, &rich_money(), confirm)
    }

    fn saved_with(input: &ProfileInput, money: &FinanceInput, confirm: bool) -> (tempfile::TempDir, Connection) {
        let (d, mut c) = db();
        seed_rich_staff(&mut c);
        seed_rich_people(&mut c);
        seed_rich_facilities(&c);
        seed_rich_board(&mut c);
        profile_store::save(&mut c, input, &profile_store::fixtures()).unwrap();
        crate::modules::finance::storage::save(&mut c, money).unwrap();
        if confirm {
            profile_store::finish_first_start(&c);
        }
        (d, c)
    }

    /// What the sheet must carry, whichever stage asks: one line of the sheet per fact of «Mi institución».
    pub(crate) const RICH_FACTS: &[&str] = &[
        "Asilo Ficticio (asilo)",
        "A qué se dedica: Un hogar digno para adultos mayores.",
        "Ubicación: Zapopan, Jalisco.",
        "Figura jurídica: asociación civil (A.C.).",
        "Donataria autorizada por el SAT: sí.",
        "CLUNI (registro federal de organizaciones de la sociedad civil): no.",
        "Capacidad total: 25 personas.",
        "Patronato u órgano de gobierno: 3 personas — presidencia, tesorería, vocal. Sus nombres no se comparten.",
        "Gasto anual aproximado (cifra a ojo de la persona, todo incluido): $1,800,000 pesos.",
        "Ingreso — cuotas de los beneficiarios (del padrón): las pagan 5 personas; el monto no se comparte.",
        "Ingreso — donante fijo: Padrinos — $10,000 pesos al mes ($120,000 al año).",
        "Ingreso — donativo ocasional: Donativos — $680,000 pesos al año.",
        "Suma de los ingresos escritos a mano: $800,000 pesos al año (sin las cuotas del padrón).",
        "Egreso: Alimentos — $15,000 pesos al mes ($180,000 al año).",
        "Egreso: Luz y agua — $36,000 pesos al año.",
        "Egreso: nómina del personal con aguinaldo y prima vacacional (del padrón); el monto no se comparte.",
        "Suma de los egresos escritos a mano: $216,000 pesos al año (sin la nómina).",
        "Balance del año, calculado con lo capturado y la lista de egresos: los ingresos alcanzan a cubrir los egresos y sobra algo.",
        "Población: Mujeres de 80 a 89 años — 4 personas.",
        "Población: Mujeres de 70 a 79 años — 4 personas.",
        "Población: Mujeres de 60 a 69 años — 3 personas.",
        "Población: Hombres de 90 años o más — 1 persona.",
        "Total de personas atendidas: 12.",
        "De ellas, 5 pagan cuota de estancia.",
        "Nivel de apoyo (solo grupos de 3 o más personas): 3 apoyo regular, 8 mucho apoyo.",
        "Movilidad (solo grupos de 3 o más personas): 4 caminan sin ayuda, 4 usan silla de ruedas.",
        "Reciben pocas o ninguna visita: 4.",
        "Hallazgo: 5 personas usan silla de ruedas o están en cama, y 3 espacios no se pueden usar en silla de ruedas: 3 baños (planta baja).",
        "Hallazgo: 9 personas necesitan mucho apoyo o apoyo en todo, y hay 4 personas del personal en puestos de cuidado y salud.",
        "Hallazgo: 4 personas reciben pocas o ninguna visita de su familia.",
        "Hallazgo: 12 personas de 65 años o más no tienen pensión ni programa social.",
        "Personal: Cuidadora (cuidado) — 4 personas (4 con sueldo; jornada: 4 tiempo completo; turno: 4 nocturno). Plazas autorizadas: 5; sin cubrir: 1.",
        "Funciones del puesto Cuidadora: Atiende a los residentes de noche.",
        "Personal: Voluntaria — 3 personas (3 de voluntariado).",
        "Total del personal: 7 personas: 4 con sueldo, 3 de voluntariado.",
        "Escolaridad del personal (solo grupos de 3 o más personas): 4 con preparatoria o carrera técnica.",
        "Antigüedad en la institución (solo grupos de 3 o más personas): 4 de 5 a 9 años.",
        "Inmueble «Casa principal»: 2 pisos contando la planta baja; entre pisos: solo escaleras; 600 m² construidos.",
        "El inmueble es propio; tiene papeles que lo acreditan (escritura o contrato).",
        "Servicios: agua de red municipal.",
        "Seguridad y protección civil: 4 extintores con la recarga al día; programa interno de protección civil: sí.",
        "Espacio: Baños, planta baja: 3 (2 bien, 1 mal). Fallas: piso dañado o resbaloso, faltan barras de apoyo o pasamanos. No se pueden usar en silla de ruedas. Sin barras de apoyo. Nota: Piso resbaloso, sin barras.",
        "Espacio: Cocina, planta baja: 1 (1 bien).",
        "Espacio: Dormitorios, primer piso: 6 (6 bien). 20 camas, 2 de hospital.",
        "Equipo: Lavadoras: 2 (1 bien, 1 no se puede usar).",
        "Instalaciones, calculado: 50 m² construidos por persona atendida; 4 personas por baño; 20 camas en total.",
        "Hallazgo: Espacios en mal estado o que no se pueden usar: 1 de 3 baños (planta baja).",
        "Hallazgo: Equipo en mal estado o que no se puede usar: 1 de 2 lavadoras.",
        "Hallazgo: El inmueble tiene varios pisos y solo escaleras entre ellos; hay 6 espacios arriba y 5 personas usan silla de ruedas o están en cama.",
        "Hallazgo: 3 de 3 baños no tienen barras de apoyo.",
        "Hallazgo: Hay 20 camas para 12 personas atendidas y una capacidad de 25.",
        "Notas de la institución: Perfil ficticio para pruebas.",
        "Nombre legal: Asilo Ficticio, A.C.",
        "Objeto social (lo que dicen sus estatutos): Dar asistencia a personas mayores sin familia.",
        "Edades que recibe: de 60 a 95 años.",
        "Servicios o programas: Residencia, comedor y terapia física.",
        "Criterios de ingreso: Personas mayores que no pueden vivir solas.",
        "Régimen fiscal: persona moral con fines no lucrativos.",
        "Rubro autorizado como donataria: asistencial.",
    ];

    #[test]
    fn the_sheet_carries_every_fact_of_the_profile_in_plain_spanish() {
        let (_d, c) = saved(&rich(), true);
        let ctx = profile_context(&c).unwrap();
        for fact in RICH_FACTS {
            assert!(ctx.contains(fact), "missing «{fact}» in:\n{ctx}");
        }
        assert!(ctx.starts_with("Datos de «Mi institución», confirmados por la persona."));
        assert!(!ctx.contains("No capturado"), "nothing is missing:\n{ctx}");
        for raw in ["poor", "good", "high", "total", "fees", "permanent", "elderly_home"] {
            assert!(!ctx.contains(&format!("{raw})")) && !ctx.contains(&format!(": {raw}")), "raw value «{raw}» in:\n{ctx}");
        }
    }

    /// An agent asks for one part at a time (ADR-034): every fact of the whole sheet is in exactly one part, each part
    /// says whether the data are confirmed, and what is missing goes with the part it belongs to.
    #[test]
    fn the_sheet_splits_into_parts_and_every_fact_is_in_one() {
        let (_d, c) = saved(&rich(), true);
        let parts: Vec<String> = SECTIONS.iter().map(|k| section_context(&c, k).unwrap().unwrap()).collect();
        for fact in RICH_FACTS {
            let n = parts.iter().filter(|p| p.contains(fact)).count();
            assert_eq!(n, 1, "«{fact}» is in {n} parts");
        }
        assert!(parts.iter().all(|p| p.starts_with("Datos de «Mi institución», confirmados por la persona.")));
        assert!(parts[0].contains("Notas de la institución: Perfil ficticio para pruebas."));
        assert!(parts[1].contains("Balance del año") && parts[4].contains("Casa principal"));
        assert_eq!(section_context(&c, "nope").unwrap(), None);

        let mut thin = rich();
        thin.capacity_total = None;
        let (_d, c) = saved_with(&thin, &FinanceInput::default(), false);
        let institution = section_context(&c, "institution").unwrap().unwrap();
        assert!(institution.contains("No capturado en esta parte: capacidad total."), "{institution}");
        let money = section_context(&c, "money").unwrap().unwrap();
        assert!(money.contains("No capturado en esta parte:") && money.contains("cuánto gasta al año"), "{money}");
        assert!(!money.contains("capacidad total"));
    }

    #[test]
    fn what_must_not_reach_the_ai_does_not() {
        let (_d, c) = saved(&rich(), true);
        let ctx = profile_context(&c).unwrap();
        // not even added up: payroll, its benefits, its cost, and the fees of the roster; nor the exact balance
        let stored = profile_store::load_current(&c).unwrap().unwrap();
        let t = stored.input.totals(stored.as_of_year);
        let f = crate::core::bridge::finance::finances(&c).unwrap();
        let numbers = crate::ai::figures::digit_numbers(&ctx);
        for hidden in [t.payroll_monthly_mxn, t.payroll_annual_mxn, t.payroll_benefits_annual_mxn, t.payroll_cost_annual_mxn,
                       t.fees_monthly_mxn, t.fees_annual_mxn, f.income_annual_mxn, f.expenses_annual_mxn.unwrap(), f.balance_annual_mxn.unwrap()] {
            assert!(!numbers.contains(&hidden.abs().to_string()), "«{hidden}» leaked:\n{ctx}");
        }
        assert!(!ctx.contains("Cuotas aprox."), "a fee estimate left out of the sums is not shown either:\n{ctx}");
        // pay, the fee a person pays, contact data, RFC and the representative; names and identifiers of the staff
        for secret in ["7777", "7,777", "3333", "3,333", "AFI200101", "55 5555", "Rosa Representante", "Secreta", "Oculta", "HEGG", "Reservada", "Privada", "Callada", "Discreto",
                       "Escondida", "4321", "45010", "45011", "600-04", "2015", "2030", "Presidenta Ficticia", "Tesorero", "002010", "Banamex"] {
            assert!(!ctx.contains(secret), "«{secret}» leaked:\n{ctx}");
        }
        // the age of a group of one person would be that person's age
        assert!(ctx.contains("Población: Hombres de 90 años o más — 1 persona."), "{ctx}");
        assert!(!ctx.contains("93"), "{ctx}");
    }

    /// An I.A.P. says it is registered before its Junta, never the folio; without the folio, the registry is missing.
    #[test]
    fn the_junta_is_said_without_its_folio() {
        let mut p = rich();
        p.institution.legal_form = Some("iap".into());
        let (_d, c) = saved(&p, true);
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.contains("No capturado en «Mi institución»: su registro ante la Junta de Asistencia Privada."), "{ctx}");
        p.institution.details.as_mut().unwrap().insert("institution.junta_folio".into(), serde_json::json!("JAP-FOLIO-99"));
        let (_d, c) = saved(&p, true);
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.contains("Registrada ante la Junta de Asistencia Privada de su estado."), "{ctx}");
        assert!(!ctx.contains("JAP-FOLIO") && !ctx.contains("No capturado"), "{ctx}");
    }

    #[test]
    fn a_section_with_nothing_captured_says_so_instead_of_staying_silent() {
        let input = ProfileInput { institution: InstitutionInput { name: "Casa Vacía".into(), ..Default::default() }, ..Default::default() };
        let (_d, mut c) = db();
        profile_store::save(&mut c, &input, &profile_store::fixtures()).unwrap();
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.contains("BORRADOR"), "{ctx}");
        assert!(ctx.contains("Institución: Casa Vacía (institución de asistencia)."), "{ctx}");
        for gap in ["a qué se dedica", "capacidad total", "cuánto gasta al año", "de dónde vienen sus ingresos", "a quiénes atiende", "el personal", "las instalaciones"] {
            assert!(ctx.contains(gap), "the gap «{gap}» is not named:\n{ctx}");
        }
        assert!(ctx.contains("no que sea cero"), "{ctx}");
        assert!(!ctx.contains("Total de personas atendidas"), "no invented zero totals:\n{ctx}");
    }

    #[test]
    fn with_only_the_approximate_expense_the_payroll_goes_inside_it_and_a_deficit_is_said_in_words() {
        let mut m = rich_money();
        m.expenses.clear();
        m.annual_budget_mxn = Some(900_000); // income is the roster fees plus 800,000 written: more than this
        let (_d, c) = saved_with(&rich(), &m, true);
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.contains("La nómina del personal (con aguinaldo y prima vacacional) va dentro del gasto aproximado; el monto no se comparte."), "{ctx}");
        assert!(ctx.contains("Balance del año, calculado con lo capturado y el gasto anual aproximado: los ingresos alcanzan a cubrir los egresos y sobra algo."), "{ctx}");
        assert!(!ctx.contains("Egreso:"), "{ctx}");

        let mut m = rich_money();
        m.expenses.clear();
        m.annual_budget_mxn = Some(5_000_000);
        let (_d, c) = saved_with(&rich(), &m, true);
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.contains("los ingresos NO alcanzan a cubrir los egresos: hay déficit."), "{ctx}");
    }

    #[test]
    fn without_any_profile_the_sheet_says_there_is_nothing() {
        let (_d, c) = db();
        let ctx = profile_context(&c).unwrap();
        assert!(ctx.starts_with("Perfil: sin datos."), "{ctx}");
    }

    #[test]
    fn the_sheet_adds_no_incidental_numbers_to_what_the_figure_check_trusts() {
        let (_d, c) = saved(&rich(), true);
        let ctx = profile_context(&c).unwrap();
        // every number of the sheet is a fact of the profile or a sum made by code
        // the years of operation change with the year the test runs
        let operating = (profile_store::current_year(&c).unwrap() - 1987).to_string();
        let allowed = [operating.as_str(), "1987", "25", "1800000", "10000", "120000", "680000", "800000", "15000", "180000", "36000", "216000", "11", "8", "3", "5", "66", "95", "12", "4", "1", "70", "80", "7", "9", "89", "79", "60", "69", "90", "65", "77", "2", "6", "20", "50", "600"];
        for n in crate::ai::figures::digit_numbers(&ctx) {
            assert!(allowed.contains(&n.as_str()), "unexpected number {n} in the sheet:\n{ctx}");
        }
    }

    #[test]
    fn the_padron_reaches_the_ai_only_as_counts() {
        let dir = tempfile::tempdir().unwrap();
        let mut c = open_encrypted(&dir.path().join("t.db"), KEY).unwrap();
        let input: ProfileInput = serde_json::from_str(include_str!("../../../fixtures/institucion-asilo.json")).unwrap();
        crate::core::profile::service::save_profile(&mut c, input.clone(), None).unwrap();
        crate::core::profile::sync::seed_examples(&mut c, include_str!("../../../fixtures/padron-asilo.json")).unwrap();
        crate::core::profile::service::save_profile(&mut c, input, None).unwrap();
        crate::core::bridge::facilities::seed_example(&mut c, include_str!("../../../fixtures/instalaciones-asilo.json")).unwrap();
        let money: FinanceInput = serde_json::from_str(include_str!("../../../fixtures/institucion-asilo.json")).unwrap();
        crate::modules::finance::storage::save(&mut c, &money).unwrap();

        let stored = profile_store::load_current(&c).unwrap().unwrap();
        let ctx = render(&stored, &money, &crate::modules::hr::api::ai_summary(&c).unwrap(), &people_sheet(&c).unwrap(), &facilities_sheet(&c).unwrap(), &BoardSheet::default());
        assert!(ctx.contains("Personal: "), "{ctx}");
        assert!(ctx.contains("Población: "), "{ctx}");
        assert!(ctx.contains("Total de personas atendidas: "), "{ctx}");
        assert!(ctx.contains("Piso resbaloso, sin barras de apoyo"), "the notes of the facilities reach the AI:\n{ctx}");
        assert!(ctx.contains("Gasto anual aproximado (cifra a ojo de la persona, todo incluido): $1,800,000 pesos."), "{ctx}");
        assert!(ctx.contains("Ingreso — cuotas de los beneficiarios (del padrón)"), "{ctx}");
        let t = stored.input.totals(stored.as_of_year);
        for hidden in [t.payroll_monthly_mxn, t.payroll_annual_mxn, t.payroll_cost_annual_mxn, t.fees_monthly_mxn, t.fees_annual_mxn] {
            assert!(!crate::ai::figures::digit_numbers(&ctx).contains(&hidden.to_string()), "a sum of pay or fees ({hidden}) leaked:\n{ctx}");
        }
        assert!(!ctx.contains("Esperanza Robles") && !ctx.contains("Vázquez"), "no name:\n{ctx}");
        for st in &stored.input.staff {
            if let Some(pay) = st.monthly_salary_mxn {
                assert!(!ctx.contains(&pay.to_string()), "a salary ({pay}) leaked:\n{ctx}");
            }
        }
        for g in &stored.input.population {
            if let Some(fee) = g.monthly_fee_mxn {
                assert!(!ctx.contains(&fee.to_string()), "a fee ({fee}) leaked:\n{ctx}");
            }
        }
    }
}
