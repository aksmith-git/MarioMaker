//! Clear conditions as the course header stores them (docs/formats/smm2-course.md §2.1): a
//! category (0x0F), a condition named by the CRC32 of its entry in the game's condition list
//! (0x10) and a magnitude (0x06).
//!
//! The list order and the hashes were checked against all 29 distinct conditions of the 550-course
//! sample (project finding). The labels are this project's English descriptions.

/// Header category: 0 none, 1 parts (defeat / collect / use N), 2 status (reach the goal in a
/// state), 3 actions (reach the goal without doing something).
pub const NONE: u8 = 0;
pub const PARTS: u8 = 1;
pub const STATUS: u8 = 2;
pub const ACTIONS: u8 = 3;

/// CRC32 of each condition, by list index − 1 (index 0 is "none", stored as hash 0).
pub const HASHES: [u32; 77] = [
    0x7f07acbf, 0xc77685e8, 0xcce12a46, 0xc7dad20f, 0x4b115542, 0xdf6717de, 0xe50302f7, 0xce9a707b,
    0xe47c2be8, 0xe25c5f60, 0x563755f2, 0xa3aec34a, 0x5a085610, 0x634a6671, 0xa09bb51f, 0x794c6eb3,
    0x387c22ca, 0x103bba8c, 0xfe75363e, 0x4c44da92, 0x7a128199, 0xce2e5a15, 0xcf81610a, 0x48d111e0,
    0x7f88648a, 0x501c7c00, 0x756120ee, 0xffe76309, 0xf571d608, 0x66c2b75e, 0xf0f35cba, 0xe3f62c75,
    0x4067ab4e, 0x3f50b513, 0x3f4124e8, 0x467afb58, 0x4b980b7f, 0xca315249, 0x3c164eb1, 0x6daa9a3f,
    0x405dce65, 0xed1023ea, 0x62b74a93, 0x2a8e77bb, 0xc73be7ec, 0x7c8612d5, 0x35a5af47, 0xbb926013,
    0x3a2f3996, 0x7ddb5d7f, 0xb7402052, 0x6a1ce415, 0x66477be4, 0x48b4157b, 0x63f3d532, 0xe6f2eebe,
    0x553a9590, 0x1a098d50, 0xfae86a49, 0xb4fa3d4b, 0xde0abfc6, 0xd9893249, 0x4c8772a3, 0xde56ffb5,
    0x97f8a309, 0xf55b3863, 0xc78f5040, 0xf5b932c2, 0xa3e39544, 0x1664515a, 0x08327ae6, 0x3eab9aa1,
    0x97f4cf7a, 0x3af23bde, 0xc2a80228, 0xef026a7f, 0x46219146,
];

/// English label of each condition subject, by list index − 1.
pub const LABELS: [&str; 77] = [
    "Goomba",
    "Goombrat",
    "Koopa Troopa",
    "Buzzy Beetle",
    "Spiny",
    "Piranha Plant",
    "Fire Piranha Plant",
    "Muncher",
    "Piranha Creeper",
    "Dry Bones",
    "Fish Bone",
    "Rocky Wrench",
    "Monty Mole",
    "Hammer Bro",
    "Sledge Bro",
    "Lakitu",
    "Blooper",
    "Cheep Cheep",
    "Spike Top",
    "Ant Trooper",
    "Lava Bubble",
    "Chain Chomp",
    "Wiggler",
    "Bob-omb",
    "Thwomp",
    "Boo",
    "Boo Ring",
    "Banzai Bill",
    "Magikoopa",
    "Angry Sun",
    "Skipsqueak",
    "Stingby",
    "Porcupuffer",
    "Charvaargh",
    "Porcupuffer (big)",
    "Hop-Chops",
    "Bowser",
    "Bowser Jr.",
    "Boom Boom",
    "Pom Pom",
    "Bullet Bill",
    "Super Mushroom",
    "1-Up Mushroom",
    "Fire Flower",
    "Super Star",
    "Big Mushroom",
    "Super Leaf",
    "Cape Feather",
    "Propeller Mushroom",
    "Super Bell",
    "Super Hammer",
    "Superball Flower",
    "POW Block",
    "Red POW Block",
    "P Switch",
    "Crate",
    "Trampoline",
    "Goomba's Shoe",
    "Yoshi",
    "Buzzy Shell",
    "Spiny Shell",
    "Koopa Clown Car",
    "Lakitu's Cloud",
    "Koopa Troopa Car",
    "Dry Bones Shell",
    "Coins",
    "10-Coins",
    "30-Coins",
    "50-Coins",
    "taking damage",
    "landing",
    "Koopa shell",
    "Swinging Claw",
    "water",
    "Bell Tree",
    "Toad (story)",
    "Stone (story)",
];

/// The hash of list index `index` (1-based; 0 = none → 0).
pub fn hash(index: u8) -> Option<u32> {
    match index {
        0 => Some(0),
        i => HASHES.get(i as usize - 1).copied(),
    }
}

/// The list index (1-based) of a header hash; 0 for 0 (none).
pub fn index(hash: u32) -> Option<u8> {
    if hash == 0 {
        return Some(0);
    }
    HASHES.iter().position(|&h| h == hash).map(|i| i as u8 + 1)
}

/// Categories a condition can be used with (from the game's condition-kind mapping,
/// research clear-conditions.md): parts for counts, status for "reach the goal as / with",
/// actions for "without".
pub fn categories(index: u8) -> &'static [u8] {
    match index {
        1..=41 => &[PARTS],
        43 => &[PARTS],
        42 | 44..=52 => &[PARTS, STATUS],
        53..=55 | 57 => &[PARTS, STATUS],
        56 | 75 => &[PARTS],
        58..=65 | 72 => &[STATUS],
        66..=69 => &[PARTS],
        70 | 71 | 73 | 74 => &[ACTIONS],
        _ => &[],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_are_crc32_of_known_entries() {
        assert_eq!(hash(1), Some(0x7f07acbf));
        assert_eq!(index(0x1664515a), Some(70));
        assert_eq!(index(0), Some(0));
        assert_eq!(index(1), None);
        assert_eq!(categories(70), &[ACTIONS]);
    }
}
