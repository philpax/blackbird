pub fn string_to_colour(s: &str) -> egui::ecolor::Hsva {
    use std::hash::Hash;
    use std::hash::Hasher;

    const DISTINCT_COLOURS: u64 = 36_000;

    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    let hash = hasher.finish();
    let hue = (hash % DISTINCT_COLOURS) as f32 / DISTINCT_COLOURS as f32;

    egui::ecolor::Hsva::new(hue, 0.75, 0.75, 1.0)
}
