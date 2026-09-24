use super::*;

#[test]
fn document_rows_carry_provider_metadata() {
    let rows = "content://docs/1\treport.pdf\tapplication/pdf\t2048\t1700000000000\n\
                content://docs/2\t\t\t\t";
    let documents = parse_documents(rows);
    assert_eq!(documents.len(), 2);
    assert_eq!(documents[0].name, "report.pdf");
    assert_eq!(documents[0].mime_type.as_deref(), Some("application/pdf"));
    assert_eq!(documents[0].len, Some(2048));
    assert_eq!(documents[0].modified_millis, Some(1_700_000_000_000));
    assert_eq!(documents[1].name, "2");
    assert_eq!(documents[1].len, None);
}

#[test]
fn directory_rows_are_told_apart_from_files() {
    let directory =
        parse_document("content://docs/tree\tMusic\tvnd.android.document/directory\t\t")
            .expect("a row parses");
    assert!(is_directory(&directory));
    let file = parse_document("content://docs/3\ta.mp3\taudio/mpeg\t10\t").expect("a row parses");
    assert!(!is_directory(&file));
}

#[test]
fn blank_rows_are_skipped() {
    assert!(parse_documents("\n\t\n").is_empty());
}
