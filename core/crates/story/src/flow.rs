//! A small interpreter for BFEVFL event flows ([`mmk_formats::bfevfl`]).
//!
//! The interpreter walks the graph and hands every action and query to a [`Host`]: the host
//! implements the actors (the Story rules, the lesson editor, the screens). An action may ask the
//! runner to **wait** (a message window, a menu, a fade): the runner then stops after that action
//! and continues from the next event on [`Runner::run`]. Queries answer at once with an integer;
//! a switch with no case for the answer ends that branch.
//!
//! Sub-flow calls push a frame (the callee's entry-point variables, overridden by the call's
//! parameters, are what `Argument` values resolve to) and return to the call's `next`. Forks run
//! their branches one after the other, each until it reaches the join (or ends), then continue
//! after the join — the flows in SMM2 use forks only to overlap presentation (a fade with a camera
//! move), so sequential order keeps their meaning.

use std::collections::HashMap;

use mmk_formats::bfevfl::{EventFlowFile, EventKind, Flowchart, NONE, Params, Value};

/// Every flowchart available to the runner, by name.
#[derive(Clone, Debug, Default)]
pub struct Flows {
    charts: Vec<Flowchart>,
    index: HashMap<String, usize>,
}

impl Flows {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add the flowcharts of one file (a later file replaces a flowchart of the same name).
    pub fn add_file(&mut self, file: EventFlowFile) {
        for fc in file.flowcharts {
            self.add(fc);
        }
    }

    pub fn add(&mut self, fc: Flowchart) {
        match self.index.get(&fc.name) {
            Some(&i) => self.charts[i] = fc,
            None => {
                self.index.insert(fc.name.clone(), self.charts.len());
                self.charts.push(fc);
            }
        }
    }

    pub fn get(&self, name: &str) -> Option<&Flowchart> {
        self.index.get(name).map(|&i| &self.charts[i])
    }

    pub fn has_entry(&self, chart: &str, entry: &str) -> bool {
        self.get(chart)
            .is_some_and(|f| f.entry_point(entry).is_some())
    }

    pub fn names(&self) -> impl Iterator<Item = &str> {
        self.charts.iter().map(|f| f.name.as_str())
    }

    pub fn len(&self) -> usize {
        self.charts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.charts.is_empty()
    }
}

/// One action or query as the host sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// Flowchart and event name (for logs and tests).
    pub chart: String,
    pub event: String,
    /// Actor name and secondary name (e.g. `QuestCameraController`, `BuildMapCamera_01`).
    pub actor: String,
    pub actor_sub: String,
    /// Action or query name.
    pub name: String,
    /// Parameters with sub-flow arguments already resolved.
    pub params: Params,
}

impl Call {
    pub fn int(&self, key: &str) -> Option<i32> {
        self.params.get(key).and_then(Value::as_int)
    }
    pub fn float(&self, key: &str) -> Option<f32> {
        self.params.get(key).and_then(Value::as_float)
    }
    pub fn bool(&self, key: &str) -> Option<bool> {
        self.params.get(key).and_then(Value::as_bool)
    }
    pub fn str(&self, key: &str) -> Option<&str> {
        self.params.get(key).and_then(Value::as_str)
    }
}

/// What an action asks of the runner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Step {
    /// Go on to the next event.
    Next,
    /// Stop after this action until the host calls [`Runner::run`] again.
    Wait,
}

/// The actors of a flow.
pub trait Host {
    fn action(&mut self, call: &Call) -> Step;
    fn query(&mut self, call: &Call) -> i32;
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlowError {
    NoChart(String),
    NoEntry(String, String),
    /// More events than the step budget without waiting: a loop with no exit.
    Runaway(String),
}

impl std::fmt::Display for FlowError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FlowError::NoChart(c) => write!(f, "no flowchart {c}"),
            FlowError::NoEntry(c, e) => write!(f, "no entry point {c}::{e}"),
            FlowError::Runaway(at) => write!(f, "flow runs away at {at}"),
        }
    }
}

impl std::error::Error for FlowError {}

/// The runner's state after [`Runner::run`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// An action asked to wait; call `run` again to continue.
    Waiting,
    Finished,
}

#[derive(Clone, Debug)]
struct Fork {
    join: u16,
    rest: Vec<u16>,
}

#[derive(Clone, Debug)]
struct Frame {
    chart: String,
    at: u16,
    vars: Params,
    forks: Vec<Fork>,
}

/// Events run per [`Runner::run`] before the flow counts as runaway.
pub const STEP_BUDGET: usize = 200_000;

/// A flow in progress (one entry point and the sub-flows it called).
#[derive(Clone, Debug)]
pub struct Runner {
    frames: Vec<Frame>,
}

fn resolve(v: &Value, vars: &Params) -> Value {
    match v {
        Value::Argument(name) => vars.get(name).cloned().unwrap_or(Value::Int(0)),
        Value::Container(c) => Value::Container(resolve_params(c, vars)),
        other => other.clone(),
    }
}

fn resolve_params(p: &Params, vars: &Params) -> Params {
    Params(
        p.0.iter()
            .map(|(k, v)| (k.clone(), resolve(v, vars)))
            .collect(),
    )
}

fn frame(flows: &Flows, chart: &str, entry: &str, args: &Params) -> Result<Frame, FlowError> {
    let fc = flows
        .get(chart)
        .ok_or_else(|| FlowError::NoChart(chart.to_string()))?;
    let ep = fc
        .entry_point(entry)
        .ok_or_else(|| FlowError::NoEntry(chart.to_string(), entry.to_string()))?;
    let mut vars = ep.variables.clone();
    for (k, v) in &args.0 {
        match vars.0.iter_mut().find(|(n, _)| n == k) {
            Some(slot) => slot.1 = v.clone(),
            None => vars.0.push((k.clone(), v.clone())),
        }
    }
    Ok(Frame {
        chart: chart.to_string(),
        at: ep.main_event,
        vars,
        forks: Vec::new(),
    })
}

impl Runner {
    /// Start `chart::entry` with sub-flow style arguments.
    pub fn start(flows: &Flows, chart: &str, entry: &str, args: Params) -> Result<Self, FlowError> {
        Ok(Self {
            frames: vec![frame(flows, chart, entry, &args)?],
        })
    }

    pub fn is_finished(&self) -> bool {
        self.frames.is_empty()
    }

    /// The flowchart and event the runner will run next (None when finished).
    pub fn position(&self) -> Option<(&str, u16)> {
        self.frames.last().map(|f| (f.chart.as_str(), f.at))
    }

    /// Run until an action waits or the flow ends.
    pub fn run(&mut self, flows: &Flows, host: &mut dyn Host) -> Result<Status, FlowError> {
        for _ in 0..STEP_BUDGET {
            let Some(top) = self.frames.last_mut() else {
                return Ok(Status::Finished);
            };
            // a fork branch ends at its join (or at the end): start the next branch, or go on
            if let Some(fork) = top.forks.last_mut()
                && (top.at == fork.join || top.at == NONE)
            {
                if !fork.rest.is_empty() {
                    top.at = fork.rest.remove(0);
                    continue;
                }
                top.at = fork.join;
                top.forks.pop();
            }
            if top.at == NONE {
                self.frames.pop();
                continue;
            }
            let fc = flows
                .get(&top.chart)
                .ok_or_else(|| FlowError::NoChart(top.chart.clone()))?;
            let Some(event) = fc.events.get(top.at as usize) else {
                top.at = NONE;
                continue;
            };
            match &event.kind {
                EventKind::Action {
                    actor,
                    action,
                    params,
                    next,
                } => {
                    let a = &fc.actors[*actor as usize];
                    let call = Call {
                        chart: fc.name.clone(),
                        event: event.name.clone(),
                        actor: a.name.clone(),
                        actor_sub: a.secondary.clone(),
                        name: a.actions[*action as usize].clone(),
                        params: resolve_params(params, &top.vars),
                    };
                    top.at = *next;
                    if host.action(&call) == Step::Wait {
                        return Ok(Status::Waiting);
                    }
                }
                EventKind::Switch {
                    actor,
                    query,
                    params,
                    cases,
                } => {
                    let a = &fc.actors[*actor as usize];
                    let call = Call {
                        chart: fc.name.clone(),
                        event: event.name.clone(),
                        actor: a.name.clone(),
                        actor_sub: a.secondary.clone(),
                        name: a.queries[*query as usize].clone(),
                        params: resolve_params(params, &top.vars),
                    };
                    let answer = host.query(&call);
                    top.at = cases
                        .iter()
                        .find(|(v, _)| *v as i64 == answer as i64)
                        .map_or(NONE, |c| c.1);
                }
                EventKind::Fork { forks, join } => match forks.split_first() {
                    Some((first, rest)) => {
                        top.forks.push(Fork {
                            join: *join,
                            rest: rest.to_vec(),
                        });
                        top.at = *first;
                    }
                    None => top.at = *join,
                },
                EventKind::Join { next } => top.at = *next,
                EventKind::SubFlow {
                    flowchart,
                    entry_point,
                    params,
                    next,
                } => {
                    let chart = if flowchart.is_empty() {
                        top.chart.clone()
                    } else {
                        flowchart.clone()
                    };
                    let args = resolve_params(params, &top.vars);
                    top.at = *next;
                    let callee = frame(flows, &chart, entry_point, &args)?;
                    self.frames.push(callee);
                }
            }
        }
        let at = self
            .position()
            .map(|(c, e)| format!("{c}/E{e}"))
            .unwrap_or_default();
        Err(FlowError::Runaway(at))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use mmk_formats::bfevfl::{Actor, EntryPoint, Event};

    pub fn p(v: Vec<(&str, Value)>) -> Params {
        Params(v.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn act(actor: u16, action: u16, params: Params, next: u16) -> Event {
        Event {
            name: String::new(),
            kind: EventKind::Action {
                actor,
                action,
                params,
                next,
            },
        }
    }

    /// Main: ask Pick; 0 → Say(a) then call Sub(n=7); 1 → fork [Say(b), Say(c)] join → Say(d).
    /// Sub: Say(arg n) then Wait.
    pub fn sample() -> Flows {
        let mut flows = Flows::new();
        flows.add(Flowchart {
            name: "t".into(),
            actors: vec![Actor {
                name: "Sys".into(),
                actions: vec!["Say".into(), "Wait".into()],
                queries: vec!["Pick".into()],
                ..Default::default()
            }],
            events: vec![
                Event {
                    name: "E0".into(),
                    kind: EventKind::Switch {
                        actor: 0,
                        query: 0,
                        params: Params::default(),
                        cases: vec![(0, 1), (1, 3)],
                    },
                },
                act(0, 0, p(vec![("v", Value::String("a".into()))]), 2),
                Event {
                    name: "E2".into(),
                    kind: EventKind::SubFlow {
                        flowchart: String::new(),
                        entry_point: "Sub".into(),
                        params: p(vec![("n", Value::Int(7))]),
                        next: NONE,
                    },
                },
                Event {
                    name: "E3".into(),
                    kind: EventKind::Fork {
                        forks: vec![4, 5],
                        join: 6,
                    },
                },
                act(0, 0, p(vec![("v", Value::String("b".into()))]), 6),
                act(0, 0, p(vec![("v", Value::String("c".into()))]), NONE),
                Event {
                    name: "E6".into(),
                    kind: EventKind::Join { next: 7 },
                },
                act(0, 0, p(vec![("v", Value::String("d".into()))]), NONE),
                act(0, 0, p(vec![("v", Value::Argument("n".into()))]), 9),
                act(0, 1, Params::default(), NONE),
            ],
            entry_points: vec![
                EntryPoint {
                    name: "Main".into(),
                    main_event: 0,
                    ..Default::default()
                },
                EntryPoint {
                    name: "Sub".into(),
                    main_event: 8,
                    variables: p(vec![("n", Value::Int(1))]),
                    ..Default::default()
                },
            ],
        });
        flows
    }

    #[derive(Default)]
    struct Log {
        pick: i32,
        said: Vec<String>,
    }

    impl Host for Log {
        fn action(&mut self, c: &Call) -> Step {
            match c.name.as_str() {
                "Say" => {
                    let v = c.params.get("v").unwrap();
                    self.said.push(match v {
                        Value::Int(n) => n.to_string(),
                        v => v.as_str().unwrap().to_string(),
                    });
                    Step::Next
                }
                _ => Step::Wait,
            }
        }
        fn query(&mut self, _: &Call) -> i32 {
            self.pick
        }
    }

    #[test]
    fn switches_subflows_arguments_and_waits() {
        let flows = sample();
        let mut log = Log::default();
        let mut r = Runner::start(&flows, "t", "Main", Params::default()).unwrap();
        assert_eq!(r.run(&flows, &mut log), Ok(Status::Waiting));
        assert_eq!(log.said, ["a", "7"]);
        assert_eq!(r.run(&flows, &mut log), Ok(Status::Finished));
        assert!(r.is_finished());
    }

    #[test]
    fn forks_run_each_branch_then_the_join() {
        let flows = sample();
        let mut log = Log {
            pick: 1,
            ..Default::default()
        };
        let mut r = Runner::start(&flows, "t", "Main", Params::default()).unwrap();
        assert_eq!(r.run(&flows, &mut log), Ok(Status::Finished));
        assert_eq!(log.said, ["b", "c", "d"]);
    }

    #[test]
    fn unmatched_answers_end_and_bad_names_fail() {
        let flows = sample();
        let mut log = Log {
            pick: 9,
            ..Default::default()
        };
        let mut r = Runner::start(&flows, "t", "Main", Params::default()).unwrap();
        assert_eq!(r.run(&flows, &mut log), Ok(Status::Finished));
        assert!(log.said.is_empty());
        assert!(matches!(
            Runner::start(&flows, "t", "Nope", Params::default()),
            Err(FlowError::NoEntry(..))
        ));
        assert!(matches!(
            Runner::start(&flows, "x", "Main", Params::default()),
            Err(FlowError::NoChart(_))
        ));
    }

    #[test]
    fn loops_without_waits_are_caught() {
        let mut flows = Flows::new();
        flows.add(Flowchart {
            name: "l".into(),
            actors: vec![Actor {
                name: "A".into(),
                actions: vec!["Nop".into()],
                ..Default::default()
            }],
            events: vec![act(0, 0, Params::default(), 0)],
            entry_points: vec![EntryPoint {
                name: "Main".into(),
                main_event: 0,
                ..Default::default()
            }],
        });
        struct Busy;
        impl Host for Busy {
            fn action(&mut self, _: &Call) -> Step {
                Step::Next
            }
            fn query(&mut self, _: &Call) -> i32 {
                0
            }
        }
        let mut r = Runner::start(&flows, "l", "Main", Params::default()).unwrap();
        assert!(matches!(
            r.run(&flows, &mut Busy),
            Err(FlowError::Runaway(_))
        ));
    }
}
