use zootree::core::name_gen::NameGenerator;

#[test]
fn test_generate_name_format() {
    let gen = NameGenerator::new();
    let name = gen.generate();
    let parts: Vec<&str> = name.split('-').collect();
    assert_eq!(parts.len(), 2, "name should be adjective-noun: {}", name);
    assert!(parts[0].chars().all(|c| c.is_ascii_lowercase()));
    assert!(parts[1].chars().all(|c| c.is_ascii_lowercase()));
}

#[test]
fn test_generate_unique_names() {
    let gen = NameGenerator::new();
    let names: Vec<String> = (0..20).map(|_| gen.generate()).collect();
    let unique: std::collections::HashSet<&String> = names.iter().collect();
    assert!(unique.len() > 1, "should generate different names");
}

#[test]
fn test_generate_with_existing_avoids_collision() {
    let gen = NameGenerator::new();
    let first = gen.generate();
    let existing = vec![first.clone()];
    let second = gen.generate_avoiding(&existing);
    assert_ne!(first, second);
}
