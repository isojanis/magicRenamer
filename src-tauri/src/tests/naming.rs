use super::*;
use crate::test_support::*;

#[test]
fn handles_date_boundaries_and_ambiguous_content() {
    let valid_dates = [
        ("Date: 2024-02-29", Some("2024-02-29")),
        ("Date: 2025-02-29", None),
        ("Date: 2026-13-01", None),
        ("Date: 14.09.2026", Some("2026-09-14")),
        ("Date: September 14, 2026", Some("2026-09-14")),
        ("Date: 14. syyskuuta 2026", Some("2026-09-14")),
        ("Date: 2026-09-14 and Date: 2026-09-15", None),
    ];
    for (text, expected) in valid_dates {
        assert_eq!(find_document_date(text, "download.pdf").as_deref(), expected, "{}", text);
    }
}

#[test]
fn parses_model_json_and_rejects_unsafe_filename_candidates() {
    let parsed = parse_model_suggestion(r#"{"new_name":"Northwind_Report.pdf"}"#).unwrap();
    assert_eq!(parsed.category, "");
    assert_eq!(parsed.reason, "");
    assert_eq!(parse_model_suggestion("```json\n{\"new_name\":\"Northwind_Report.pdf\"}\n```").unwrap().new_name, "Northwind_Report.pdf");
    assert!(parse_model_suggestion("{}").is_err());
    assert!(parse_model_suggestion("{\"category\":\"Report\"}").is_err());
    assert_eq!(parse_model_suggestion("Northwind_Report.pdf").unwrap().new_name, "Northwind_Report.pdf");
    assert!(parse_model_suggestion(" ").is_err());

    let suggestion = |name: &str| FilenameSuggestion {
        new_name: name.to_string(),
        category: "Report".to_string(),
        reason: "Source text".to_string(),
    };
    for unsafe_name in ["", "../outside.pdf", "folder\\outside.pdf", ".."] {
        assert!(finalize_suggestion(suggestion(unsafe_name), "pdf", None, None).is_err(), "{}", unsafe_name);
    }
    assert!(finalize_suggestion(suggestion("2026-09-14_Northwind_Report.pdf"), "pdf", None, None).is_err());
    let normalized = finalize_suggestion(suggestion("Northwind.Report.PDF"), "pdf", None, None).unwrap();
    assert_eq!(normalized.new_name, "Northwind_Report.pdf");
    assert_eq!(normalized.category, "Report");
    assert_eq!(normalized.reason, "Source text");
    let retained_extension = finalize_suggestion(suggestion("Northwind_Report.docx"), "pdf", None, None).unwrap();
    assert_eq!(retained_extension.new_name, "Northwind_Report.pdf");
    let reference = finalize_suggestion(suggestion("10024665.pdf"), "pdf", None, None).unwrap();
    assert_eq!(reference.new_name, "10024665.pdf");
}

#[test]
fn mixed_folder_context_selects_only_the_same_document_kind_and_subject() {
    let folder = unique_temp_dir("mixed-folder");
    for name in [
        "download (20).pdf",
        "2026-04-10_Nordic_Engineering_Association_Lasku.pdf",
        "2025-11-12_Nordic_Engineering_Association_Invoice.pdf",
        "2024-02-13_Nordic_Engineering_Association_Lasku.pdf",
        "2026-03-12_Nordic_Engineering_Association_Report.pdf",
        "2026-07-01_Alpine_Office_Lasku.pdf",
    ] {
        std::fs::write(folder.join(name), []).expect("fixture file should be created");
    }
    let blueprint = build_directory_blueprint(folder.to_string_lossy().into_owned()).unwrap();
    let siblings = relevant_sibling_names(
        &folder.join("download (20).pdf"),
        "Invoice\nVendor: Nordic Engineering Association\nInvoice date: 2026-09-14",
    );
    let unrelated = relevant_sibling_names(
        &folder.join("download (20).pdf"),
        "Contract\nCustomer: Alpine Office\nSigned: 2026-09-14",
    );
    std::fs::remove_dir_all(&folder).expect("temporary folder should be removed");

    assert!(blueprint.contains("may be unrelated"));
    assert_eq!(siblings.len(), 3);
    assert_eq!(dominant_invoice_label(&siblings), Some("Lasku"));
    assert!(siblings.iter().all(|name| name.contains("Nordic_Engineering_Association")));
    assert!(siblings.iter().all(|name| name.contains("Lasku") || name.contains("Invoice")));
    assert!(unrelated.is_empty());
}

#[test]
fn identifies_issue_date_without_using_invoice_due_date() {
    assert_eq!(
        find_document_date(
            "Laskun päiväys: 14.09.2026\nEräpäivä: 29.09.2026",
            "download (20).pdf"
        ),
        Some("2026-09-14".to_string())
    );
    assert_eq!(
        find_document_date("Date: August 12, 2026", "download_1.txt"),
        Some("2026-08-12".to_string())
    );
    assert_eq!(
        find_document_date("Laskun päiväys: 14. syyskuuta 2026", "download.pdf"),
        Some("2026-09-14".to_string())
    );
    assert_eq!(
        find_document_date("No date in contents", "2026-03-12_invoice.pdf"),
        Some("2026-03-12".to_string())
    );
}

#[test]
fn moves_confirmed_date_to_front_and_rejects_generic_names() {
    let suggestion = |name: &str| FilenameSuggestion {
        new_name: name.to_string(),
        category: "Invoice".to_string(),
        reason: String::new(),
    };
    let dated = finalize_suggestion(
        suggestion("Tekniikan-Akateemiset-ry-LASKU-2026-09-14.pdf"),
        "pdf",
        Some("2026-09-14"),
        None,
    ).expect("dated invoice should be normalized");
    assert_eq!(
        dated.new_name,
        "2026-09-14_Tekniikan_Akateemiset_Ry_Lasku.pdf"
    );
    let consistent = finalize_suggestion(
        suggestion("Invoice_Nordic_Engineering_Association.pdf"),
        "pdf",
        Some("2026-09-14"),
        Some("Lasku"),
    ).expect("matching invoices should use the same type label");
    assert_eq!(consistent.new_name, "2026-09-14_Nordic_Engineering_Association_Lasku.pdf");
    let finnish = finalize_suggestion(
        suggestion("2025-05-01_Jäsenlasku_TEK.pdf"), "pdf", Some("2025-05-01"), None
    ).unwrap();
    assert_eq!(finnish.new_name, "2025-05-01_Jasenlasku_TEK.pdf");
    assert!(finalize_suggestion(suggestion("download (20).pdf"), "pdf", None, None).is_err());
    assert!(finalize_suggestion(suggestion("2026-10-12-Invoice.pdf"), "pdf", Some("2026-10-12"), None).is_err());
    assert!(finalize_suggestion(suggestion("Nordic_Association_document.pdf"), "pdf", None, None).is_err());
}
