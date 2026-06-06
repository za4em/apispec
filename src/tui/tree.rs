use std::collections::{HashMap, HashSet};

use crate::spec::index::EndpointSummary;

const UNTAGGED_GROUP: &str = "Untagged";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GroupNode {
    pub id: String,
    pub label: String,
    pub endpoint_ids: Vec<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TreeRowKind {
    Group,
    Endpoint,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub kind: TreeRowKind,
    pub group_id: String,
    pub group_label: String,
    pub endpoint_id: Option<usize>,
    pub depth: u16,
    pub is_expanded: bool,
    pub is_match: bool,
}

#[derive(Debug, Clone)]
pub struct TreeModel {
    pub groups: Vec<GroupNode>,
    pub rows_visible: Vec<TreeRow>,
    pub expanded_groups: HashSet<String>,
    pub manual_expanded_groups: HashSet<String>,
    endpoint_positions_by_id: HashMap<usize, usize>,
    filtered_endpoint_count: usize,
    filter_active: bool,
}

impl TreeModel {
    pub fn new(endpoints: &[EndpointSummary]) -> Self {
        let groups = build_groups(endpoints);
        let endpoint_positions_by_id = endpoints
            .iter()
            .enumerate()
            .map(|(index, endpoint)| (endpoint.id, index))
            .collect::<HashMap<_, _>>();
        let manual_expanded_groups = groups.iter().map(|group| group.id.clone()).collect();

        let mut model = Self {
            groups,
            rows_visible: Vec::new(),
            expanded_groups: HashSet::new(),
            manual_expanded_groups,
            endpoint_positions_by_id,
            filtered_endpoint_count: 0,
            filter_active: false,
        };
        model.rebuild_visible_rows(endpoints, "");
        model
    }

    pub fn rebuild_visible_rows(&mut self, endpoints: &[EndpointSummary], query: &str) {
        let tokens = tokenize_query(query);
        self.filter_active = !tokens.is_empty();
        self.rows_visible.clear();
        self.filtered_endpoint_count = 0;

        if self.filter_active {
            self.expanded_groups.clear();
        } else {
            self.expanded_groups = self.manual_expanded_groups.clone();
        }

        for group in &self.groups {
            let group_matches = matches_all_tokens(&group.label.to_ascii_lowercase(), &tokens);
            let matching_endpoint_ids = group
                .endpoint_ids
                .iter()
                .copied()
                .filter(|endpoint_id| {
                    let Some(position) = self.endpoint_positions_by_id.get(endpoint_id).copied()
                    else {
                        return false;
                    };
                    let endpoint = &endpoints[position];
                    matches_all_tokens(&endpoint.search_text, &tokens)
                })
                .collect::<Vec<_>>();

            if self.filter_active {
                if !group_matches && matching_endpoint_ids.is_empty() {
                    continue;
                }
                self.filtered_endpoint_count += matching_endpoint_ids.len();
                self.expanded_groups.insert(group.id.clone());

                self.rows_visible.push(TreeRow {
                    kind: TreeRowKind::Group,
                    group_id: group.id.clone(),
                    group_label: group.label.clone(),
                    endpoint_id: None,
                    depth: 0,
                    is_expanded: true,
                    is_match: group_matches || !matching_endpoint_ids.is_empty(),
                });

                for endpoint_id in matching_endpoint_ids {
                    self.rows_visible.push(TreeRow {
                        kind: TreeRowKind::Endpoint,
                        group_id: group.id.clone(),
                        group_label: group.label.clone(),
                        endpoint_id: Some(endpoint_id),
                        depth: 1,
                        is_expanded: false,
                        is_match: true,
                    });
                }
                continue;
            }

            self.filtered_endpoint_count += group.endpoint_ids.len();
            let is_expanded = self.expanded_groups.contains(&group.id);
            self.rows_visible.push(TreeRow {
                kind: TreeRowKind::Group,
                group_id: group.id.clone(),
                group_label: group.label.clone(),
                endpoint_id: None,
                depth: 0,
                is_expanded,
                is_match: true,
            });

            if is_expanded {
                for endpoint_id in &group.endpoint_ids {
                    self.rows_visible.push(TreeRow {
                        kind: TreeRowKind::Endpoint,
                        group_id: group.id.clone(),
                        group_label: group.label.clone(),
                        endpoint_id: Some(*endpoint_id),
                        depth: 1,
                        is_expanded: false,
                        is_match: true,
                    });
                }
            }
        }
    }

    pub fn filtered_endpoint_count(&self) -> usize {
        self.filtered_endpoint_count
    }

    pub fn filter_active(&self) -> bool {
        self.filter_active
    }

    pub fn toggle_group(&mut self, group_id: &str) -> bool {
        if self.filter_active {
            return false;
        }

        if self.manual_expanded_groups.remove(group_id) {
            true
        } else {
            self.manual_expanded_groups.insert(group_id.to_owned())
        }
    }

    pub fn row_index_for_endpoint(&self, endpoint_id: usize) -> Option<usize> {
        self.rows_visible
            .iter()
            .position(|row| row.endpoint_id == Some(endpoint_id))
    }

    pub fn row_index_for_group(&self, group_id: &str) -> Option<usize> {
        self.rows_visible
            .iter()
            .position(|row| matches!(row.kind, TreeRowKind::Group) && row.group_id == group_id)
    }

    pub fn first_visible_endpoint_id(&self) -> Option<usize> {
        self.rows_visible.iter().find_map(|row| row.endpoint_id)
    }

    pub fn first_endpoint_row_index(&self) -> Option<usize> {
        self.rows_visible
            .iter()
            .position(|row| matches!(row.kind, TreeRowKind::Endpoint))
    }
}

#[derive(Debug, Clone)]
struct GroupDraft {
    id: String,
    label: String,
    sort_key: String,
    is_untagged: bool,
    endpoint_ids: Vec<usize>,
}

fn build_groups(endpoints: &[EndpointSummary]) -> Vec<GroupNode> {
    let mut groups = Vec::<GroupDraft>::new();
    let mut positions = HashMap::<String, usize>::new();

    for endpoint in endpoints {
        let key = endpoint.group_key.clone();
        let position = if let Some(existing) = positions.get(&key).copied() {
            existing
        } else {
            let index = groups.len();
            groups.push(GroupDraft {
                id: key.clone(),
                label: key.clone(),
                sort_key: endpoint.group_sort_key.clone(),
                is_untagged: endpoint.group_key == UNTAGGED_GROUP,
                endpoint_ids: Vec::new(),
            });
            positions.insert(key.clone(), index);
            index
        };

        groups[position].endpoint_ids.push(endpoint.id);
    }

    groups.sort_by(|left, right| match (left.is_untagged, right.is_untagged) {
        (true, false) => std::cmp::Ordering::Greater,
        (false, true) => std::cmp::Ordering::Less,
        _ => left
            .sort_key
            .cmp(&right.sort_key)
            .then(left.label.cmp(&right.label)),
    });

    groups
        .into_iter()
        .map(|group| GroupNode {
            id: group.id,
            label: group.label,
            endpoint_ids: group.endpoint_ids,
        })
        .collect()
}

fn tokenize_query(query: &str) -> Vec<String> {
    query
        .split_whitespace()
        .map(str::trim)
        .filter(|token| !token.is_empty())
        .map(|token| token.to_ascii_lowercase())
        .collect()
}

fn matches_all_tokens(haystack_lower: &str, tokens: &[String]) -> bool {
    tokens.iter().all(|token| haystack_lower.contains(token))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::spec::index::build_endpoint_index;
    use oas3::Spec;

    fn parse_spec(yaml: &str) -> Spec {
        serde_yaml::from_str::<Spec>(yaml).unwrap()
    }

    #[test]
    fn preserves_manual_expansion_when_filter_auto_expands() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: ["animals"]
      responses:
        "200":
          description: ok
  /users:
    get:
      tags: ["users"]
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut tree = TreeModel::new(&endpoints);

        assert!(tree.manual_expanded_groups.contains("animals"));
        assert!(tree.toggle_group("animals"));
        tree.rebuild_visible_rows(&endpoints, "");

        assert!(!tree.manual_expanded_groups.contains("animals"));
        assert!(!tree.expanded_groups.contains("animals"));

        tree.rebuild_visible_rows(&endpoints, "pets");

        assert!(tree.filter_active());
        assert!(tree.expanded_groups.contains("animals"));
        assert!(!tree.manual_expanded_groups.contains("animals"));

        tree.rebuild_visible_rows(&endpoints, "");

        assert!(!tree.expanded_groups.contains("animals"));
        assert!(!tree.filter_active());
    }

    #[test]
    fn filter_shows_matching_groups_and_matching_endpoints_only() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: ["animals"]
      responses:
        "200":
          description: ok
  /users:
    get:
      tags: ["users"]
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut tree = TreeModel::new(&endpoints);

        tree.rebuild_visible_rows(&endpoints, "users");

        assert_eq!(tree.filtered_endpoint_count(), 1);
        assert_eq!(tree.rows_visible.len(), 2);
        assert!(matches!(tree.rows_visible[0].kind, TreeRowKind::Group));
        assert_eq!(tree.rows_visible[0].group_label, "users");
        assert!(matches!(tree.rows_visible[1].kind, TreeRowKind::Endpoint));
    }

    #[test]
    fn filter_with_no_matches_returns_empty_projection() {
        let spec = parse_spec(
            r#"
openapi: 3.1.0
info:
  title: demo
  version: 1.0.0
paths:
  /pets:
    get:
      tags: ["animals"]
      responses:
        "200":
          description: ok
"#,
        );
        let endpoints = build_endpoint_index(&spec);
        let mut tree = TreeModel::new(&endpoints);

        tree.rebuild_visible_rows(&endpoints, "does-not-exist");

        assert!(tree.filter_active());
        assert!(tree.rows_visible.is_empty());
        assert_eq!(tree.filtered_endpoint_count(), 0);
    }

    #[test]
    fn handles_large_endpoint_collections() {
        let mut yaml = String::from(
            r#"
openapi: 3.1.0
info:
  title: large-demo
  version: 1.0.0
paths:
"#,
        );
        for index in 0..300 {
            yaml.push_str(&format!(
                "  /items/{index}:\n    get:\n      tags: [\"group{}\"]\n      responses:\n        \"200\":\n          description: ok\n",
                index % 12
            ));
        }

        let spec = parse_spec(&yaml);
        let endpoints = build_endpoint_index(&spec);
        let mut tree = TreeModel::new(&endpoints);

        assert_eq!(endpoints.len(), 300);
        assert_eq!(tree.filtered_endpoint_count(), 300);

        tree.rebuild_visible_rows(&endpoints, "group3");
        assert!(tree.filter_active());
        assert!(tree.filtered_endpoint_count() > 0);
        assert!(tree.filtered_endpoint_count() < 300);
        assert!(
            tree.rows_visible
                .iter()
                .any(|row| matches!(row.kind, TreeRowKind::Group))
        );
    }
}
