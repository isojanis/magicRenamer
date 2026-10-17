use super::*;
use crate::test_support::*;

#[test]
fn scans_only_supported_files_recursively_and_reports_metadata() {
    let folder = unique_temp_dir("scan");
    let nested = folder.join("nested");
    fs::create_dir_all(&nested).unwrap();
    fs::write(folder.join("Report.TXT"), "abc").unwrap();
    fs::write(folder.join("ignored.exe"), "no").unwrap();
    fs::write(nested.join("data.JsOn"), "{}").unwrap();

    let files = scan_folder(folder.to_string_lossy().into_owned()).unwrap();
    assert_eq!(files.len(), 2);
    let text_file = files.iter().find(|file| file.name == "Report.TXT").unwrap();
    assert_eq!(text_file.extension, "txt");
    assert_eq!(text_file.size_bytes, 3);
    assert!(text_file.path.ends_with("Report.TXT"));
    assert!(files.iter().any(|file| file.extension == "json" && file.path.contains("nested")));
    assert!(scan_folder(folder.join("Report.TXT").to_string_lossy().into_owned()).is_err());
    assert!(scan_folder(folder.join("missing").to_string_lossy().into_owned()).is_err());
    remove_temp_dir(&folder);
}

#[test]
fn excerpt_strategies_keep_useful_content_and_respect_limits() {
    assert_eq!(smart_excerpt("csv", "a,b\n1,2\n3,4\n5,6"), "a,b\n1,2\n3,4");
    assert_eq!(smart_excerpt("md", "Title\nline two\nline three\n# Important\nlast"), "Title\nline two\nline three\n# Important\nlast");
    assert!(smart_excerpt("json", r#"{"vendor":"Northwind","items":[1,2]}"#).contains("vendor: \"Northwind\""));
    assert_eq!(smart_excerpt("json", "[1,2,3]"), "Root array with 3 items");
    assert_eq!(smart_excerpt("txt", &"x".repeat(1500)).chars().count(), 1400);
}

#[tokio::test]
async fn rejects_unreadable_or_unsupported_documents() {
    let folder = unique_temp_dir("extract-errors");
    let invalid = folder.join("invalid.txt");
    fs::write(&invalid, [0xff, 0xfe]).unwrap();
    assert!(extract_document_snippet(invalid.to_string_lossy().into_owned()).await.is_err());
    assert!(extract_document_snippet(folder.join("missing.txt").to_string_lossy().into_owned()).await.is_err());
    let unsupported = folder.join("image.png");
    fs::write(&unsupported, "not text").unwrap();
    assert!(extract_document_snippet(unsupported.to_string_lossy().into_owned()).await.unwrap_err().contains("Unsupported"));
    let empty = folder.join("empty.md");
    fs::write(&empty, "  ").unwrap();
    assert!(extract_document_snippet(empty.to_string_lossy().into_owned()).await.unwrap_err().contains("No readable text"));
    remove_temp_dir(&folder);
}

#[test]
fn recursively_scans_the_large_nested_fixture_batch() {
    let files = scan_folder(fixture("large-batch")).expect("fixture folder should scan");
    assert_eq!(files.len(), 522, "the batch has 504 root files and 18 nested files");
    assert!(files.iter().any(|file| file.name.ends_with(".json")));
    assert!(files.iter().any(|file| file.name.ends_with(".csv")));
    assert!(files.iter().any(|file| file.path.contains("/archive/")));
}

#[tokio::test]
async fn excerpt_preserves_a_date_found_later_in_the_document() {
    let folder = unique_temp_dir("late-date");
    let path = folder.join("download.txt");
    let content = format!(
        "Invoice\nVendor: Nordic Engineering Association\n{}\nInvoice date: 2026-09-14\nClosing note",
        "Item description\n".repeat(10)
    );
    std::fs::write(&path, content).unwrap();
    let snippet = extract_document_snippet(path.to_string_lossy().into_owned()).await.unwrap();
    std::fs::remove_dir_all(&folder).unwrap();
    assert!(snippet.starts_with("Document date: 2026-09-14"), "{}", snippet);
}

#[test]
fn scans_all_supported_fixture_formats_recursively() {
    let files = scan_folder(fixtures_dir().to_string_lossy().into_owned())
        .expect("test_docs should scan recursively");
    assert!(files.len() > 550, "expected the broad fixture corpus, got {} files", files.len());
    for extension in ["txt", "md", "csv", "json", "pdf", "docx"] {
        assert!(files.iter().any(|file| file.extension == extension), "missing .{} fixture", extension);
    }
}

#[tokio::test]
async fn extracts_snippets_from_real_fixture_files() {
    let examples = [
        ("finance/invoices/2026-10-05_Redwood_Invoice.json", "Root keys:"),
        ("finance/invoices/2026-09-02_Harbor_Invoice_772.csv", "invoice,date,vendor,total,currency"),
        ("engineering/meeting-notes/2026-08-05_platform_sync.md", "Platform Sync"),
        ("download_1.txt", "INV-2026-0812"),
        ("document-formats/2026-08-16_Lab_Results.pdf", "Laboratory Results"),
        ("document-formats/2026-09-06_Project_Brief.docx", "Project Brief: Offline Archive"),
    ];

    for (relative_path, expected_text) in examples {
        let snippet = extract_document_snippet(fixture(relative_path))
            .await
            .unwrap_or_else(|error| panic!("failed to extract {}: {}", relative_path, error));
        assert!(snippet.contains(expected_text), "{} did not contain {:?}: {}", relative_path, expected_text, snippet);
    }
}
