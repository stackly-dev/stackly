//! The start graph of Up (spec 0046, *Aturan graph*): a pure function over
//! the plan's order, its edges, and the phase of every service. It does no
//! IO and reads no clock; the driver calls it on every event and hands it
//! the time since the start stage began.

use std::collections::BTreeMap;
use std::time::Duration;

use stackly_domain::{MemberAlias, SkipReason, UpPhase};

/// One `dependsOn` edge of a dependent, with its effective timeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Edge {
    /// The service depended on.
    pub dependency: MemberAlias,
    /// How long the dependent waits for it once it is `starting`.
    pub timeout: Duration,
}

/// The services in start order and the edges of each.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Graph {
    order: Vec<MemberAlias>,
    edges: BTreeMap<MemberAlias, Vec<Edge>>,
}

impl Graph {
    /// Creates the graph of `order` (the plan's start order) with the
    /// edges of each dependent. An edge to an alias not in `order` is
    /// dropped: validate refused it already.
    #[must_use]
    pub fn new(order: Vec<MemberAlias>, edges: BTreeMap<MemberAlias, Vec<Edge>>) -> Self {
        let edges = edges
            .into_iter()
            .map(|(service, list)| {
                let kept = list
                    .into_iter()
                    .filter(|edge| order.contains(&edge.dependency))
                    .collect();
                (service, kept)
            })
            .collect();
        Self { order, edges }
    }

    /// Returns the services in start order.
    #[must_use]
    pub fn order(&self) -> &[MemberAlias] {
        &self.order
    }

    /// Returns the edges of `service`, empty when it depends on nothing.
    #[must_use]
    pub fn edges(&self, service: &MemberAlias) -> &[Edge] {
        self.edges.get(service).map_or(&[], Vec::as_slice)
    }
}

/// Where one service stands, as the graph reads it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NodeState {
    /// Its phase.
    pub phase: UpPhase,
    /// When it entered `starting`, as time since the start stage began;
    /// the timers of the edges to it count from here.
    pub starting_since: Option<Duration>,
}

impl NodeState {
    /// A service in `phase` that never entered `starting`.
    #[must_use]
    pub const fn new(phase: UpPhase) -> Self {
        Self {
            phase,
            starting_since: None,
        }
    }
}

/// What the graph decided for one service.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Decision {
    /// Every dependency is ready: start it now.
    Start(MemberAlias),
    /// It waits for the dependencies listed, in edge order.
    Wait {
        /// The service.
        service: MemberAlias,
        /// The dependencies that are not ready yet.
        waiting_on: Vec<MemberAlias>,
    },
    /// A dependency is not ready: the service ends `skipped`.
    Skip {
        /// The service.
        service: MemberAlias,
        /// The first dependency, in edge order, that was not ready.
        dependency: MemberAlias,
        /// Why.
        reason: SkipReason,
    },
}

/// Decides, at `now` since the start stage began, which services start,
/// wait, or end `skipped` (rules 2, 4, 5, and 6). A dependency that ended
/// in any phase but `ready` skips its dependents with `failed`, and that
/// carries through to their own dependents in the same call; a timer that
/// ran out while its dependency is still not ready skips with `timeout`.
/// At most `limit` services are `starting` at once, counting those that
/// already are; the ones allowed are picked by start order. A service
/// already `starting` or ended gets no decision.
#[must_use]
pub fn next(
    graph: &Graph,
    states: &BTreeMap<MemberAlias, NodeState>,
    now: Duration,
    limit: usize,
) -> Vec<Decision> {
    let mut states = states.clone();
    let mut running = states
        .values()
        .filter(|state| state.phase == UpPhase::Starting)
        .count();
    let mut decisions = Vec::new();
    for service in graph.order() {
        let Some(state) = states.get(service).copied() else {
            continue;
        };
        if !matches!(state.phase, UpPhase::Pending | UpPhase::Waiting) {
            continue;
        }
        let decision = decide(graph, &states, service, now);
        let decision = match decision {
            Decision::Start(_) if running >= limit => Decision::Wait {
                service: service.clone(),
                waiting_on: Vec::new(),
            },
            other => other,
        };
        let phase = match &decision {
            Decision::Start(_) => {
                running += 1;
                UpPhase::Starting
            }
            Decision::Wait { .. } => UpPhase::Waiting,
            Decision::Skip { .. } => UpPhase::Skipped,
        };
        states.insert(
            service.clone(),
            NodeState {
                phase,
                starting_since: (phase == UpPhase::Starting).then_some(now),
            },
        );
        decisions.push(decision);
    }
    decisions
}

fn decide(
    graph: &Graph,
    states: &BTreeMap<MemberAlias, NodeState>,
    service: &MemberAlias,
    now: Duration,
) -> Decision {
    let edges = graph.edges(service);
    let phase_of = |alias: &MemberAlias| {
        states
            .get(alias)
            .copied()
            .unwrap_or(NodeState::new(UpPhase::Cancelled))
    };
    if let Some(edge) = edges.iter().find(|edge| {
        let phase = phase_of(&edge.dependency).phase;
        phase.is_final() && phase != UpPhase::Ready
    }) {
        return Decision::Skip {
            service: service.clone(),
            dependency: edge.dependency.clone(),
            reason: SkipReason::Failed,
        };
    }
    if let Some(edge) = edges.iter().find(|edge| {
        let state = phase_of(&edge.dependency);
        state.phase != UpPhase::Ready
            && state
                .starting_since
                .is_some_and(|since| now.saturating_sub(since) >= edge.timeout)
    }) {
        return Decision::Skip {
            service: service.clone(),
            dependency: edge.dependency.clone(),
            reason: SkipReason::Timeout,
        };
    }
    let waiting_on: Vec<MemberAlias> = edges
        .iter()
        .filter(|edge| phase_of(&edge.dependency).phase != UpPhase::Ready)
        .map(|edge| edge.dependency.clone())
        .collect();
    if waiting_on.is_empty() {
        Decision::Start(service.clone())
    } else {
        Decision::Wait {
            service: service.clone(),
            waiting_on,
        }
    }
}

/// Returns when the next edge timer of a waiting service runs out, as time
/// since the start stage began; `None` when no timer runs. The driver
/// wakes at that time even when nothing else happens.
#[must_use]
pub fn next_deadline(graph: &Graph, states: &BTreeMap<MemberAlias, NodeState>) -> Option<Duration> {
    graph
        .order()
        .iter()
        .filter(|service| {
            states
                .get(*service)
                .is_some_and(|state| matches!(state.phase, UpPhase::Pending | UpPhase::Waiting))
        })
        .flat_map(|service| graph.edges(service))
        .filter_map(|edge| {
            let state = states.get(&edge.dependency)?;
            if state.phase == UpPhase::Ready || state.phase.is_final() {
                return None;
            }
            state.starting_since.map(|since| since + edge.timeout)
        })
        .min()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn alias(text: &str) -> MemberAlias {
        text.parse().unwrap()
    }

    fn edge(to: &str, seconds: u64) -> Edge {
        Edge {
            dependency: alias(to),
            timeout: Duration::from_secs(seconds),
        }
    }

    /// A graph over `order` with `edges` as (dependent, dependency).
    fn graph(order: &[&str], edges: &[(&str, &str)]) -> Graph {
        let mut map: BTreeMap<MemberAlias, Vec<Edge>> = BTreeMap::new();
        for (from, to) in edges {
            map.entry(alias(from)).or_default().push(edge(to, 600));
        }
        Graph::new(order.iter().map(|text| alias(text)).collect(), map)
    }

    fn states(entries: &[(&str, UpPhase)]) -> BTreeMap<MemberAlias, NodeState> {
        entries
            .iter()
            .map(|(name, phase)| (alias(name), NodeState::new(*phase)))
            .collect()
    }

    fn started(decisions: &[Decision]) -> Vec<String> {
        decisions
            .iter()
            .filter_map(|decision| match decision {
                Decision::Start(service) => Some(service.to_string()),
                _ => None,
            })
            .collect()
    }

    fn skipped(decisions: &[Decision]) -> Vec<(String, String, SkipReason)> {
        decisions
            .iter()
            .filter_map(|decision| match decision {
                Decision::Skip {
                    service,
                    dependency,
                    reason,
                } => Some((service.to_string(), dependency.to_string(), *reason)),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn a_chain_starts_one_service_at_a_time_in_dependency_order() {
        let graph = graph(&["db", "api", "web"], &[("api", "db"), ("web", "api")]);
        let mut now = states(&[
            ("db", UpPhase::Pending),
            ("api", UpPhase::Pending),
            ("web", UpPhase::Pending),
        ]);

        let first = next(&graph, &now, Duration::ZERO, 4);
        assert_eq!(started(&first), ["db"]);
        assert_eq!(
            first[1],
            Decision::Wait {
                service: alias("api"),
                waiting_on: vec![alias("db")],
            }
        );

        now.insert(alias("db"), NodeState::new(UpPhase::Ready));
        now.insert(alias("api"), NodeState::new(UpPhase::Waiting));
        now.insert(alias("web"), NodeState::new(UpPhase::Waiting));
        assert_eq!(started(&next(&graph, &now, Duration::ZERO, 4)), ["api"]);

        now.insert(alias("api"), NodeState::new(UpPhase::Ready));
        assert_eq!(started(&next(&graph, &now, Duration::ZERO, 4)), ["web"]);
    }

    #[test]
    fn a_diamond_starts_both_middles_together_and_the_top_after_both() {
        let graph = graph(
            &["db", "cache", "queue", "api"],
            &[
                ("cache", "db"),
                ("queue", "db"),
                ("api", "cache"),
                ("api", "queue"),
            ],
        );
        let mut now = states(&[
            ("db", UpPhase::Ready),
            ("cache", UpPhase::Pending),
            ("queue", UpPhase::Pending),
            ("api", UpPhase::Pending),
        ]);
        assert_eq!(
            started(&next(&graph, &now, Duration::ZERO, 4)),
            ["cache", "queue"]
        );

        now.insert(alias("cache"), NodeState::new(UpPhase::Ready));
        now.insert(
            alias("queue"),
            NodeState {
                phase: UpPhase::Starting,
                starting_since: Some(Duration::ZERO),
            },
        );
        now.insert(alias("api"), NodeState::new(UpPhase::Waiting));
        let decisions = next(&graph, &now, Duration::from_secs(1), 4);
        assert_eq!(
            decisions,
            [Decision::Wait {
                service: alias("api"),
                waiting_on: vec![alias("queue")],
            }]
        );

        now.insert(alias("queue"), NodeState::new(UpPhase::Ready));
        assert_eq!(started(&next(&graph, &now, Duration::ZERO, 4)), ["api"]);
    }

    #[test]
    fn a_failed_dependency_skips_its_dependents_and_theirs_in_one_call() {
        let graph = graph(
            &["db", "api", "web", "worker"],
            &[("api", "db"), ("web", "api")],
        );
        let now = states(&[
            ("db", UpPhase::Failed),
            ("api", UpPhase::Waiting),
            ("web", UpPhase::Waiting),
            ("worker", UpPhase::Pending),
        ]);

        let decisions = next(&graph, &now, Duration::ZERO, 4);

        assert_eq!(
            skipped(&decisions),
            [
                ("api".to_owned(), "db".to_owned(), SkipReason::Failed),
                ("web".to_owned(), "api".to_owned(), SkipReason::Failed),
            ]
        );
        assert_eq!(started(&decisions), ["worker"]);
    }

    #[test]
    fn needs_secret_skipped_and_cancelled_dependencies_count_as_failed() {
        for phase in [UpPhase::NeedsSecret, UpPhase::Skipped, UpPhase::Cancelled] {
            let graph = graph(&["db", "api"], &[("api", "db")]);
            let now = states(&[("db", phase), ("api", UpPhase::Waiting)]);

            assert_eq!(
                skipped(&next(&graph, &now, Duration::ZERO, 4)),
                [("api".to_owned(), "db".to_owned(), SkipReason::Failed)],
                "{phase:?}"
            );
        }
    }

    #[test]
    fn a_timer_counts_from_when_the_dependency_starts_and_skips_with_timeout() {
        let mut edges = BTreeMap::new();
        edges.insert(alias("api"), vec![edge("db", 1)]);
        let graph = Graph::new(vec![alias("db"), alias("api")], edges);
        let mut now = states(&[("db", UpPhase::Starting), ("api", UpPhase::Waiting)]);
        now.insert(
            alias("db"),
            NodeState {
                phase: UpPhase::Starting,
                starting_since: Some(Duration::from_secs(5)),
            },
        );

        // Install, create, and its own waits never count: before the
        // dependency starts, no timer runs.
        assert!(matches!(
            next(&graph, &now, Duration::from_millis(5_900), 4)[..],
            [Decision::Wait { .. }]
        ));
        assert_eq!(next_deadline(&graph, &now), Some(Duration::from_secs(6)));
        assert_eq!(
            skipped(&next(&graph, &now, Duration::from_secs(6), 4)),
            [("api".to_owned(), "db".to_owned(), SkipReason::Timeout)]
        );
    }

    #[test]
    fn a_waiting_dependency_runs_no_timer() {
        let mut edges = BTreeMap::new();
        edges.insert(alias("api"), vec![edge("db", 1)]);
        edges.insert(alias("db"), vec![edge("base", 600)]);
        let graph = Graph::new(vec![alias("base"), alias("db"), alias("api")], edges);
        let now = states(&[
            ("base", UpPhase::Starting),
            ("db", UpPhase::Waiting),
            ("api", UpPhase::Waiting),
        ]);

        assert!(skipped(&next(&graph, &now, Duration::from_secs(3_000), 4)).is_empty());
    }

    #[test]
    fn at_most_limit_services_start_and_the_earliest_in_order_go_first() {
        let graph = graph(&["a", "b", "c", "d", "e", "f"], &[]);
        let mut now = states(&[
            ("a", UpPhase::Starting),
            ("b", UpPhase::Pending),
            ("c", UpPhase::Pending),
            ("d", UpPhase::Pending),
            ("e", UpPhase::Pending),
            ("f", UpPhase::Pending),
        ]);
        now.insert(
            alias("a"),
            NodeState {
                phase: UpPhase::Starting,
                starting_since: Some(Duration::ZERO),
            },
        );

        assert_eq!(
            started(&next(&graph, &now, Duration::ZERO, 4)),
            ["b", "c", "d"]
        );
    }

    #[test]
    fn a_service_that_is_starting_or_ended_gets_no_decision() {
        let graph = graph(&["a", "b", "c"], &[]);
        let now = states(&[
            ("a", UpPhase::Starting),
            ("b", UpPhase::Ready),
            ("c", UpPhase::Cancelled),
        ]);

        assert!(next(&graph, &now, Duration::ZERO, 4).is_empty());
    }
}
