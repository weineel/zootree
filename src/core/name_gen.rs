use rand::seq::SliceRandom;

const ADJECTIVES: &[&str] = &[
    "bold", "brave", "calm", "cool", "dark", "deep", "fair", "fast", "free", "glad", "gold",
    "good", "keen", "kind", "late", "lean", "live", "long", "loud", "mild", "neat", "nice", "open",
    "pale", "pure", "rare", "rich", "safe", "slim", "soft", "sure", "tall", "thin", "true", "warm",
    "wide", "wild", "wise", "young", "keen",
];

const NOUNS: &[&str] = &[
    "arch", "bark", "beam", "bird", "bolt", "cave", "clay", "cove", "dawn", "deer", "dove", "dune",
    "dust", "fern", "fire", "fish", "ford", "fox", "gate", "glen", "glow", "hawk", "hill", "jade",
    "lake", "leaf", "lion", "lynx", "mist", "moon", "moss", "oak", "owl", "palm", "peak", "pine",
    "pond", "rain", "reed", "reef", "ridge", "river", "rock", "rose", "sage", "sand", "seal",
    "snow", "star", "stone", "swan", "tide", "tree", "vale", "vine", "wave", "wind", "wolf",
    "wood", "wren",
];

pub struct NameGenerator;

impl NameGenerator {
    pub fn new() -> Self {
        Self
    }

    pub fn generate(&self) -> String {
        let mut rng = rand::thread_rng();
        let adj = ADJECTIVES.choose(&mut rng).unwrap();
        let noun = NOUNS.choose(&mut rng).unwrap();
        format!("{}-{}", adj, noun)
    }

    pub fn generate_avoiding(&self, existing: &[String]) -> String {
        for _ in 0..100 {
            let name = self.generate();
            if !existing.contains(&name) {
                return name;
            }
        }
        let name = self.generate();
        format!("{}-{}", name, rand::random::<u16>() % 1000)
    }
}
