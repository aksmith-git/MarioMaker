//! Yamamura's Dojo lessons on the flow interpreter (coursebot.md §B.4).
//!
//! A lesson flow (`lesson_technique%03d`, `lesson_player%03d`, `lesson_tutorial000`, sharing
//! `lesson_subflows`) scripts Nina (`Character000`), Yamamura (`Character001`), the editor
//! (`Demo`: placing parts, demo replays, camera, UI) and waits (`System`). [`Lesson`] runs it and
//! hands every action to the lesson screen as a [`Call`]; the screen plays it and calls
//! [`Lesson::next`] again. Queries about the editor (`HasPlacedHatenaBlock`, …) go to the
//! screen's answer function; the ones about saved state are answered here:
//! - `HasAlreadyDoneSelfIntroduction` from [`Lesson::intro_done`] (set by
//!   `FinishSelfIntroduction`, which the Dojo save keeps);
//! - `HasChangedPlayerOperationByOption` from [`Lesson::dash_remapped`].
//!
//! `EndLesson` marks the lesson finished ([`Lesson::ended`]); the Dojo then records completion
//! and offers "watch again" / "leave".

use std::sync::Arc;

use mmk_formats::bfevfl::Params;

use crate::flow::{Call, FlowError, Flows, Host, Runner, Status, Step};

/// The entry point every lesson flow starts at.
pub const LESSON_ENTRY: &str = "EntryPoint0";

pub struct Lesson {
    flows: Arc<Flows>,
    runner: Option<Runner>,
    /// Nina has introduced herself (saved by the Dojo).
    pub intro_done: bool,
    /// The dash button is remapped in the options (picks the Y/B or A/B prompt variant).
    pub dash_remapped: bool,
    /// `EndLesson` has run.
    pub ended: bool,
}

struct LessonHost<'a> {
    intro_done: &'a mut bool,
    dash_remapped: bool,
    ended: &'a mut bool,
    out: &'a mut Option<Call>,
    answer: &'a mut dyn FnMut(&Call) -> i32,
}

impl Host for LessonHost<'_> {
    fn action(&mut self, call: &Call) -> Step {
        match call.name.as_str() {
            "FinishSelfIntroduction" => *self.intro_done = true,
            "EndLesson" => *self.ended = true,
            _ => {}
        }
        *self.out = Some(call.clone());
        Step::Wait
    }

    fn query(&mut self, call: &Call) -> i32 {
        match call.name.as_str() {
            "HasAlreadyDoneSelfIntroduction" => *self.intro_done as i32,
            "HasChangedPlayerOperationByOption" => self.dash_remapped as i32,
            _ => (self.answer)(call),
        }
    }
}

impl Lesson {
    /// Start lesson flow `flow` (e.g. `lesson_technique012`).
    pub fn start(flows: Arc<Flows>, flow: &str, intro_done: bool) -> Result<Self, FlowError> {
        let runner = Runner::start(&flows, flow, LESSON_ENTRY, Params::default())?;
        Ok(Self {
            flows,
            runner: Some(runner),
            intro_done,
            dash_remapped: false,
            ended: false,
        })
    }

    /// The next action for the screen to play (None: the lesson flow has finished).
    /// `answer` answers the editor queries (e.g. how many ? Blocks the player placed).
    pub fn next(
        &mut self,
        answer: &mut dyn FnMut(&Call) -> i32,
    ) -> Result<Option<Call>, FlowError> {
        let Some(mut runner) = self.runner.take() else {
            return Ok(None);
        };
        let mut out = None;
        let status = runner.run(
            &self.flows,
            &mut LessonHost {
                intro_done: &mut self.intro_done,
                dash_remapped: self.dash_remapped,
                ended: &mut self.ended,
                out: &mut out,
                answer,
            },
        )?;
        if status == Status::Waiting {
            self.runner = Some(runner);
        }
        Ok(out)
    }

    pub fn finished(&self) -> bool {
        self.runner.is_none()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::tests::sample;

    #[test]
    fn plays_every_action_and_answers_queries() {
        // the interpreter's sample flow: Pick → 1 runs the fork branch (three Say actions)
        let flows = Arc::new(sample());
        let mut l = Lesson {
            flows: flows.clone(),
            runner: Some(Runner::start(&flows, "t", "Main", Params::default()).unwrap()),
            intro_done: false,
            dash_remapped: false,
            ended: false,
        };
        let mut said = 0;
        while let Some(c) = l.next(&mut |_| 1).unwrap() {
            assert_eq!(c.name, "Say");
            said += 1;
        }
        assert_eq!(said, 3);
        assert!(l.finished() && !l.ended);
        assert!(matches!(
            Lesson::start(flows, "nope", false),
            Err(FlowError::NoChart(_))
        ));
    }
}
