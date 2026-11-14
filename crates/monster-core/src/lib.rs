//! The rules the Flexspeak app and the Monster collection server must agree on: the flavours,
//! the rarities and their odds, how a can's card is rolled, how collections are ranked, and
//! the shapes of everything the two say to each other (`api`).

pub mod api;

use std::collections::HashSet;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Debug, PartialEq, Eq)]
pub struct Flavour {
    /// What the database stores; never changes once shipped.
    pub id: &'static str,
    pub name: &'static str,
    pub line: &'static str,
    /// Dark base, main colour, highlight, second colour (sRGB).
    pub palette: [u32; 4],
}

const fn flavour(
    id: &'static str,
    name: &'static str,
    line: &'static str,
    palette: [u32; 4],
) -> Flavour {
    Flavour {
        id,
        name,
        line,
        palette,
    }
}

#[rustfmt::skip]
pub const FLAVOURS: &[Flavour] = &[
    flavour("original", "Original", "Classic", [0x0b120a, 0x7ddc1f, 0xd4ff5c, 0x2b7a0b]),
    flavour("lo-carb", "Lo-Carb", "Classic", [0x0a1430, 0x2e7bff, 0x8fd0ff, 0x10408c]),
    flavour("nitro", "Nitro Super Dry", "Classic", [0x0a1e33, 0x6cc8f5, 0xf2fbff, 0x2c6fa8]),
    flavour("the-doctor", "The Doctor", "Classic", [0x1a2600, 0xc9f02a, 0xfff46b, 0x6b9e00]),
    flavour("zero-sugar", "Zero Sugar", "Classic", [0x0b0b0b, 0x8fe63a, 0xe2ffc0, 0x3a3a3a]),
    flavour("absolutely-zero", "Absolutely Zero", "Classic", [0x0c0f12, 0x9aa6b2, 0xf2f6fa, 0x4dd13a]),
    flavour("assault", "Assault", "Classic", [0x1b1f12, 0x6f7a4a, 0xd8dcc0, 0xc8102e]),
    flavour("nitro-cosmic-peach", "Nitro Cosmic Peach", "Classic", [0x1a0e2e, 0xff9a6a, 0xffe0cc, 0x6a3cc8]),
    flavour("lando-norris", "Lando Norris Zero Sugar", "Classic", [0x2a2f12, 0xd4ff3a, 0xf6ffd0, 0x1e5cff]),
    flavour("lewis-hamilton", "Lewis Hamilton Zero Sugar", "Classic", [0x0e0e10, 0xb26bff, 0xeadcff, 0x3a3a46]),
    flavour("mule", "Mule Ginger Brew", "Classic", [0x2a1a0a, 0xc89a4a, 0xf6e6c4, 0x6b4a1e]),
    flavour("ultra-white", "Ultra White", "Ultra", [0x39465a, 0xcfdae6, 0xffffff, 0x7f93aa]),
    flavour("ultra-black", "Ultra Black", "Ultra", [0x08080b, 0x3d3d4a, 0xc9c9d6, 0xa3122a]),
    flavour("ultra-gold", "Ultra Gold", "Ultra", [0x2e1f04, 0xeeb92e, 0xffe9a3, 0xa8740f]),
    flavour("ultra-violet", "Ultra Violet", "Ultra", [0x1a0844, 0x8b4dff, 0xd9c2ff, 0x4a1fb8]),
    flavour("ultra-red", "Ultra Red", "Ultra", [0x32050e, 0xff2d46, 0xffa3ad, 0x9c0f22]),
    flavour("ultra-blue", "Ultra Blue", "Ultra", [0x061a44, 0x2f8cff, 0xa8dcff, 0x1553b8]),
    flavour("ultra-watermelon", "Ultra Watermelon", "Ultra", [0x0f3d23, 0xff4d6d, 0xffc7d1, 0x35c978]),
    flavour("ultra-paradise", "Ultra Paradise", "Ultra", [0x053b2c, 0x27e0a0, 0xbdffe2, 0x0b8f66]),
    flavour("ultra-fiesta", "Ultra Fiesta Mango", "Ultra", [0x481400, 0xff8a1f, 0xffd98a, 0xff3d77]),
    flavour("ultra-peachy-keen", "Ultra Peachy Keen", "Ultra", [0x4e1c0d, 0xff9472, 0xffe3d1, 0xf0603a]),
    flavour("ultra-rosa", "Ultra Rosá", "Ultra", [0x4a142b, 0xff6fac, 0xffd9ea, 0xd42a73]),
    flavour("ultra-sunrise", "Ultra Sunrise", "Ultra", [0x521200, 0xff5a1f, 0xffe08a, 0xffa12e]),
    flavour("ultra-strawberry", "Ultra Strawberry Dreams", "Ultra", [0x4a0e2a, 0xff5c8a, 0xffc9d9, 0xb81f55]),
    flavour("ultra-fantasy-ruby-red", "Ultra Fantasy Ruby Red", "Ultra", [0x3a0612, 0xe8203c, 0xffb3c0, 0x8c0a22]),
    flavour("ultra-vice-guava", "Ultra Vice Guava", "Ultra", [0x2e0b3a, 0xff5fa8, 0xffd3e8, 0x2ed1c9]),
    flavour("ultra-blue-hawaiian", "Ultra Blue Hawaiian", "Ultra", [0x05264a, 0x1fb6ff, 0xc8f0ff, 0xffd23a]),
    flavour("mango-loco", "Mango Loco", "Juice", [0x0b3a56, 0xffb01f, 0xffe7a0, 0x2ed1c9]),
    flavour("pipeline-punch", "Pipeline Punch", "Juice", [0x540e37, 0xff6fa3, 0xffd0a0, 0xd81f6b]),
    flavour("pacific-punch", "Pacific Punch", "Juice", [0x0f2a6a, 0xff4f64, 0xffc78a, 0x3fd6ff]),
    flavour("khaotic", "Khaotic", "Juice", [0x362000, 0xff9d1f, 0xffe96e, 0xff5e00]),
    flavour("aussie-lemonade", "Aussie Lemonade", "Juice", [0x37380a, 0xefdc2e, 0xfffbc9, 0x8bd12e]),
    flavour("monarch", "Monarch", "Juice", [0x3a1a05, 0xff9f2e, 0xffe2b0, 0xc8102e]),
    flavour("bad-apple", "Bad Apple", "Juice", [0x13300a, 0x8fd12e, 0xe8ffc0, 0xd8202e]),
    flavour("rio-punch", "Rio Punch", "Juice", [0x3a0a2a, 0xff3d8a, 0xffd0e4, 0x2ed16a]),
    flavour("reserve-pineapple", "Reserve White Pineapple", "Reserve", [0x3a300e, 0xe8d68a, 0xfffae0, 0xb99a2e]),
    flavour("reserve-watermelon", "Reserve Watermelon", "Reserve", [0x123b24, 0xf0556f, 0xffd5dc, 0x2a9c5e]),
    flavour("reserve-orange-dreamsicle", "Reserve Orange Dreamsicle", "Reserve", [0x3a1c06, 0xff9a3a, 0xfff0dc, 0xd86a1a]),
    flavour("reserve-kiwi-strawberry", "Reserve Kiwi Strawberry", "Reserve", [0x123a14, 0x8fd13a, 0xffd0d8, 0xe8304a]),
    flavour("rehab-peach", "Rehab Peach Tea", "Rehab", [0x4a2410, 0xf2a06a, 0xffe6cf, 0xb8622c]),
    flavour("rehab-lemonade", "Rehab Tea + Lemonade", "Rehab", [0x1c1904, 0xf7dc12, 0xfff7b8, 0x15140c]),
    flavour("rehab-wild-berry", "Rehab Wild Berry Tea", "Rehab", [0x2a0a2e, 0xb04ad8, 0xf0d4ff, 0x6a1a8c]),
    flavour("java-mean-bean", "Java Mean Bean", "Java", [0x1c1007, 0xa06a3c, 0xeed2b0, 0x5a3518]),
    flavour("java-salted-caramel", "Java Salted Caramel", "Java", [0x24140a, 0xc8864a, 0xf6e0c4, 0x6a3c18]),
    flavour("other", "Another Flavour", "Other", [0x15171c, 0x8a93a3, 0xeef1f6, 0x3a3f4b]),
];

pub fn flavour_by_id(id: &str) -> Option<(usize, &'static Flavour)> {
    FLAVOURS.iter().enumerate().find(|(_, f)| f.id == id)
}

/// The flavour a saved can names, or the catch-all if a later build dropped it.
pub fn flavour_or_other(id: &str) -> &'static Flavour {
    flavour_by_id(id).map_or(&FLAVOURS[FLAVOURS.len() - 1], |(_, f)| f)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Mythic,
    Legendary,
}

impl Rarity {
    pub const ALL: [Self; 5] = [
        Self::Common,
        Self::Uncommon,
        Self::Rare,
        Self::Mythic,
        Self::Legendary,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Mythic => "Mythic",
            Self::Legendary => "Legendary",
        }
    }

    /// Chance in a thousand; together they make a thousand.
    pub fn permille(self) -> u32 {
        match self {
            Self::Common => 600,
            Self::Uncommon => 250,
            Self::Rare => 100,
            Self::Mythic => 40,
            Self::Legendary => 10,
        }
    }

    /// The rarity's badge and tally colour (sRGB).
    pub fn color(self) -> u32 {
        match self {
            Self::Common => 0xa3a9b3,
            Self::Uncommon => 0x57d38c,
            Self::Rare => 0x4da3ff,
            Self::Mythic => 0xc08bff,
            Self::Legendary => 0xffb02e,
        }
    }

    /// A roll from 0 to 999, up the table from Common.
    pub fn from_roll(roll: u32) -> Self {
        let mut ceiling = 0;
        for rarity in Self::ALL {
            ceiling += rarity.permille();
            if roll < ceiling {
                return rarity;
            }
        }
        Self::Legendary
    }

    pub fn index(self) -> usize {
        self as usize
    }
}

/// A rarity, and the seed that varies the card's art, from the first bytes of a hash.
pub fn from_hash(hash: &[u8]) -> (Rarity, u16) {
    let dice = u64::from_be_bytes(hash[..8].try_into().unwrap_or([0; 8]));
    let seed = u16::from_be_bytes(hash[8..10].try_into().unwrap_or([0; 2]));
    (Rarity::from_roll((dice % 1000) as u32), seed)
}

/// A new can's luck: `key` is the collection's secret, and the rest makes every can's roll its
/// own, two cans of a flavour added in the same second included.
pub fn draw(key: &[u8; 32], flavour: &str, at: i64, number: i64) -> (Rarity, u16) {
    let mut hash = Sha256::new();
    hash.update(b"flexspeak-monster-v2:");
    hash.update(key);
    hash.update(format!(":{flavour}:{at}:{number}"));
    from_hash(&hash.finalize())
}

/// What the leaderboard ranks by. Never by how many cans someone drinks: a bigger collection
/// ranks higher, not a bigger habit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Ranking {
    /// Different cards: each flavour at each rarity, out of `CARDS`.
    #[default]
    Collection,
    Legendary,
    Flavours,
}

/// Every flavour at every rarity: a complete collection.
pub const CARDS: usize = FLAVOURS.len() * Rarity::ALL.len();

impl Ranking {
    pub const ALL: [Self; 3] = [Self::Collection, Self::Legendary, Self::Flavours];

    pub fn name(self) -> &'static str {
        match self {
            Self::Collection => "Collection",
            Self::Legendary => "Legendary",
            Self::Flavours => "Flavours",
        }
    }
}

/// Where someone stands, for each way of ranking.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Standing {
    /// Different cards, out of `CARDS`.
    pub cards: usize,
    pub legendary: usize,
    /// Different flavours, out of `FLAVOURS.len()`.
    pub flavours: usize,
}

impl Standing {
    /// Counts a collection, given each can's flavour id and rarity.
    pub fn count<'a>(cans: impl IntoIterator<Item = (&'a str, Rarity)>) -> Self {
        let mut cards = HashSet::new();
        let mut flavours = HashSet::new();
        let mut legendary = 0;
        for (flavour, rarity) in cans {
            cards.insert((flavour, rarity));
            flavours.insert(flavour);
            legendary += usize::from(rarity == Rarity::Legendary);
        }
        Self {
            cards: cards.len(),
            legendary,
            flavours: flavours.len(),
        }
    }

    pub fn score(self, ranking: Ranking) -> usize {
        match ranking {
            Ranking::Collection => self.cards,
            Ranking::Legendary => self.legendary,
            Ranking::Flavours => self.flavours,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_odds_add_up_to_a_thousand() {
        let total: u32 = Rarity::ALL.iter().map(|r| r.permille()).sum();
        assert_eq!(total, 1000);
        assert_eq!(Rarity::from_roll(0), Rarity::Common);
        assert_eq!(Rarity::from_roll(599), Rarity::Common);
        assert_eq!(Rarity::from_roll(600), Rarity::Uncommon);
        assert_eq!(Rarity::from_roll(989), Rarity::Mythic);
        assert_eq!(Rarity::from_roll(990), Rarity::Legendary);
        assert_eq!(Rarity::from_roll(999), Rarity::Legendary);
    }

    #[test]
    fn every_flavour_has_a_unique_id() {
        let ids: HashSet<_> = FLAVOURS.iter().map(|f| f.id).collect();
        assert_eq!(ids.len(), FLAVOURS.len());
    }

    #[test]
    fn a_standing_counts_different_cards_not_cans() {
        let cans = [
            ("original", Rarity::Common),
            ("original", Rarity::Common),
            ("original", Rarity::Legendary),
            ("mango-loco", Rarity::Legendary),
        ];
        let standing = Standing::count(cans);
        assert_eq!(
            (standing.cards, standing.legendary, standing.flavours),
            (3, 2, 2)
        );
        assert_eq!(standing.score(Ranking::Flavours), 2);
    }

    #[test]
    fn a_rarity_travels_as_its_lowercase_name() {
        assert_eq!(
            serde_json::to_string(&Rarity::Legendary).unwrap(),
            "\"legendary\""
        );
        assert_eq!(
            serde_json::from_str::<Ranking>("\"collection\"").unwrap(),
            Ranking::Collection
        );
    }
}
