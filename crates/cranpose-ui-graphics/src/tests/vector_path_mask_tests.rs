use super::*;

#[test]
fn vector_mask_cache_accounts_for_stored_bytes_on_insert_and_eviction() {
    let mut cache = VectorPathMaskCache::new();
    for key in 0..128 {
        let image = if key % 2 == 0 {
            ImageBitmap::from_alpha8(2, 2, [31, 116, 201], vec![127; 4])
        } else {
            ImageBitmap::from_rgba8(2, 2, vec![127; 16])
        }
        .expect("cache image");
        cache.put(key, image);
        assert_eq!(
            cache.bytes,
            cache
                .entries
                .iter()
                .map(|(_, image)| image.pixels().len())
                .sum::<usize>()
        );
    }
    assert_eq!(cache.entries.len(), VECTOR_PATH_MASK_CACHE_ENTRIES);
    assert_eq!(cache.bytes, 48 * (4 + 16));
    assert!(cache.get(0).is_none());
    assert!(cache.get(127).is_some());
}
