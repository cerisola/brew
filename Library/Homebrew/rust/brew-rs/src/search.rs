use crate::BrewResult;
use regex::Regex;
use rust_fuzzy_search::fuzzy_search_threshold;

pub(crate) fn search_names(names: &[String], query: &str) -> BrewResult<Vec<String>> {
    Ok(search_names_for_matcher(names, &Matcher::try_from(query)?))
}

fn search_names_for_matcher(names: &[String], matcher: &Matcher) -> Vec<String> {
    let matched_names = names
        .iter()
        .filter(|name| matcher.matches(name))
        .cloned()
        .collect::<Vec<_>>();
    if !matched_names.is_empty() {
        return matched_names;
    }

    let Matcher::String(query) = matcher else {
        return matched_names;
    };
    if query.len() < 3 {
        return matched_names;
    }

    let candidates = names.iter().map(String::as_str).collect::<Vec<_>>();
    let mut similar = fuzzy_search_threshold(query, &candidates, 0.5);
    similar.sort_by(|(_, left), (_, right)| right.total_cmp(left));
    similar
        .into_iter()
        .map(|(name, _)| name.to_string())
        .collect()
}

enum Matcher {
    Regex(Regex),
    String(String),
}

impl TryFrom<&str> for Matcher {
    type Error = anyhow::Error;

    fn try_from(query: &str) -> BrewResult<Self> {
        if query.len() > 2 && query.starts_with('/') && query.ends_with('/') {
            Regex::new(&query[1..query.len() - 1])
                .map(Self::Regex)
                .map_err(|error| anyhow::anyhow!("{query} is not a valid regex: {error}"))
        } else {
            Ok(Self::String(simplify_string(query)))
        }
    }
}

impl Matcher {
    fn matches(&self, value: &str) -> bool {
        match self {
            Self::Regex(regex) => regex.is_match(value),
            Self::String(string) => simplify_string(value).contains(string),
        }
    }
}

fn simplify_string(value: &str) -> String {
    value
        .chars()
        .filter_map(|character| {
            let lowered = character.to_ascii_lowercase();
            if lowered.is_ascii_alphanumeric() || matches!(lowered, '@' | '+') {
                Some(lowered)
            } else {
                None
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::{Matcher, search_names, simplify_string};

    #[test]
    fn parses_regex_queries() {
        let matcher = Matcher::try_from("/test.*/").unwrap();
        assert!(matches!(matcher, Matcher::Regex(_)));
    }

    #[test]
    fn rejects_invalid_regex_queries() {
        let error = match Matcher::try_from("/[/") {
            Ok(_) => panic!("expected invalid regex query to fail"),
            Err(error) => error,
        };

        assert!(error.to_string().contains("is not a valid regex"));
    }

    #[test]
    fn matches_simplified_plain_text_queries() {
        let matcher = Matcher::try_from("foo-bar").unwrap();
        assert!(matcher.matches("Foo Bar"));
    }

    #[test]
    fn simplify_string_keeps_only_matching_characters() {
        assert_eq!(simplify_string("Foo+Bar@1!"), "foo+bar@1");
    }

    #[test]
    fn returns_plain_text_matches_before_fuzzy_results() {
        let names = vec!["testball".to_string(), "another".to_string()];

        assert_eq!(
            search_names(&names, "testball").unwrap(),
            vec!["testball".to_string()]
        );
    }

    #[test]
    fn returns_fuzzy_matches_for_long_plain_text_queries() {
        let names = vec!["testball".to_string(), "other".to_string()];

        assert_eq!(
            search_names(&names, "testbal").unwrap(),
            vec!["testball".to_string()]
        );
    }

    #[test]
    fn does_not_use_fuzzy_matching_for_short_queries() {
        let names = vec!["foo-bar".to_string()];

        assert!(search_names(&names, "fb").unwrap().is_empty());
    }

    #[test]
    fn does_not_use_fuzzy_matching_for_regex_queries() {
        let names = vec!["testball".to_string()];

        assert!(search_names(&names, "/foo/").unwrap().is_empty());
    }
}
