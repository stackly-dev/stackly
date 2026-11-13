//! *Urutan* (spec 0047 AC-6): instances stop in reverse dependency order.
//! An instance may begin to stop once every member of the stop set that
//! depends on it has ended, whatever its result.

use std::collections::{BTreeMap, BTreeSet};

use stackly_domain::{EnvironmentMember, InstanceId, MemberAlias};

/// The instances of `stop` that may begin to stop now, at most `capacity`
/// minus those of `started` that have not `ended`, in alias order and then
/// ID (an instance with no member row comes last).
///
/// An instance P may begin once every member D in `stop` whose
/// `dependency_conditions` name P's alias has ended. An edge to an alias
/// that is gone, and an edge of a member outside `stop`, are ignored. When
/// nothing may begin and nothing runs but instances are left (broken edge
/// data), the rest begin in alias order; cycles never pass validate.
#[must_use]
pub fn order(
    members: &[EnvironmentMember],
    stop: &[InstanceId],
    started: &BTreeSet<InstanceId>,
    ended: &BTreeSet<InstanceId>,
    capacity: usize,
) -> Vec<InstanceId> {
    let in_stop: BTreeSet<InstanceId> = stop.iter().copied().collect();
    let alias_of: BTreeMap<InstanceId, &MemberAlias> = members
        .iter()
        .map(|member| (member.instance_id(), member.alias()))
        .collect();
    let running = started.iter().filter(|id| !ended.contains(id)).count();
    let room = capacity.saturating_sub(running);
    let mut left: Vec<InstanceId> = in_stop
        .iter()
        .copied()
        .filter(|id| !started.contains(id) && !ended.contains(id))
        .collect();
    left.sort_by(|a, b| {
        let key = |id: &InstanceId| (!alias_of.contains_key(id), alias_of.get(id).copied(), *id);
        key(a).cmp(&key(b))
    });
    if room == 0 || left.is_empty() {
        return Vec::new();
    }
    let free = |candidate: &InstanceId| {
        let Some(alias) = alias_of.get(candidate) else {
            return true;
        };
        members
            .iter()
            .filter(|member| in_stop.contains(&member.instance_id()))
            .filter(|member| member.instance_id() != *candidate)
            .filter(|member| {
                member
                    .dependency_conditions()
                    .iter()
                    .any(|condition| condition.service() == *alias)
            })
            .all(|member| ended.contains(&member.instance_id()))
    };
    let ready: Vec<InstanceId> = left.iter().copied().filter(|id| free(id)).collect();
    if ready.is_empty() && running == 0 {
        return left.into_iter().take(room).collect();
    }
    ready.into_iter().take(room).collect()
}

#[cfg(test)]
mod tests {
    use stackly_domain::{DependencyCondition, MemberDependency, Ownership};

    use super::*;

    fn id(n: u128) -> InstanceId {
        InstanceId::from_uuid(uuid::Uuid::from_u128(
            0x0190_0000_0000_7000_8000_0000_0000_0000 | n,
        ))
        .unwrap()
    }

    fn member(alias: &str, n: u128, on: &[&str]) -> EnvironmentMember {
        EnvironmentMember::new(alias.parse().unwrap(), id(n), Ownership::Dedicated).with_conditions(
            on.iter()
                .map(|dep| {
                    MemberDependency::new(dep.parse().unwrap(), DependencyCondition::Ready, 600)
                })
                .collect(),
        )
    }

    fn set(ids: &[u128]) -> BTreeSet<InstanceId> {
        ids.iter().map(|n| id(*n)).collect()
    }

    #[test]
    fn a_chain_of_three_stops_from_the_last_dependent_back() {
        // api -> cache -> db: api stops first, db last.
        let members = [
            member("db", 1, &[]),
            member("cache", 2, &["db"]),
            member("api", 3, &["cache"]),
        ];
        let stop = [id(1), id(2), id(3)];

        assert_eq!(order(&members, &stop, &set(&[]), &set(&[]), 4), [id(3)]);
        assert_eq!(
            order(&members, &stop, &set(&[3]), &set(&[]), 4),
            Vec::<InstanceId>::new(),
            "cache waits for api to end"
        );
        assert_eq!(order(&members, &stop, &set(&[3]), &set(&[3]), 4), [id(2)]);
        assert_eq!(
            order(&members, &stop, &set(&[2, 3]), &set(&[2, 3]), 4),
            [id(1)]
        );
    }

    #[test]
    fn a_diamond_stops_both_middles_together_and_the_base_after_both() {
        // web -> {api, jobs} -> db.
        let members = [
            member("db", 1, &[]),
            member("api", 2, &["db"]),
            member("jobs", 3, &["db"]),
            member("web", 4, &["api", "jobs"]),
        ];
        let stop = [id(1), id(2), id(3), id(4)];

        assert_eq!(order(&members, &stop, &set(&[]), &set(&[]), 4), [id(4)]);
        assert_eq!(
            order(&members, &stop, &set(&[4]), &set(&[4]), 4),
            [id(2), id(3)],
            "alias order"
        );
        assert_eq!(
            order(&members, &stop, &set(&[2, 3, 4]), &set(&[2, 4]), 4),
            Vec::<InstanceId>::new(),
            "db waits for jobs too"
        );
        assert_eq!(
            order(&members, &stop, &set(&[2, 3, 4]), &set(&[2, 3, 4]), 4),
            [id(1)]
        );
    }

    #[test]
    fn a_dependent_outside_the_stop_set_does_not_hold_its_dependency() {
        // api keeps running (not in stop), so db begins at once.
        let members = [member("db", 1, &[]), member("api", 2, &["db"])];

        assert_eq!(order(&members, &[id(1)], &set(&[]), &set(&[]), 4), [id(1)]);
    }

    #[test]
    fn a_dependent_that_ended_kept_or_failed_lets_its_dependency_begin() {
        let members = [member("db", 1, &[]), member("api", 2, &["db"])];
        let stop = [id(1), id(2)];

        // api ended, whatever its result: db may begin.
        assert_eq!(order(&members, &stop, &set(&[2]), &set(&[2]), 4), [id(1)]);
    }

    #[test]
    fn at_most_the_capacity_begins_and_running_stops_count() {
        let members: Vec<EnvironmentMember> =
            (1..=6).map(|n| member(&format!("s{n}"), n, &[])).collect();
        let stop: Vec<InstanceId> = (1..=6).map(id).collect();

        assert_eq!(
            order(&members, &stop, &set(&[]), &set(&[]), 4),
            [id(1), id(2), id(3), id(4)]
        );
        assert_eq!(
            order(&members, &stop, &set(&[1, 2, 3, 4]), &set(&[1]), 4),
            [id(5)]
        );
    }

    #[test]
    fn an_instance_with_no_member_row_begins_freely_and_comes_last() {
        let members = [member("db", 1, &[])];
        let stop = [id(9), id(1)];

        assert_eq!(
            order(&members, &stop, &set(&[]), &set(&[]), 4),
            [id(1), id(9)]
        );
    }

    #[test]
    fn broken_edges_that_hold_everything_still_let_the_rest_begin() {
        // A cycle never passes validate; the data says one anyway.
        let members = [member("a", 1, &["b"]), member("b", 2, &["a"])];
        let stop = [id(1), id(2)];

        assert_eq!(
            order(&members, &stop, &set(&[]), &set(&[]), 4),
            [id(1), id(2)]
        );
    }
}
