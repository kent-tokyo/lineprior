use lineprior::{
    BuildConfig, PRIOR_BOOK_SCHEMA_VERSION, PriorBook, PriorBookMetadata,
    load_prior_book_with_metadata, save_prior_book_with_config,
};

#[test]
fn public_metadata_api_exposes_version_producer_and_config() {
    let book = PriorBook::default();
    let config = BuildConfig::default();
    let mut jsonl = Vec::new();
    save_prior_book_with_config(&book, &config, &mut jsonl).unwrap();

    let loaded = load_prior_book_with_metadata(jsonl.as_slice()).unwrap();
    assert!(loaded.book.entries.is_empty());
    match loaded.metadata {
        Some(PriorBookMetadata::Versioned(metadata)) => {
            assert_eq!(metadata.schema_version, PRIOR_BOOK_SCHEMA_VERSION);
            assert_eq!(metadata.producer_version, env!("CARGO_PKG_VERSION"));
            assert_eq!(metadata.build_config, serde_json::to_value(config).unwrap());
        }
        Some(_) => panic!("expected versioned metadata"),
        None => panic!("expected metadata"),
    }
}
