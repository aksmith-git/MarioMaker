//! Story Mode played through the game's own flows from an asset cache (skipped without one:
//! `MMK_ASSETS=<cache dir>`, else ~/smm-work/cache/smm2-3.0.3; the cache needs the `modes`
//! import stage).
//!
//! A scripted player answers every question with the first choice, takes the lowest job on
//! offer, orders the first affordable build, clears every job (collecting 20 coins) and talks
//! to everyone on the hub, round after round, until the castle is complete and the post-game is
//! done. The test checks the economy and the story beats along the way.

use std::path::PathBuf;
use std::sync::Arc;

use mmk_story::story::Request;
use mmk_story::{JobOutcome, Story, StoryData, StoryState};

fn cache() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("MMK_ASSETS")
        .map(PathBuf::from)
        .into_iter()
        .chain([home.join("smm-work/cache/smm2-3.0.3")])
        .find(|p| p.join("story/index.json").is_file())
}

#[derive(Default)]
struct Log {
    rewards: Vec<(i32, i32)>,
    courses: Vec<u8>,
    staff_rolls: usize,
    messages: usize,
    saves: usize,
    /// Take this job from the list instead of the lowest open one.
    prefer: Option<u8>,
    menus: usize,
    luigi: Option<u8>,
}

/// Drive the running flow to its end; returns the job entered, if any.
fn drain(story: &mut Story, log: &mut Log) -> Option<u8> {
    let mut entered = None;
    while let Some(req) = story.step().unwrap() {
        match req {
            Request::Message { .. } => {
                log.messages += 1;
                story.answer(0);
            }
            Request::JobList { only } => {
                let pick = match only.or(log.prefer.take()) {
                    Some(q) => Some(q),
                    None => story
                        .job_list(true)
                        .into_iter()
                        .next()
                        .or_else(|| story.job_list(false).into_iter().next()),
                };
                if let Some(q) = pick {
                    story.mark_displayed(q);
                }
                story.order_job(pick);
            }
            Request::BuildMenu { options } => {
                // crises: the Chief sells nothing while Red Toad / the crew is missing
                let st = &story.state;
                let crisis = (st.builds[5].finished && !st.jobs[114].cleared)
                    || ((12..15).all(|b| st.builds[b].finished) && !st.jobs[115].cleared);
                assert!(!crisis, "build menu during a crisis");
                log.menus += 1;
                let coins = story.state.coins;
                let pick = options
                    .iter()
                    .copied()
                    .find(|&b| story.data.builds[b as usize].cost <= coins);
                story.order_build(pick);
            }
            Request::Reward {
                collected, payment, ..
            } => log.rewards.push((collected, payment)),
            Request::EnterCourse { job } => {
                log.courses.push(job);
                entered = Some(job);
            }
            Request::StaffRoll { .. } => log.staff_rolls += 1,
            Request::Save => log.saves += 1,
            Request::Present(_) => {}
        }
    }
    entered
}

/// Play the job just entered (cleared, 20 coins) and run the hub's scene entry.
fn play(story: &mut Story, log: &mut Log, job: u8) {
    assert_eq!(story.job_play().map(|p| p.job), Some(job));
    // the job chosen for Luigi: TOO BAD, then Call Luigi
    let luigi = log.luigi == Some(job) && !story.state.jobs[job as usize].cleared;
    if luigi {
        let p = story.job_play().unwrap();
        while !p.miss() {}
        p.too_bad(mmk_story::TooBadChoice::CallLuigi);
        assert!(p.luigi && p.struggling);
    }
    story
        .finish_job(JobOutcome::Clear { coins: 20, luigi })
        .unwrap();
    drain(story, log);
    if log.staff_rolls > 0 && !story.state.staff_roll_seen {
        story.staff_roll_done().unwrap();
        drain(story, log);
    }
}

#[test]
fn story_mode_from_the_opening_to_the_post_game() {
    let Some(dir) = cache() else {
        eprintln!("skipped: no asset cache with Story data");
        return;
    };
    let data = Arc::new(StoryData::load_dir(&dir).unwrap());
    assert_eq!(data.jobs.len(), 120);
    let mut story = Story::new(data.clone(), StoryState::default());
    story.seed(7);
    let mut log = Log::default();

    assert!(story.start_opening().unwrap());
    drain(&mut story, &mut log);
    assert!(story.state.opening_seen);

    // first talk to the Chief offers job 0 at the Taskmaster
    story.talk("quest_BuildChief", "EntryPoint0").unwrap();
    drain(&mut story, &mut log);
    assert!(story.state.jobs[0].activated);
    assert_eq!(story.job_list(false), [0]);

    // the first job: paid on the first clear, a day passes
    story.enter_hub();
    story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
    let job = drain(&mut story, &mut log).expect("the Taskmaster sends Mario to job 0");
    assert_eq!(job, 0);
    play(&mut story, &mut log, 0);
    let pay0 = data.jobs[0].reward.coins();
    assert_eq!(story.state.coins, 20 + pay0);
    assert_eq!(story.state.days, 1);
    assert!(story.state.jobs[0].cleared);
    assert_eq!(story.state.ordered, None);

    // the Taskmaster sends Mario to the Chief; ordering the Foundation pays it in full and
    // brings jobs 1–3
    story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
    assert_eq!(drain(&mut story, &mut log), None);
    story.talk("quest_BuildChief", "EntryPoint0").unwrap();
    drain(&mut story, &mut log);
    let b0 = &data.builds[0];
    assert!(
        story.state.builds[0].building,
        "{:?}",
        story.state.builds[0]
    );
    assert_eq!(story.state.builds[0].days_left, b0.days);
    assert_eq!(story.state.coins, 20 + pay0 - b0.cost);
    assert_eq!(story.job_list(true), [1, 2, 3]);

    // a failure: nothing paid, no day, the build does not move
    let coins = story.state.coins;
    story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
    assert_eq!(drain(&mut story, &mut log), Some(1));
    story.finish_job(JobOutcome::Fail).unwrap();
    drain(&mut story, &mut log);
    assert_eq!((story.state.coins, story.state.days), (coins, 1));
    assert_eq!(story.state.builds[0].days_left, b0.days);
    assert!(!story.state.jobs[1].cleared && story.state.jobs[1].plays == 1);

    // a replay of job 0 pays only the coins collected, and still passes a day
    log.prefer = Some(0);
    story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
    assert_eq!(drain(&mut story, &mut log), Some(0));
    play(&mut story, &mut log, 0);
    assert_eq!((story.state.coins, story.state.days), (coins + 20, 2));
    assert_eq!(story.state.builds[0].days_left, b0.days - 1);

    // Luigi clears job 2 the first time it is played
    log.luigi = Some(2);

    // the rest: rounds of talking to everyone present and playing what they offer
    let mut rounds = 0;
    let mut ending_round = None;
    while rounds < 600 {
        rounds += 1;
        story.enter_hub();
        let mut played = false;
        for thing in story.hub_things() {
            if !data.flows.has_entry(&thing.flow, &thing.entry) {
                continue;
            }
            story.talk(&thing.flow, &thing.entry).unwrap();
            if let Some(job) = drain(&mut story, &mut log) {
                play(&mut story, &mut log, job);
                played = true;
            }
        }
        if story.castle_complete() && ending_round.is_none() {
            ending_round = Some(rounds);
        }
        if story.state.jobs[2].cleared && log.luigi.is_some() {
            // a Luigi clear is paid as usual but stamps the job …
            assert_eq!(story.stamp(2), 2);
            assert!(story.state.jobs[2].struggling, "TOO BAD after 5 attempts");
            if story.state.jobs.iter().filter(|j| j.cleared).count() > 100 {
                // … until Mario clears it himself
                log.luigi = None;
                log.prefer = Some(2);
                story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
                assert_eq!(drain(&mut story, &mut log), Some(2));
                play(&mut story, &mut log, 2);
                assert_eq!(story.stamp(2), 1);
            }
        }
        let all = story.state.jobs.iter().all(|j| j.cleared);
        let statue = (0..10).all(|i| story.state.flag(&format!("KinopikoOBJ_{i:03}")) & 1 != 0);
        if all && statue && story.state.gear >> 9 & 1 != 0 {
            break;
        }
        if !played {
            // nothing new anywhere: make some money on a replay
            story.talk("quest_QuestCounter", "EntryPoint0").unwrap();
            if let Some(job) = drain(&mut story, &mut log) {
                play(&mut story, &mut log, job);
            }
        }
    }
    let s = &story.state;
    let open: Vec<usize> = (0..120).filter(|&q| !s.jobs[q].cleared).collect();
    assert!(
        open.is_empty(),
        "jobs never cleared: {open:?} after {rounds} rounds"
    );
    assert!(ending_round.is_some(), "the castle was completed");
    assert!(s.builds.iter().all(|b| b.finished), "{:?}", s.builds);
    assert!(s.ending_seen && s.staff_roll_seen && s.builder_mario);
    // every gear reward: 9 job gear types + Builder Mario (5) + Coursebot all-clear (9)
    for kind in [0, 1, 2, 3, 4, 5, 6, 7, 8, 9] {
        assert!(
            s.gear >> kind & 1 != 0,
            "gear {kind} missing: {:#b}",
            s.gear
        );
    }
    assert!(s.jobs[117].cleared && s.jobs[118].cleared && s.jobs[119].cleared);
    // every first clear paid once: payments sum to the table's coin rewards
    let paid: i32 = log.rewards.iter().map(|r| r.1).sum();
    let table: i32 = data.jobs.iter().map(|j| j.reward.coins()).sum();
    assert_eq!(paid, table);
    // one day per cleared job (one course was a failure)
    assert_eq!(s.days as usize, log.courses.len() - 1);
    assert!(log.saves > 0 && log.messages > 100 && log.menus >= 16);
    assert!(!s.jobs.iter().any(|j| j.luigi));
    // the state survives a save and load
    let back = StoryState::from_json(&s.to_json());
    assert_eq!(&back, s);
    eprintln!(
        "{} courses, {} rounds, ending in round {:?}, {} days, {} coins",
        log.courses.len(),
        rounds,
        ending_round,
        s.days,
        s.coins
    );
}
