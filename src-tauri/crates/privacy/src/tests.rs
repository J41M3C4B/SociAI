use super::*;
use std::path::Path;

fn scanner() -> RegexScanner {
    RegexScanner::new(ScannerConfig::default())
}

fn fixture(name: &str) -> String {
    std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../fixtures/escaner").join(name))
        .unwrap()
}

fn cases(name: &str) -> Vec<String> {
    fixture(name)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect()
}

fn kinds(report: &ScanReport) -> Vec<&'static str> {
    report.findings.iter().map(|f| f.kind.key()).collect()
}

#[test]
fn every_positive_case_is_detected_with_its_kind() {
    let s = scanner();
    let lines = cases("positivos.txt");
    assert!(lines.len() >= 20);
    let mut missed = Vec::new();
    for line in &lines {
        let (kind, text) = line.split_once(" | ").expect("kind | text");
        let r = s.scan(text);
        if !kinds(&r).contains(&kind) {
            missed.push(format!("{kind}: {text} -> {:?}", kinds(&r)));
        }
    }
    assert!(missed.is_empty(), "not detected:\n{}", missed.join("\n"));
}

#[test]
fn no_negative_case_is_detected() {
    let s = scanner();
    let mut wrong = Vec::new();
    for text in cases("negativos.txt") {
        let r = s.scan(&text);
        if !r.is_clean() {
            wrong.push(format!("{text} -> {:?}", kinds(&r)));
        }
    }
    assert!(wrong.is_empty(), "false positives:\n{}", wrong.join("\n"));
}

#[test]
fn redact_never_leaves_the_original_value() {
    let s = scanner();
    let secrets = [
        "LOPM800101MDFRZN09",
        "012180001234567899",
        "4111 1111 1111 1111",
        "55 1234 5678",
        "maria.lopez@example.com",
        "Rosa Hernández López",
        "LPMRMN80010109H400",
        "42859012348",
    ];
    let text = secrets.iter().map(|x| format!("Dato: {x}. ")).collect::<String>();
    let r = s.scan(&text);
    let out = s.redact(&text, &r);
    for x in secrets {
        assert!(!out.contains(x), "{x} survived in: {out}");
    }
    assert!(out.contains("[CURP OCULTA]"));
    assert!(out.contains("[NOMBRE OCULTO]"));
    // and a second scan of the redacted text is clean
    assert!(s.scan(&out).is_clean(), "{:?}", kinds(&s.scan(&out)));
}

#[test]
fn redact_keeps_the_rest_of_the_text_and_accents() {
    let s = scanner();
    let text = "Atención: la CURP LOPM800101MDFRZN09 es de ella. ¡Gracias!";
    let out = s.redact(text, &s.scan(text));
    assert_eq!(out, "Atención: la CURP [CURP OCULTA] es de ella. ¡Gracias!");
}

#[test]
fn spans_point_to_the_original_text_with_accents() {
    let s = scanner();
    let text = "Ñandú — teléfono: (33) 3456-7890 fin";
    let r = s.scan(text);
    let f = r.findings.iter().find(|f| f.kind == FindingKind::Phone).unwrap();
    assert_eq!(&text[f.span.clone()], "(33) 3456-7890");
}

/// ADR-041: what the person said is not personal in an errand is not covered again before the AI; only that text,
/// and only a warning. A CURP stays blocked even if it was listed.
#[test]
fn what_the_person_cleared_is_not_flagged_again() {
    let text = "Nuestro RFC es CSE870512AB3 y el teléfono 33 1234 5678.";
    assert_eq!(kinds(&scanner().scan(text)), vec!["rfc_company", "phone"]);
    let s = RegexScanner::new(ScannerConfig { cleared: vec!["CSE870512AB3".into(), "33 1234 5678".into(), "LOPM800101MDFRZN09".into()], ..Default::default() });
    assert!(s.scan(text).is_clean(), "{:?}", kinds(&s.scan(text)));
    // another phone is still a phone; a CURP is never cleared
    assert_eq!(kinds(&s.scan("Y el 55 9999 0000.")), vec!["phone"]);
    assert_eq!(kinds(&s.scan("La CURP LOPM800101MDFRZN09.")), vec!["curp"]);
}

#[test]
fn institutional_phone_and_email_are_not_flagged() {
    let s = RegexScanner::new(ScannerConfig {
        institutional_phones: vec!["+52 55 1234 5678".into()],
        institutional_emails: vec!["Contacto@CasaHogar.org".into()],
        ..Default::default()
    });
    let r = s.scan("Llamar al 55 1234 5678 o escribir a contacto@casahogar.org");
    assert!(r.is_clean(), "{:?}", kinds(&r));
    let r = s.scan("Otro teléfono 55 9999 0000 y otro@correo.com");
    assert_eq!(kinds(&r), vec!["phone", "email"]);
}

/// OCR (ADR-035): in a text read from a picture, an email whose «@» was misread is still found when its line speaks
/// of mail or a sign stands in place of the «@»; a web address, the email of the institution and a typed text are not.
#[test]
fn an_email_with_its_at_sign_misread_is_found_in_a_text_read_from_a_picture() {
    let cfg = ScannerConfig { institutional_emails: vec!["contacto@casaficticia.org".into()], ..Default::default() };
    let pictures = RegexScanner::new(cfg.clone()).reading_pictures();
    for misread in ["Email: infoGorlandojuan.es", "Correo electrónico: ventas.norteGempresa.com.mx", "Escriba a maria(lopez.mx o pase"] {
        let r = pictures.scan(misread);
        assert_eq!(kinds(&r), vec!["email"], "{misread}");
        assert!(!pictures.redact(misread, &r).contains("juan.es") && !pictures.redact(misread, &r).contains("empresa.com"), "{misread}");
    }
    for not_an_email in [
        "Visite www.fundacionficticia.org.mx para más datos",
        "Página: https://convocatoria.example.org",
        "La fundación publica sus bases en fundacionficticia.org",
        "Email: contactoGcasaficticia.org",
        "Email: info@orlandojuan.es",
    ] {
        let r = pictures.scan(not_an_email);
        assert!(r.findings.iter().all(|f| f.confidence == 1.0), "{not_an_email}: {:?}", kinds(&r));
    }
    // a text typed by a person is not read from a picture: only true emails count there
    assert!(RegexScanner::new(cfg).scan("Email: infoGorlandojuan.es").is_clean());
}

/// OCR (ADR-035): in a text read from a picture, a CURP or an RFC of a person with a letter where a digit goes («O» for
/// «0», «S» for «5») is still covered, when fixed it is one: the check digit of the CURP, a real date in the RFC, and
/// four true digits of the six of its date. Words with those letters are not; a typed text is not read this way.
#[test]
fn a_curp_or_an_rfc_with_a_letter_for_a_digit_is_found_in_a_text_read_from_a_picture() {
    let pictures = RegexScanner::new(ScannerConfig::default()).reading_pictures();
    for (misread, kind) in [
        ("CURP: LOPM8001O1MDFRZN09", "curp"),
        ("Su CURP es RAMC92121SMNLNRL0O.", "curp"),
        ("RFC: LOPM8001O1AB3", "rfc_person"),
    ] {
        let r = pictures.scan(misread);
        assert_eq!(kinds(&r), vec![kind], "{misread}");
        assert!(r.has_blocking(), "{misread}");
        assert!(!pictures.redact(misread, &r).contains("LOPM") && !pictures.redact(misread, &r).contains("RAMC"), "{misread}");
    }
    for not_one in [
        // fixed, its check digit is wrong
        "CURP: LOPM8001O1MDFRZN08",
        // words whose letters could be digits: too few true digits in the date
        "COSTOSBOLSOS y LIBROSOBRESALIENTES",
        // fixed, its date does not exist
        "RFC: LOPM8O13S1AB3",
    ] {
        assert!(!pictures.scan(not_one).has_blocking(), "{not_one}: {:?}", kinds(&pictures.scan(not_one)));
    }
    // a text typed by a person is not read from a picture
    assert!(!RegexScanner::new(ScannerConfig::default()).scan("CURP: LOPM8001O1MDFRZN09").has_blocking());
}

/// ADR-033 §3: a CLABE of the institution is its own; any other CLABE, even one digit away, is still blocked.
#[test]
fn only_exactly_an_account_of_the_institution_is_let_through() {
    let own = RegexScanner::new(ScannerConfig { institutional_clabes: vec!["002 010 07777777777 1".into()], ..Default::default() });
    assert!(own.scan("Donativos a la CLABE 002010077777777771, a nombre de la casa.").is_clean());
    assert!(own.scan("CLABE: 002 010 07777777777 1").is_clean(), "written in groups too");
    // a person's account, valid and different: blocked as before
    let r = own.scan("La CLABE de la cocinera es 012180001234567899.");
    assert_eq!(kinds(&r), vec!["clabe"]);
    assert!(r.has_blocking());
    // without the accounts of the institution, the same text is blocked
    assert_eq!(kinds(&scanner().scan("Donativos a la CLABE 002010077777777771.")), vec!["clabe"]);
}

#[test]
fn safe_phrases_protect_institution_names() {
    let text = "La directora Rosa Hernández López coordina el hogar.";
    assert!(!scanner().scan(text).is_clean());
    let s = RegexScanner::new(ScannerConfig {
        safe_phrases: vec!["Rosa Hernández López".into()],
        ..Default::default()
    });
    assert!(s.scan(text).is_clean());
}

#[test]
fn block_findings_are_block_and_names_are_warn() {
    let r = scanner().scan("CURP LOPM800101MDFRZN09 de Rosa Hernández López");
    let sev = |k| r.findings.iter().find(|f| f.kind == k).unwrap().severity;
    assert_eq!(sev(FindingKind::Curp), Severity::Block);
    assert_eq!(sev(FindingKind::PersonName), Severity::Warn);
    assert!(r.has_blocking());
}

#[test]
fn wrong_check_digit_is_only_a_warning() {
    let r = scanner().scan("LOPM800101MDFRZN08");
    assert_eq!(kinds(&r), vec!["curp"]);
    assert_eq!(r.findings[0].severity, Severity::Warn);
}

#[test]
fn counts_never_contain_the_value() {
    let s = scanner();
    let r = s.scan("CURP LOPM800101MDFRZN09 y tel 55 1234 5678");
    let json = serde_json::to_string(&r).unwrap() + &format!("{:?}", r.counts());
    assert!(!json.contains("LOPM"));
    assert!(!json.contains("1234"));
    assert_eq!(r.counts().get("curp"), Some(&1));
}

#[test]
fn roster_detection() {
    let s = scanner();
    let table = "Nombre\tEdad\tDiagnóstico\nAna\t80\tDiabetes";
    assert!(s.looks_like_roster(table, &s.scan(table)));
    let many: String = (0..6).map(|_| "LOPM800101MDFRZN09\n").collect();
    assert!(s.looks_like_roster(&many, &s.scan(&many)));
    let ok = "Atendemos a 18 personas y 6 usan andadera.";
    assert!(!s.looks_like_roster(ok, &s.scan(ok)));
}

#[test]
fn aggregated_health_phrases_are_valid() {
    let r = scanner().scan("12 de los 18 residentes viven con diabetes. Los niños reciben tratamiento dental.");
    assert!(r.is_clean(), "{:?}", kinds(&r));
}

#[test]
fn hundred_pages_scan_fast() {
    let page = "La casa hogar atiende a 18 adultos mayores. Se requieren 4 regaderas y 6 barras de apoyo. \
                El presupuesto total es de 150000 pesos. Rosa Hernández López llamó al 55 1234 5678.\n"
        .repeat(30);
    let doc = page.repeat(100);
    let s = scanner();
    let t = std::time::Instant::now();
    let r = s.scan(&doc);
    let el = t.elapsed();
    println!("scanned {} KB in {:?}, {} findings", doc.len() / 1024, el, r.findings.len());
    // 1 s in release; debug builds are much slower
    let limit = if cfg!(debug_assertions) { 15 } else { 1 };
    assert!(el.as_secs() < limit, "too slow: {el:?}");
}
