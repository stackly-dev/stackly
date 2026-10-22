//! The name a duplicate starts with (spec 0024, *Nama duplicate*).

use stackly_domain::InstanceName;

use crate::instance::create::NAME_SUGGESTION_MAX_SUFFIX;

/// The longest instance name.
const NAME_MAX: usize = 63;
/// What a duplicate name ends with.
const COPY: &str = "-copy";

/// `<base>-copy`, then `<base>-copy-2` to `<base>-copy-99`, the first one
/// that no instance in `taken` has and that is a valid name. `<base>` is
/// the name of `source` without an ending `-copy` or `-copy-<n>`, cut from
/// the right so the result has at most 63 characters, without a hyphen at
/// the end. None when every candidate fails.
#[must_use]
pub fn duplicate_name(source: &InstanceName, taken: &[InstanceName]) -> Option<InstanceName> {
    let base = base_of(source.as_str());
    (1..=NAME_SUGGESTION_MAX_SUFFIX).find_map(|n| {
        let suffix = if n == 1 {
            COPY.to_owned()
        } else {
            format!("{COPY}-{n}")
        };
        let room = NAME_MAX.saturating_sub(suffix.len());
        let kept: String = base.chars().take(room).collect();
        let candidate: InstanceName = format!("{}{suffix}", kept.trim_end_matches('-'))
            .parse()
            .ok()?;
        (!taken.contains(&candidate)).then_some(candidate)
    })
}

/// The name without an ending `-copy` or `-copy-<n>` with n from 2 to 99,
/// unless that leaves nothing.
fn base_of(name: &str) -> &str {
    let stripped = name.strip_suffix(COPY).or_else(|| {
        let (head, number) = name.rsplit_once("-copy-")?;
        let n: u32 = number.parse().ok()?;
        ((2..=NAME_SUGGESTION_MAX_SUFFIX).contains(&n) && number == n.to_string()).then_some(head)
    });
    stripped.filter(|base| !base.is_empty()).unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(text: &str) -> InstanceName {
        text.parse().expect("a valid instance name")
    }

    fn names(texts: &[&str]) -> Vec<InstanceName> {
        texts.iter().map(|text| name(text)).collect()
    }

    #[test]
    fn a_free_name_gets_copy_after_the_source_name() {
        assert_eq!(
            duplicate_name(&name("pg17-dev"), &[]),
            Some(name("pg17-dev-copy"))
        );
    }

    #[test]
    fn a_taken_copy_name_counts_up_from_two() {
        assert_eq!(
            duplicate_name(&name("pg17-dev"), &names(&["pg17-dev-copy"])),
            Some(name("pg17-dev-copy-2"))
        );
        assert_eq!(
            duplicate_name(
                &name("pg17-dev"),
                &names(&["pg17-dev-copy", "pg17-dev-copy-2"])
            ),
            Some(name("pg17-dev-copy-3"))
        );
    }

    #[test]
    fn the_copy_ending_of_the_source_is_dropped_first() {
        assert_eq!(
            duplicate_name(&name("pg17-dev-copy"), &names(&["pg17-dev-copy"])),
            Some(name("pg17-dev-copy-2"))
        );
        assert_eq!(
            duplicate_name(&name("pg17-dev-copy-7"), &names(&["pg17-dev-copy-7"])),
            Some(name("pg17-dev-copy"))
        );
    }

    #[test]
    fn an_ending_that_is_not_a_copy_number_stays_part_of_the_base() {
        for (source, expected) in [
            ("pg17-copy-1", "pg17-copy-1-copy"),
            ("pg17-copy-100", "pg17-copy-100-copy"),
            ("pg17-copy-02", "pg17-copy-02-copy"),
            ("pg17-copy-x", "pg17-copy-x-copy"),
            ("pg17copy", "pg17copy-copy"),
        ] {
            assert_eq!(
                duplicate_name(&name(source), &[]),
                Some(name(expected)),
                "{source}"
            );
        }
    }

    #[test]
    fn a_long_base_is_cut_to_fit_sixty_three_characters() {
        let source = name(&format!("a{}", "b".repeat(62)));
        let first = duplicate_name(&source, &[]).expect("a candidate fits");

        assert_eq!(first.as_str(), format!("a{}-copy", "b".repeat(57)));
        assert_eq!(first.as_str().len(), 63);
        let second = duplicate_name(&source, std::slice::from_ref(&first)).expect("a candidate");
        assert_eq!(second.as_str(), format!("a{}-copy-2", "b".repeat(55)));
        assert_eq!(second.as_str().len(), 63);
    }

    #[test]
    fn a_hyphen_left_at_the_cut_is_dropped() {
        let source = name(&format!("{}-b{}", "a".repeat(57), "c".repeat(4)));
        // 57 a, a hyphen, then "bcccc": the cut at 58 characters ends on
        // the hyphen.
        let candidate = duplicate_name(&source, &[]).expect("a candidate");

        assert_eq!(candidate.as_str(), format!("{}-copy", "a".repeat(57)));
    }

    #[test]
    fn no_name_is_left_when_every_candidate_is_taken() {
        let mut taken = names(&["pg17-dev-copy"]);
        taken.extend((2..=99).map(|n| name(&format!("pg17-dev-copy-{n}"))));

        assert_eq!(duplicate_name(&name("pg17-dev"), &taken), None);
    }
}
