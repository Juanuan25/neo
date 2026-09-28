//! Lane assignment for a DAG drawn git-graph style on shared timeline rows.
//!
//! Rows run top (newest) to bottom (oldest). Each node sits in one row; a DAG
//! only occupies some rows (the other graph owns the rest), which is fine:
//! edges simply pass through rows without a node.
//!
//! Classic "expected id per lane" sweep: a lane carries the id of the node it
//! is waiting for. A node takes the leftmost lane waiting for it (or the first
//! free lane when nothing waits — a branch tip); other lanes waiting for the
//! same node end there with a curve into it (a fork point, seen from above).
//! The first parent continues in the node's own lane; further parents (merge
//! commits) bend into the lane that already waits for them, or a free one.
//! Free lanes are reused, so width stays at the number of lines that are
//! actually open at once.
use std::collections::HashMap;

use serde::Serialize;

/// One node for [`assign_lanes`]: its timeline row, id and parent ids
/// (first parent first). Parents that never show up are drawn as lines that
/// continue past the bottom of the window.
#[derive(Clone, Debug)]
pub struct DagNode {
    pub row: usize,
    pub id: String,
    pub parents: Vec<String>,
}

/// An edge child (upper row) → parent (lower row).
///
/// Drawing: leave the child at `from_lane`; if `lane != from_lane` bend into
/// `lane` right below the child; run straight down `lane`; if
/// `to_lane != lane` bend into `to_lane` right above the parent.
#[derive(Serialize, Clone, Debug, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct Edge {
    pub from_row: usize,
    pub from_lane: usize,
    /// None: the parent is outside the window (line continues downward).
    pub to_row: Option<usize>,
    pub to_lane: usize,
    /// Lane the edge travels in between the two rows.
    pub lane: usize,
    /// Second or later parent (merge commit).
    pub merge: bool,
}

#[derive(Serialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct LaneLayout {
    /// Node id → lane.
    #[serde(skip)]
    pub lane_of: HashMap<String, usize>,
    pub edges: Vec<Edge>,
    /// Number of lanes used (graph width).
    pub lanes: usize,
}

fn free_slot(active: &mut Vec<Option<String>>, avoid: Option<usize>) -> usize {
    if let Some(i) = active
        .iter()
        .enumerate()
        .position(|(i, s)| s.is_none() && Some(i) != avoid)
    {
        return i;
    }
    active.push(None);
    active.len() - 1
}

/// Assign lanes to `nodes` (any order; sorted by row here) and route edges.
pub fn assign_lanes(nodes: &[DagNode]) -> LaneLayout {
    let mut order: Vec<&DagNode> = nodes.iter().collect();
    order.sort_by_key(|n| n.row);

    // lane → id of the node the lane is waiting for
    let mut active: Vec<Option<String>> = Vec::new();
    // parent id → indices of edges still waiting for it
    let mut pending: HashMap<String, Vec<usize>> = HashMap::new();
    let mut edges: Vec<Edge> = Vec::new();
    let mut lane_of: HashMap<String, usize> = HashMap::new();
    let mut width = 0usize;

    for n in order {
        if lane_of.contains_key(&n.id) {
            continue; // duplicate id: keep the first (newest) occurrence
        }
        let hits: Vec<usize> = active
            .iter()
            .enumerate()
            .filter(|(_, s)| s.as_deref() == Some(n.id.as_str()))
            .map(|(i, _)| i)
            .collect();
        let lane = match hits.first() {
            Some(&l) => l,
            None => free_slot(&mut active, None),
        };
        for &h in &hits {
            active[h] = None;
        }
        lane_of.insert(n.id.clone(), lane);
        if let Some(waiting) = pending.remove(&n.id) {
            for ei in waiting {
                edges[ei].to_row = Some(n.row);
                edges[ei].to_lane = lane;
            }
        }

        let mut seen: Vec<&str> = Vec::new();
        for (k, p) in n.parents.iter().enumerate() {
            if seen.contains(&p.as_str()) || p == &n.id {
                continue;
            }
            seen.push(p);
            let travel = if k == 0 {
                active[lane] = Some(p.clone());
                lane
            } else if let Some(j) = active.iter().position(|s| s.as_deref() == Some(p.as_str())) {
                j
            } else {
                let j = free_slot(&mut active, Some(lane));
                active[j] = Some(p.clone());
                j
            };
            edges.push(Edge {
                from_row: n.row,
                from_lane: lane,
                to_row: None,
                to_lane: travel,
                lane: travel,
                merge: k > 0,
            });
            pending.entry(p.clone()).or_default().push(edges.len() - 1);
        }
        width = width.max(active.len());
        // Trim trailing free lanes so later tips reuse the left side.
        while active.last().is_some_and(|s| s.is_none()) {
            active.pop();
        }
    }

    LaneLayout {
        lane_of,
        edges,
        lanes: width,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(row: usize, id: &str, parents: &[&str]) -> DagNode {
        DagNode {
            row,
            id: id.to_string(),
            parents: parents.iter().map(|s| s.to_string()).collect(),
        }
    }

    fn lane(l: &LaneLayout, id: &str) -> usize {
        l.lane_of[id]
    }

    #[test]
    fn linear_history_is_one_lane() {
        let l = assign_lanes(&[n(0, "c", &["b"]), n(1, "b", &["a"]), n(2, "a", &[])]);
        assert_eq!(l.lanes, 1);
        assert!(["a", "b", "c"].iter().all(|id| lane(&l, id) == 0));
        assert_eq!(l.edges.len(), 2);
        assert!(l
            .edges
            .iter()
            .all(|e| e.lane == 0 && e.from_lane == 0 && e.to_lane == 0));
        assert_eq!(l.edges[0].to_row, Some(1));
        assert_eq!(l.edges[1].to_row, Some(2));
    }

    #[test]
    fn restore_then_edit_forks_a_second_lane() {
        // p ← x ← y (old line), then "restore p" and edit: p ← r (new tip on top).
        let l = assign_lanes(&[
            n(0, "r", &["p"]),
            n(1, "y", &["x"]),
            n(2, "x", &["p"]),
            n(3, "p", &[]),
        ]);
        assert_eq!(l.lanes, 2);
        assert_eq!(lane(&l, "r"), 0);
        assert_eq!(lane(&l, "y"), 1);
        assert_eq!(lane(&l, "x"), 1);
        assert_eq!(lane(&l, "p"), 0);
        // x → p travels in lane 1 and bends into lane 0 right above p.
        let xp = l.edges.iter().find(|e| e.from_row == 2).unwrap();
        assert_eq!((xp.lane, xp.to_lane, xp.to_row), (1, 0, Some(3)));
        // r → p is a straight line in lane 0 through rows 1 and 2.
        let rp = l.edges.iter().find(|e| e.from_row == 0).unwrap();
        assert_eq!((rp.lane, rp.to_lane, rp.to_row), (0, 0, Some(3)));
    }

    #[test]
    fn freed_lanes_are_reused() {
        // Two short side branches one after another: both use lane 1.
        let l = assign_lanes(&[
            n(0, "a", &["b"]),
            n(1, "s1", &["b"]),
            n(2, "b", &["c"]),
            n(3, "s2", &["d"]),
            n(4, "c", &["d"]),
            n(5, "d", &[]),
        ]);
        assert_eq!(lane(&l, "s1"), 1);
        assert_eq!(lane(&l, "s2"), 1);
        assert_eq!(l.lanes, 2);
    }

    #[test]
    fn merge_parent_bends_into_existing_lane() {
        // m merges a (first parent) and b (second parent, already expected by t).
        let l = assign_lanes(&[
            n(0, "t", &["b"]),
            n(1, "m", &["a", "b"]),
            n(2, "b", &["o"]),
            n(3, "a", &["o"]),
            n(4, "o", &[]),
        ]);
        assert_eq!(lane(&l, "t"), 0);
        assert_eq!(lane(&l, "m"), 1);
        let merge = l.edges.iter().find(|e| e.merge).unwrap();
        assert_eq!(merge.from_lane, 1);
        assert_eq!(merge.lane, 0); // bends left right below m
        assert_eq!(merge.to_row, Some(2));
        assert_eq!(lane(&l, "b"), 0);
    }

    #[test]
    fn merge_parent_gets_new_lane_when_unexpected() {
        let l = assign_lanes(&[n(0, "m", &["a", "b"]), n(1, "b", &[]), n(2, "a", &[])]);
        let merge = l.edges.iter().find(|e| e.merge).unwrap();
        assert_eq!((merge.from_lane, merge.lane), (0, 1));
        assert_eq!(lane(&l, "b"), 1);
        assert_eq!(lane(&l, "a"), 0);
    }

    #[test]
    fn parents_outside_window_stay_open() {
        let l = assign_lanes(&[n(0, "b", &["a"])]);
        assert_eq!(l.edges.len(), 1);
        assert_eq!(l.edges[0].to_row, None);
        assert_eq!(l.edges[0].to_lane, 0);
    }

    #[test]
    fn sparse_rows_and_unsorted_input() {
        // The other graph owns rows 1 and 3; input order does not matter.
        let l = assign_lanes(&[n(4, "a", &[]), n(0, "c", &["b"]), n(2, "b", &["a"])]);
        assert_eq!(l.lanes, 1);
        let e: Vec<(usize, Option<usize>)> =
            l.edges.iter().map(|e| (e.from_row, e.to_row)).collect();
        assert_eq!(e, vec![(0, Some(2)), (2, Some(4))]);
    }

    #[test]
    fn three_tips_on_one_base() {
        let l = assign_lanes(&[
            n(0, "t1", &["base"]),
            n(1, "t2", &["base"]),
            n(2, "t3", &["base"]),
            n(3, "base", &[]),
        ]);
        assert_eq!(l.lanes, 3);
        assert_eq!(lane(&l, "base"), 0);
        let into_base: Vec<usize> = l
            .edges
            .iter()
            .filter(|e| e.to_row == Some(3))
            .map(|e| e.lane)
            .collect();
        assert_eq!(into_base, vec![0, 1, 2]);
        assert!(l.edges.iter().all(|e| e.to_lane == 0));
    }
}
