use std::collections::HashSet;

use crate::repo::CommitInfo;

#[derive(Clone, Debug, PartialEq)]
pub enum GraphCell {
    Empty,      // 0
    Node,       // 1
    Vertical,   // 2
    MergeLeft,  // 3
    MergeRight, // 4
    Crossing,   // 5 — vertical lane + horizontal merge passing through
}

#[derive(Clone, Debug)]
pub struct GraphCellInfo {
    pub cell: GraphCell,
    pub line_above: bool,
    pub line_below: bool,
    pub merge_from_left: bool,
    pub merge_from_right: bool,
}

impl GraphCellInfo {
    fn empty() -> Self {
        Self { cell: GraphCell::Empty, line_above: false, line_below: false, merge_from_left: false, merge_from_right: false }
    }
}

#[derive(Clone, Debug)]
pub struct GraphRow {
    pub commit_id: String,
    pub cells: Vec<GraphCellInfo>,
    pub node_column: usize,
}

pub fn compute_graph(commits: &[CommitInfo]) -> Vec<GraphRow> {
    let all_ids: HashSet<&str> = commits.iter().map(|c| c.id.as_str()).collect();
    let mut lanes: Vec<Option<String>> = Vec::new();
    let mut rows = Vec::new();

    for commit in commits {
        // Check if any lane already expects this commit (a child above connects to it)
        let found_existing = lanes.iter().any(|s| s.as_deref() == Some(&commit.id));

        let node_column = find_or_alloc(&mut lanes, &commit.id);

        // Find other lanes that also target this commit (merge sources)
        let merge_sources: Vec<usize> = lanes
            .iter()
            .enumerate()
            .filter(|(i, slot)| *i != node_column && slot.as_deref() == Some(&commit.id))
            .map(|(i, _)| i)
            .collect();

        let width = lanes.len();
        let mut cells: Vec<GraphCellInfo> = (0..width).map(|_| GraphCellInfo::empty()).collect();

        let node_line_below = commit.parents.iter().any(|p| all_ids.contains(p.as_str()));

        // Place the node
        cells[node_column] = GraphCellInfo {
            cell: GraphCell::Node,
            line_above: found_existing,
            line_below: node_line_below,
            merge_from_left: false,
            merge_from_right: false,
        };

        // Place vertical lines for other active lanes
        for (i, slot) in lanes.iter().enumerate() {
            if i == node_column || merge_sources.contains(&i) {
                continue;
            }
            if slot.is_some() {
                cells[i] = GraphCellInfo {
                    cell: GraphCell::Vertical,
                    line_above: true,
                    line_below: true,
                    merge_from_left: false,
                    merge_from_right: false,
                };
            }
        }

        // Place merge lines: fill ALL columns between merge source and node
        for &src in &merge_sources {
            let (lo, hi) = if src < node_column {
                (src, node_column)
            } else {
                (node_column, src)
            };

            // Fill intermediate columns
            for col in (lo + 1)..hi {
                if cells[col].cell == GraphCell::Vertical {
                    cells[col] = GraphCellInfo {
                        cell: GraphCell::Crossing,
                        line_above: true,
                        line_below: true,
                        merge_from_left: false,
                        merge_from_right: false,
                    };
                } else if cells[col].cell == GraphCell::Empty {
                    let merge_dir = if src < node_column {
                        GraphCell::MergeRight
                    } else {
                        GraphCell::MergeLeft
                    };
                    cells[col] = GraphCellInfo {
                        cell: merge_dir,
                        line_above: false,
                        line_below: false,
                        merge_from_left: false,
                        merge_from_right: false,
                    };
                }
                // If already a merge/crossing, leave it
            }

            // The merge source cell itself
            let merge_cell = if src < node_column {
                GraphCell::MergeRight
            } else {
                GraphCell::MergeLeft
            };
            cells[src] = GraphCellInfo {
                cell: merge_cell,
                line_above: true,
                line_below: false,
                merge_from_left: false,
                merge_from_right: false,
            };
        }

        for &src in &merge_sources {
            if src < node_column {
                cells[node_column].merge_from_left = true;
            } else {
                cells[node_column].merge_from_right = true;
            }
        }

        // Free merge source lanes
        for &src in &merge_sources {
            lanes[src] = None;
        }

        // Assign parents to lanes
        if commit.parents.is_empty() {
            lanes[node_column] = None;
        } else {
            lanes[node_column] = Some(commit.parents[0].clone());
            for parent in commit.parents.iter().skip(1) {
                let slot = find_free_or_append(&mut lanes);
                lanes[slot] = Some(parent.clone());
            }
        }

        // Orphan lane cleanup: free lanes targeting commits not in the dataset
        for lane in lanes.iter_mut() {
            if let Some(id) = lane.as_ref() {
                if !all_ids.contains(id.as_str()) {
                    *lane = None;
                }
            }
        }

        // Trim trailing empty lanes
        while lanes.last() == Some(&None) {
            lanes.pop();
        }

        rows.push(GraphRow {
            commit_id: commit.id.clone(),
            cells,
            node_column,
        });
    }

    rows
}

fn find_or_alloc(lanes: &mut Vec<Option<String>>, id: &str) -> usize {
    if let Some(pos) = lanes.iter().position(|s| s.as_deref() == Some(id)) {
        return pos;
    }
    find_free_or_append(lanes)
}

fn find_free_or_append(lanes: &mut Vec<Option<String>>) -> usize {
    if let Some(pos) = lanes.iter().position(|s| s.is_none()) {
        return pos;
    }
    lanes.push(None);
    lanes.len() - 1
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::repo;

    fn load_and_compute(path: &str) -> Vec<GraphRow> {
        let commits = repo::load_log(path).unwrap();
        compute_graph(&commits)
    }

    fn cell_types(row: &GraphRow) -> Vec<&GraphCell> {
        row.cells.iter().map(|c| &c.cell).collect()
    }

    #[test]
    fn test_linear() {
        let rows = load_and_compute("tests/samples/linear.json");
        assert_eq!(rows.len(), 4);
        // All nodes in column 0
        assert!(rows.iter().all(|r| r.node_column == 0));
        // All rows have exactly 1 cell (single column)
        assert!(rows.iter().all(|r| r.cells.len() == 1));
        // All cells are Node
        assert!(rows.iter().all(|r| r.cells[0].cell == GraphCell::Node));
        // First node: no line above (new lane)
        assert!(!rows[0].cells[0].line_above);
        assert!(rows[0].cells[0].line_below);
        // Middle nodes: both lines
        assert!(rows[1].cells[0].line_above);
        assert!(rows[1].cells[0].line_below);
        assert!(rows[2].cells[0].line_above);
        assert!(rows[2].cells[0].line_below);
        // Last node (root): no line below
        assert!(rows[3].cells[0].line_above);
        assert!(!rows[3].cells[0].line_below);
    }

    #[test]
    fn test_single() {
        let rows = load_and_compute("tests/samples/single.json");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].node_column, 0);
        assert_eq!(rows[0].cells.len(), 1);
        assert_eq!(rows[0].cells[0].cell, GraphCell::Node);
        assert!(!rows[0].cells[0].line_above);
        assert!(!rows[0].cells[0].line_below);
    }

    #[test]
    fn test_fork() {
        let rows = load_and_compute("tests/samples/fork.json");
        assert_eq!(rows.len(), 4);

        // Row 0: A in col 0 (no line above, line below)
        assert_eq!(rows[0].node_column, 0);
        assert!(!rows[0].cells[0].line_above);
        assert!(rows[0].cells[0].line_below);

        // Row 1: B in col 1 (no line above since B's lane is new, line below = true since parent C is in dataset)
        assert_eq!(rows[1].node_column, 1);
        assert!(!rows[1].cells[1].line_above);
        // Col 0 should be vertical (waiting for C)
        assert_eq!(rows[1].cells[0].cell, GraphCell::Vertical);

        // Row 2: C in col 0, merge from col 1
        assert_eq!(rows[2].node_column, 0);
        assert!(rows[2].cells[0].line_above);
        assert!(rows[2].cells[0].line_below);
        // Col 1 should be MergeLeft (merging left toward col 0)
        assert_eq!(rows[2].cells[1].cell, GraphCell::MergeLeft);
        assert!(rows[2].cells[1].line_above);   // lane was active
        assert!(!rows[2].cells[1].line_below);   // lane freed

        // Row 3: D in col 0 (root)
        assert_eq!(rows[3].node_column, 0);
        assert!(rows[3].cells[0].line_above);
        assert!(!rows[3].cells[0].line_below);
    }

    #[test]
    fn test_merge() {
        let rows = load_and_compute("tests/samples/merge.json");
        assert_eq!(rows.len(), 4);

        // Row 0: A in col 0, two parents → B stays in col 0, C gets col 1
        assert_eq!(rows[0].node_column, 0);
        assert!(!rows[0].cells[0].line_above);
        assert!(rows[0].cells[0].line_below);

        // Row 1: B in col 0, col 1 vertical (waiting for C)
        assert_eq!(rows[1].node_column, 0);
        assert_eq!(rows[1].cells[1].cell, GraphCell::Vertical);

        // Row 2: C in col 1
        assert_eq!(rows[2].node_column, 1);
        assert!(rows[2].cells[1].line_above);

        // Row 3: D in col 0, merge from col 1
        assert_eq!(rows[3].node_column, 0);
    }

    #[test]
    fn test_diamond() {
        let rows = load_and_compute("tests/samples/diamond.json");
        assert_eq!(rows.len(), 5);

        // Row 0: A
        assert_eq!(rows[0].node_column, 0);
        // Row 1: B (merge commit with 2 parents)
        assert_eq!(rows[1].node_column, 0);
        // Row 2: C in col 0
        assert_eq!(rows[2].node_column, 0);
        // Row 3: D in col 1
        assert_eq!(rows[3].node_column, 1);
        // Row 4: E in col 0, merge from col 1
        assert_eq!(rows[4].node_column, 0);
        assert_eq!(rows[4].cells[1].cell, GraphCell::MergeLeft);
    }

    #[test]
    fn test_orphan_only() {
        let rows = load_and_compute("tests/samples/orphan_only.json");
        assert_eq!(rows.len(), 3);

        // All nodes should be in col 0 (lanes freed by orphan cleanup)
        for row in &rows {
            assert_eq!(row.node_column, 0);
            assert_eq!(row.cells.len(), 1);
            assert_eq!(row.cells[0].cell, GraphCell::Node);
            // No lines: orphan parents freed, no children above
            assert!(!row.cells[0].line_below);
        }
        // First has no line above
        assert!(!rows[0].cells[0].line_above);
    }

    #[test]
    fn test_two_roots() {
        let rows = load_and_compute("tests/samples/two_roots.json");
        assert_eq!(rows.len(), 4);

        // Row 0: A in col 0
        assert_eq!(rows[0].node_column, 0);
        // Row 1: C in col 1
        assert_eq!(rows[1].node_column, 1);
        // Row 2: B in col 0 (root of chain 1)
        assert_eq!(rows[2].node_column, 0);
        assert!(rows[2].cells[0].line_above);
        assert!(!rows[2].cells[0].line_below);
        // Row 3: D in col 1 (root of chain 2)
        assert_eq!(rows[3].node_column, 1);
        assert!(rows[3].cells[1].line_above);
        assert!(!rows[3].cells[1].line_below);
    }

    #[test]
    fn test_wide() {
        let rows = load_and_compute("tests/samples/wide.json");
        assert_eq!(rows.len(), 6);

        // Rows 0-3: A1-A4 each in increasing columns
        assert_eq!(rows[0].node_column, 0);
        assert_eq!(rows[1].node_column, 1);
        assert_eq!(rows[2].node_column, 2);
        assert_eq!(rows[3].node_column, 3);

        // Row 4: P in col 0, merges from cols 1,2,3
        assert_eq!(rows[4].node_column, 0);
        assert!(rows[4].cells[0].line_above);
        assert!(rows[4].cells[0].line_below);

        // Row 5: ROOT in col 0
        assert_eq!(rows[5].node_column, 0);
        assert_eq!(rows[5].cells.len(), 1);
    }

    #[test]
    fn test_long_fork() {
        let rows = load_and_compute("tests/samples/long_fork.json");
        assert_eq!(rows.len(), 6);

        // Row 0: A in col 0
        assert_eq!(rows[0].node_column, 0);
        // Rows 1-3: B,C,D in col 1
        assert_eq!(rows[1].node_column, 1);
        assert_eq!(rows[2].node_column, 1);
        assert_eq!(rows[3].node_column, 1);
        // Col 0 should be vertical through rows 1-3
        assert_eq!(rows[1].cells[0].cell, GraphCell::Vertical);
        assert_eq!(rows[2].cells[0].cell, GraphCell::Vertical);
        assert_eq!(rows[3].cells[0].cell, GraphCell::Vertical);

        // Row 4: P in col 0, merge from col 1
        assert_eq!(rows[4].node_column, 0);
        assert_eq!(rows[4].cells[1].cell, GraphCell::MergeLeft);

        // Row 5: ROOT
        assert_eq!(rows[5].node_column, 0);
    }

    #[test]
    fn test_crossing() {
        let rows = load_and_compute("tests/samples/crossing.json");
        assert_eq!(rows.len(), 6);

        // Row 0: A in col 0 (parent D)
        assert_eq!(rows[0].node_column, 0);
        // Row 1: B in col 1 (parent E)
        assert_eq!(rows[1].node_column, 1);
        // Row 2: C in col 2 (parent D)
        assert_eq!(rows[2].node_column, 2);

        // Row 3: D in col 0. Merge from col 2 crosses active lane at col 1 → Crossing
        assert_eq!(rows[3].node_column, 0);
        assert_eq!(rows[3].cells[1].cell, GraphCell::Crossing);
        assert!(rows[3].cells[1].line_above);
        assert!(rows[3].cells[1].line_below);
        // Col 2 should be MergeLeft
        assert_eq!(rows[3].cells[2].cell, GraphCell::MergeLeft);
        assert!(rows[3].cells[2].line_above);
        assert!(!rows[3].cells[2].line_below);

        // Row 4: E in col 1
        assert_eq!(rows[4].node_column, 1);

        // Row 5: F in col 0, merge from col 1
        assert_eq!(rows[5].node_column, 0);
    }

    #[test]
    fn test_fork_with_orphan() {
        let rows = load_and_compute("tests/samples/fork_with_orphan.json");
        assert_eq!(rows.len(), 4);

        // Row 0: 4f4658 in col 0 (no line above, line below)
        assert_eq!(rows[0].node_column, 0);
        assert!(!rows[0].cells[0].line_above);
        assert!(rows[0].cells[0].line_below);

        // Row 1: b74d31 (zzkomqmq) — orphan parent, should have no line below after cleanup
        assert!(!rows[1].cells[rows[1].node_column].line_below);

        // Row 3: 636d81 should be a node with merge
        assert_eq!(rows[3].node_column, 0);
    }
}
