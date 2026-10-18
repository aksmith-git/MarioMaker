//! The Dojo from an asset cache (skipped without one, see `flows.rs`): the imported lesson list
//! parses into the Dojo catalogue, and every lesson flow runs from its start to `EndLesson`.

use std::path::PathBuf;
use std::sync::Arc;

use mmk_modes::dojo::{DojoSave, LessonCatalogue, LessonKind, Level, PowerUpLesson};
use mmk_story::StoryData;
use mmk_story::lesson::Lesson;

fn cache() -> Option<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    std::env::var_os("MMK_ASSETS")
        .map(PathBuf::from)
        .into_iter()
        .chain([home.join("smm-work/cache/smm2-3.0.3")])
        .find(|p| p.join("lessons/index.json").is_file())
}

#[test]
fn every_lesson_runs_to_its_end() {
    let Some(dir) = cache() else {
        eprintln!("skipped: no asset cache with lesson data");
        return;
    };
    let text = std::fs::read_to_string(dir.join("lessons/index.json")).unwrap();
    let cat = LessonCatalogue::from_json_str(&text).unwrap();
    for level in Level::ALL {
        assert_eq!(cat.level(level).len(), 15, "{level:?}");
    }
    let forms: Vec<u8> = [
        PowerUpLesson::Raccoon,
        PowerUpLesson::Cape,
        PowerUpLesson::Propeller,
        PowerUpLesson::Cat,
    ]
    .iter()
    .map(|&p| cat.power_up(p).expect("power-up lesson").form)
    .collect();
    assert_eq!(forms, [5, 4, 3, 7]);
    assert!(cat.basics().is_some());

    let data = StoryData::load_dir(&dir).unwrap();
    let flows = Arc::new(data.flows.clone());
    let mut save = DojoSave::default();
    let lessons: Vec<_> = Level::ALL
        .iter()
        .flat_map(|&l| cat.level(l))
        .chain(cat.basics())
        .chain(
            [PowerUpLesson::Raccoon, PowerUpLesson::Cat]
                .iter()
                .filter_map(|&p| cat.power_up(p)),
        )
        .collect();
    let mut steps = 0;
    for lesson in &lessons {
        let mut run = Lesson::start(flows.clone(), &lesson.flow, save.nina_intro_done)
            .unwrap_or_else(|e| panic!("{}: {e}", lesson.flow));
        // the editor queries: the player did what was asked (two ? Blocks placed / erased)
        while let Some(call) = run.next(&mut |_| 1).unwrap() {
            assert!(!call.actor.is_empty());
            steps += 1;
        }
        assert!(run.ended, "{} never reached EndLesson", lesson.flow);
        if run.intro_done {
            save.finish_self_introduction();
        }
        save.lesson_finished(&lesson.descriptor());
    }
    assert!(
        save.nina_intro_done,
        "Nina introduced herself in one of the lessons"
    );
    assert_eq!(save.complete_count(), 45, "power-up lessons set no mark");
    assert!(save.basics_done);
    assert_eq!(save.title_course_count(), 10);
    assert!(lessons.iter().any(|l| l.kind == LessonKind::Player));
    assert!(steps > 3000, "{steps} lesson actions");
}
