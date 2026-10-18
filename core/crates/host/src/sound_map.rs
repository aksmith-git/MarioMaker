//! Which of the game's sound users and keys the engine's objects and events play
//! (research/specs/audio-triggers.md has the evidence for each). Users are named
//! `<style>_<base>` in the sound database; the bases and keys here are the database's own
//! identifiers.

use mmk_sim::actors::Kind;
use mmk_sim::player::Form;

/// The user base names an actor kind plays its sounds through, best first (a style lacking the
/// first has the next).
pub fn actor_users(kind: Kind) -> &'static [&'static str] {
    use Kind as K;
    match kind {
        K::Goomba => &["Kuribo"],
        K::Goombrat => &["Kakibo", "Kuribo"],
        K::Koopa { red: false } => &["Nokonoko"],
        K::Koopa { red: true } => &["NokonokoRed", "Nokonoko"],
        K::Shell { red: false } => &["koura", "Nokonoko"],
        K::Shell { red: true } => &["koura", "NokonokoRed", "Nokonoko"],
        K::Piranha { fire: false } => &["Pakkun"],
        K::Piranha { fire: true } => &["FirePakkun", "Pakkun"],
        K::Thwomp { .. } => &["Dossun"],
        K::BulletBill => &["Killer"],
        K::BanzaiBill { .. } => &["MagnumKiller", "Killer"],
        K::HammerBro => &["Bros"],
        K::Hammer => &["Hammer"],
        K::BobOmb => &["Bombhei"],
        K::DryBones => &["Karon"],
        K::Boo | K::RingBoo { .. } => &["Teresa"],
        K::Blooper => &["Gesso"],
        K::BabyBlooper { .. } => &["GessoChild", "Gesso"],
        K::Spiny | K::SpinyEgg => &["Togezo"],
        K::Buzzy => &["Met"],
        K::SpikeTop { .. } => &["Togemet"],
        K::Lakitu => &["Jugem"],
        K::Cloud => &["JugemCloud"],
        K::Magikoopa => &["Kameck"],
        K::Magic => &["KameckMagic"],
        K::Wiggler => &["Hanachan"],
        K::ChainChomp { .. } => &["Wanwan"],
        K::LavaBubble => &["Bubble"],
        K::MontyMole => &["Choropoo"],
        K::AngrySun { moon: false } => &["Sun"],
        K::AngrySun { moon: true } => &["Moon"],
        K::FishBone => &["FishBone"],
        K::CheepCheep { red: false } => &["Pukupuku"],
        K::CheepCheep { red: true } => &["PukupukuRed", "Pukupuku"],
        K::RockyWrench => &["Poo"],
        K::Wrench => &["BulletSpanner"],
        K::Bowser { .. } => &["Koopa", "Koopa3W"],
        K::BowserFire => &["BulletKoopaFire"],
        K::BoomBoom { pom: true, .. } => &["Punpun", "Bunbun", "BunbunM3", "Bunbun3W"],
        K::BoomBoom { .. } => &["Bunbun", "BunbunM3", "Bunbun3W"],
        K::Shuriken => &["BulletPunpunShuriken"],
        K::BowserJr { .. } => &["KoopaJr"],
        K::ClownCar { .. } => &["KoopaClown"],
        K::Spring => &["JumpStep"],
        K::SideSpring => &["JumpStepSide"],
        K::Trampoline { .. } => &["Trampoline", "JumpStep"],
        K::PSwitch => &["PSwitch"],
        K::Pow => &["PowBlock"],
        K::Key => &["Key"],
        K::Mushroom => &["SuperKinoko"],
        K::FireFlower => &["FireFlower"],
        K::SuperballFlower => &["SuperBallFlower", "FireFlower"],
        K::Star => &["SuperStar"],
        K::OneUp => &["1upKinoko"],
        K::SuperHammer => &["SuperHammer"],
        K::Lift { .. } => &["Lift"],
        K::LavaLift { .. } => &["YouganLift"],
        K::Grinder => &["Saw"],
        K::Twister => &["Tornado"],
        K::Claw => &["SwingCrane"],
        K::ShoeGoomba { heels: false } => &["KutsuKuribo"],
        K::ShoeGoomba { heels: true } => &["KutsuKuriboHeels", "KutsuKuribo"],
        K::Shoe { .. } => &["Shoes"],
        K::Cannonball => &["BulletSenkanBall"],
        K::EnemyFire => &["EnmyFire"],
        K::Fireball => &["PlayerFire"],
        K::Superball => &["PlayerSuperBall", "PlayerFire"],
        K::Muncher => &["BlackPakkun"],
        K::Bumper => &["Marumaru"],
        K::Stone => &["Stone"],
        K::Crate { .. } => &["WoodBox"],
        K::BigCoin { value: 30 } => &["Coin30", "Coin10"],
        K::BigCoin { value: 50 } => &["Coin50", "Coin10"],
        K::BigCoin { .. } => &["Coin10"],
        K::FloatCoin => &["Coin"],
        K::AntTrooper { .. } => &["Arihei"],
        K::Stingby => &["Hacchin"],
        K::Skipsqueak { .. } => &["Pyonchu"],
        K::HopChops => &["Hopper"],
        K::Porcupuffer => &["Fugumannen"],
        K::Bully => &["Donketu"],
        K::Charvaargh => &["UganFish"],
        K::PiranhaCreeper { .. } => &["NobinobiPakkun"],
        K::KoopaCar => &["KoopaCar"],
        K::Yoshi { .. } => &["Yoshi"],
        K::YoshiEgg { .. } => &["YoshiEgg", "Yoshi"],
        _ => &[],
    }
}

/// The player's `PlayerMode` value for a form (the database's names).
pub fn player_mode(form: Form) -> &'static str {
    match form {
        Form::Small => "Normal",
        Form::Super => "Super",
        Form::Fire => "Fire",
        Form::Raccoon => "Sippo",
        Form::Cape => "Mant",
        Form::Propeller => "Propeller",
        Form::Cat => "Neko",
        Form::Builder => "Builder",
        Form::Superball => "SuperBall",
        Form::Mega => "Mega",
        // the Weird Mushroom's lanky Mario is the database's "Funny" mode (its own jump and
        // power-up waves); a costume is "Chara"
        Form::Lanky => "Funny",
        Form::Costume => "Chara",
        // the update forms, by the names the game gives them (forms 10–15)
        Form::Link => "Link",
        Form::Frog => "Frog",
        Form::Balloon => "Balloon",
        Form::Squirrel => "Flying",
        Form::Boomerang => "Boomerang",
        Form::Smb2 => "USA",
    }
}

/// Course themes by id as the music keys spell them.
pub const THEME_KEYS: [&str; 10] = [
    "Plain",
    "UnderGround",
    "Castle",
    "Airship",
    "Water",
    "HauntedHouse",
    "Snow",
    "Desert",
    "Athletic",
    "Woods",
];

/// The `@theme` values of the database (course themes by id).
pub const THEME_VALUES: [&str; 10] = [
    "plain",
    "underground",
    "castle",
    "airship",
    "water",
    "hauntedhouse",
    "snow",
    "desert",
    "athletic",
    "woods",
];

/// The course music key of a theme: `BGM_<Theme>_<Day|Night>[_Hurry]`.
pub fn bgm_key(theme: u8, night: bool, hurry: bool) -> String {
    format!(
        "BGM_{}_{}{}",
        THEME_KEYS[(theme as usize).min(9)],
        if night { "Night" } else { "Day" },
        if hurry { "_Hurry" } else { "" }
    )
}

/// Player keys the engine plays (for preloading).
pub const PLAYER_KEYS: &[&str] = &[
    "jump",
    "spinjump",
    "swim",
    "fireball",
    "r_power_up",
    "r_get_item",
    "r_power_down",
    "DokanIn",
    "block_hit",
    "pole",
    "r_MiddleFlag_On",
    "hipdrop",
    "hipdrop_start",
    "stomped",
    "key_get",
    "p_get_missionClear",
    "kabekick",
    "wallkick",
    "landing",
    "r_splash_in",
    "r_splash_out",
    "catchIvy",
    "climb",
    "kutsuIn",
    "marioRideOff",
    "turn_slip",
    "propellerjump",
    "footstep_L",
    "footstep_R",
    "DoorIn",
    "water_in",
    "water_out",
    "r_get_star",
    "voice_triple_jump1",
    "voice_triple_jump2",
    "voice_jump_high",
];

/// Keys of the shared object user (`<style>_ObjectCommon`).
pub const OBJECT_KEYS: &[&str] = &[
    "playCoin",
    "playCoin10",
    "playCoin30",
    "playCoin50",
    "block_hit",
    "block_break",
    "block_out",
    "onoff_sw",
    "key_appear",
    "p_GoalMission_Bubble_appear",
];

/// Keys each actor user may play (for preloading).
pub const ACTOR_KEYS: &[&str] = &[
    "r_damage",
    "r_damage_stamp",
    "r_damage_star",
    "r_damage_shell",
    "kick",
    "koura_hit",
    "block_out",
    "block_outy",
    "press",
    "r_shoot",
    "explode",
    "p_switch",
    "pow",
    "jump",
    "high_jump",
    "ignite",
    "shot_fire",
    "fire",
    "fly",
    "castspell",
    "eggcrack",
    "r_hit",
    "marioRideOn",
    "marioRideOff",
    "tongue",
];

/// Keys of the course music user (`CourseSceneBGM_<style>`), besides the theme tracks.
pub const MUSIC_KEYS: &[&str] = &[
    "BGM_Hurry",
    "BGM_Star",
    "BGM_StarHurry",
    "BGM_Switch",
    "BGM_SwitchHurry",
    "FanfareDown",
    "FanfareGoal",
];

/// Keys of `CourseSceneBGM_Common`.
pub const COMMON_KEYS: &[&str] = &[
    "play1UpSe",
    "play1UpSe_100coin",
    "playStarEndSign",
    "playAmbient",
];
