use zootree::core::copy_files::merge_copy_files;

#[test]
fn test_merge_copy_files_combines() {
    let global = vec![".env".to_string()];
    let repo = vec![".env.local".to_string(), ".vscode/settings.json".to_string()];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local", ".vscode/settings.json"]);
}

#[test]
fn test_merge_copy_files_dedup() {
    let global = vec![".env".to_string()];
    let repo = vec![".env".to_string(), ".env.local".to_string()];
    let merged = merge_copy_files(&global, &repo);
    assert_eq!(merged, vec![".env", ".env.local"]);
}

#[test]
fn test_merge_copy_files_empty() {
    let global: Vec<String> = vec![];
    let repo: Vec<String> = vec![];
    let merged = merge_copy_files(&global, &repo);
    assert!(merged.is_empty());
}
