use super::*;

fn temp_path(label: &str) -> PathBuf {
    std::env::temp_dir().join(format!(
        "va-preview-cleanup-{label}-{}-{}.json",
        std::process::id(),
        uuid::Uuid::new_v4()
    ))
}

#[test]
fn cleanup_ports_round_trip_sorted_and_unique() {
    let path = temp_path("ports");
    persist_at(&path, &[5173, 3000, 5173]).unwrap();

    assert_eq!(read_at(&path).unwrap(), vec![3000, 5173]);

    remove_at(&path).unwrap();
}

#[test]
fn empty_ports_remove_the_journal() {
    let path = temp_path("empty");
    std::fs::write(&path, "stale").unwrap();

    persist_at(&path, &[]).unwrap();

    assert!(!path.exists());
}
