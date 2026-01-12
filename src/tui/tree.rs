// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The raw data structure (The "Truth")
#[derive(Debug, Clone, PartialEq)] 
pub struct RawNode<T> {
    pub name: String,
    pub full_path: PathBuf, // Must match the crawler output in storage.rs
    pub children: Vec<RawNode<T>>,
    pub payload: T, 
    pub is_dir: bool,
}

// ------------------------------------------------------------------
// BLOCK 1: General methods (Relaxed bounds)
// These methods work for any T that can be Cloned.
// ------------------------------------------------------------------
impl<T: Clone> RawNode<T> {
    /// Recursively finds a node by path and updates its children.
    pub fn merge_subtree(&mut self, incoming: RawNode<T>) -> bool {
        if self.full_path == incoming.full_path {
            self.children = incoming.children;
            return true;
        }

        if incoming.full_path.starts_with(&self.full_path) {
            for child in self.children.iter_mut() {
                if child.merge_subtree(incoming.clone()) {
                    return true;
                }
            }
        }
        false
    }

    pub fn expand_all(&self, state: &mut TreeViewState) {
        if self.is_dir {
            state.expanded_paths.insert(self.full_path.clone());
            for child in &self.children {
                child.expand_all(state);
            }
        }
    }

    pub fn sort_recursive(&mut self) {
        self.children.sort_by(|a, b| {
            match (a.is_dir, b.is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });
        for child in &mut self.children {
            child.sort_recursive();
        }
    }

    pub fn find_and_act<F>(&mut self, target_path: &Path, action: &mut F) -> bool
    where
        F: FnMut(&mut Self),
    {
        if self.full_path == target_path {
            action(self);
            return true;
        }
        if target_path.starts_with(&self.full_path) {
            for child in &mut self.children {
                if child.find_and_act(target_path, action) {
                    return true;
                }
            }
        }
        false
    }

    pub fn apply_recursive<F>(&mut self, action: &F)
    where
        F: Fn(&mut Self),
    {
        action(self);
        for child in &mut self.children {
            child.apply_recursive(action);
        }
    }
}

// ------------------------------------------------------------------
// BLOCK 2: Math-heavy methods (Strict bounds)
// These explicitly require T to be summable (AddAssign).
// ------------------------------------------------------------------
impl<T: Clone + Default + std::ops::AddAssign> RawNode<T> {
    pub fn from_path_list(custom_root: Option<String>, files: Vec<(Vec<String>, T)>) -> Vec<Self> {
        let mut internal_root = RawNode {
            name: String::new(),
            full_path: PathBuf::new(),
            children: Vec::new(),
            payload: T::default(),
            is_dir: true,
        };

        for (path_parts, payload) in files {
            internal_root.insert_recursive(&path_parts, payload, Path::new(""));
        }
        
        internal_root.sort_recursive();

        if let Some(root_name) = custom_root {
            let wrapper = RawNode {
                name: root_name.clone(),
                full_path: PathBuf::from(root_name),
                children: internal_root.children,
                payload: internal_root.payload, 
                is_dir: true,
            };
            vec![wrapper]
        } else {
            internal_root.children
        }
    }

    fn insert_recursive(&mut self, path_parts: &[String], payload: T, parent_path: &Path) {
        // This line is the reason we need AddAssign
        self.payload += payload.clone(); 

        if path_parts.is_empty() { return; }

        let name = &path_parts[0];
        let is_last = path_parts.len() == 1;
        let current_path = parent_path.join(name);

        let child_idx = if let Some(idx) = self.children.iter().position(|c| &c.name == name) {
            idx
        } else {
            let new_node = RawNode {
                name: name.clone(),
                full_path: current_path.clone(),
                children: Vec::new(),
                payload: T::default(),
                is_dir: !is_last,
            };
            self.children.push(new_node);
            self.children.len() - 1
        };

        if is_last {
            self.children[child_idx].payload = payload;
        } else {
            self.children[child_idx].insert_recursive(&path_parts[1..], payload, &current_path);
        }
    }
}

impl RawNode<crate::app::TorrentPreviewPayload> {
    /// Recursively collects all file indices and their associated priorities.
    /// This is used when confirming a download to pass the user's selection to the engine.
    pub fn collect_priorities(&self, out: &mut std::collections::HashMap<usize, crate::app::FilePriority>) {
        // If this node is a file (has an index), record its priority
        if let Some(idx) = self.payload.file_index {
            out.insert(idx, self.payload.priority);
        }

        // Recurse through all children
        for child in &self.children {
            child.collect_priorities(out);
        }
    }
}

#[derive(Clone)]
pub struct TreeFilter<T> {
    pub queries: Vec<String>,
    pub node_rule: Option<Rc<dyn Fn(&RawNode<T>) -> bool>>,
}

impl<T> Default for TreeFilter<T> {
    fn default() -> Self {
        Self { 
            queries: Vec::new(), 
            node_rule: None,
        }
    }
}

impl<T> TreeFilter<T> {
    pub fn from_text(input: &str) -> Self {
        let queries = input
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        Self { queries, node_rule: None }
    }

    pub fn new(input: &str, rule: impl Fn(&RawNode<T>) -> bool + 'static) -> Self {
        let mut filter = Self::from_text(input);
        filter.node_rule = Some(Rc::new(rule));
        filter
    }

    pub fn matches(&self, node: &RawNode<T>) -> bool {
        if let Some(rule) = &self.node_rule {
            if !(rule)(node) { return false; }
        }
        if self.queries.is_empty() { return true; }
        let name_lower = node.name.to_lowercase();
        self.queries.iter().all(|q| name_lower.contains(q))
    }

    pub fn any_matches(&self, node: &RawNode<T>) -> bool {
        if self.matches(node) { return true; }
        node.children.iter().any(|child| self.any_matches(child))
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct TreeViewState {
    pub cursor_path: Option<PathBuf>,
    pub current_path: PathBuf,
    pub expanded_paths: HashSet<PathBuf>,
    pub selected_paths: HashSet<PathBuf>,
    pub top_most_offset: usize,
}

impl TreeViewState {
    pub fn new() -> Self { Self::default() }

    pub fn rename_path(&mut self, old_path: &Path, new_path: &Path) {
        let rewrite = |path: &PathBuf| -> Option<PathBuf> {
            if let Ok(suffix) = path.strip_prefix(old_path) {
                Some(new_path.join(suffix))
            } else {
                None
            }
        };

        let expanded_replacements: Vec<_> = self.expanded_paths.iter()
            .filter_map(|p| rewrite(p).map(|new| (p.clone(), new)))
            .collect();
        for (old, new) in expanded_replacements {
            self.expanded_paths.remove(&old);
            self.expanded_paths.insert(new);
        }

        let selected_replacements: Vec<_> = self.selected_paths.iter()
            .filter_map(|p| rewrite(p).map(|new| (p.clone(), new)))
            .collect();
        for (old, new) in selected_replacements {
            self.selected_paths.remove(&old);
            self.selected_paths.insert(new);
        }

        if let Some(cursor) = &self.cursor_path {
            if let Some(new_cursor) = rewrite(cursor) {
                self.cursor_path = Some(new_cursor);
            }
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct RenderItem<'a, T> {
    pub node: &'a RawNode<T>,
    pub path: PathBuf,
    pub depth: usize,
    pub is_last: bool,
    pub is_expanded: bool,
    pub is_selected: bool,
    pub is_cursor: bool,
}

impl<'a, T> Clone for RenderItem<'a, T> {
    fn clone(&self) -> Self {
        Self {
            node: self.node,
            path: self.path.clone(),
            depth: self.depth,
            is_last: self.is_last,
            is_expanded: self.is_expanded,
            is_selected: self.is_selected,
            is_cursor: self.is_cursor,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TreeAction { Up, Down, Left, Right, Toggle, Select }

pub struct TreeMathHelper;

impl TreeMathHelper {
    pub fn get_visible_slice<'a, T>(
        nodes: &'a [RawNode<T>],
        state: &TreeViewState,
        filter: TreeFilter<T>,
        max_height: usize,
    ) -> Vec<RenderItem<'a, T>> {
        let mut full_list = Vec::new();
        Self::project_recursive(nodes, state, &filter, 0, &mut full_list);

        let start = state.top_most_offset.min(full_list.len());
        let end = (start + max_height).min(full_list.len());

        if start < end { full_list[start..end].to_vec() } else { Vec::new() }
    }

    pub fn apply_action<T>(
        state: &mut TreeViewState,
        nodes: &[RawNode<T>],
        action: TreeAction,
        filter: TreeFilter<T>,
        max_height: usize,
    ) -> bool {
        let mut full_list = Vec::new();
        Self::project_recursive(nodes, state, &filter, 0, &mut full_list);
        Self::handle_action(state, &full_list, action, max_height)
    }

    pub fn jump_to_path<T>(
        state: &mut TreeViewState,
        nodes: &[RawNode<T>],
        target_path: &Path,
        filter: TreeFilter<T>,
        max_height: usize,
    ) -> bool {
        let mut full_list = Vec::new();
        Self::project_recursive(nodes, state, &filter, 0, &mut full_list);
        if let Some(target_idx) = full_list.iter().position(|item| item.path == target_path) {
            state.cursor_path = Some(target_path.to_path_buf());
            let half = max_height / 2;
            state.top_most_offset = target_idx.saturating_sub(half);
            return true;
        }
        false
    }

    fn project_recursive<'a, T>(
        nodes: &'a [RawNode<T>],
        state: &TreeViewState,
        filter: &TreeFilter<T>, 
        depth: usize,
        output: &mut Vec<RenderItem<'a, T>>,
    ) {
        let is_searching = !filter.queries.is_empty();
        let visible_nodes: Vec<_> = nodes.iter()
            .filter(|node| filter.any_matches(node))
            .collect();

        let len = visible_nodes.len();
        for (i, node) in visible_nodes.into_iter().enumerate() {
            let path = node.full_path.clone(); 
            let expanded = if is_searching { true } else { state.expanded_paths.contains(&path) };

            output.push(RenderItem {
                node, 
                path: path.clone(), 
                depth, 
                is_last: i == len - 1,
                is_expanded: expanded, 
                is_selected: state.selected_paths.contains(&path),
                is_cursor: state.cursor_path.as_ref() == Some(&path),
            });

            if node.is_dir && expanded {
                Self::project_recursive(&node.children, state, filter, depth + 1, output);
            }
        }
    }

    fn handle_action<T>(
        state: &mut TreeViewState,
        full_list: &[RenderItem<'_, T>],
        action: TreeAction,
        max_height: usize,
    ) -> bool {
        if full_list.is_empty() { return false; }

        let current_idx = state.cursor_path.as_ref()
            .and_then(|path| full_list.iter().position(|item| &item.path == path))
            .unwrap_or(0);

        let mut new_idx = current_idx;

        match action {
            TreeAction::Up => new_idx = current_idx.saturating_sub(1),
            TreeAction::Down => if current_idx < full_list.len() - 1 { new_idx = current_idx + 1; },
            TreeAction::Right => {
                let item = &full_list[current_idx];
                if item.node.is_dir {
                    if !state.expanded_paths.contains(&item.path) {
                        state.expanded_paths.insert(item.path.clone());
                    } else if current_idx < full_list.len() - 1 {
                        let next_item = &full_list[current_idx + 1];
                        if next_item.depth > item.depth { new_idx = current_idx + 1; }
                    }
                }
            }
            TreeAction::Left => {
                let item = &full_list[current_idx];
                if item.node.is_dir && state.expanded_paths.contains(&item.path) {
                    state.expanded_paths.remove(&item.path);
                } else if item.depth > 0 {
                    let parent = full_list[0..current_idx].iter().rfind(|x| x.depth == item.depth - 1);
                    if let Some(p) = parent {
                        new_idx = full_list.iter().position(|i| i.path == p.path).unwrap_or(current_idx);
                    }
                }
            }
            TreeAction::Toggle => {
                let item = &full_list[current_idx];
                if item.node.is_dir {
                    if state.expanded_paths.contains(&item.path) { state.expanded_paths.remove(&item.path); }
                    else { state.expanded_paths.insert(item.path.clone()); }
                }
            }
            TreeAction::Select => {
                let path = &full_list[current_idx].path;
                if state.selected_paths.contains(path) { state.selected_paths.remove(path); }
                else { state.selected_paths.insert(path.clone()); }
            }
        }

        state.cursor_path = Some(full_list[new_idx].path.clone());
        let effective_height = max_height.max(1);
        if new_idx < state.top_most_offset {
            state.top_most_offset = new_idx;
        } else if new_idx >= state.top_most_offset + effective_height {
            state.top_most_offset = (new_idx + 1).saturating_sub(effective_height);
        }
        true
    }

    pub fn get_parent_path(state: &TreeViewState) -> Option<PathBuf> {
        state.cursor_path.as_ref().and_then(|path| path.parent().map(|p| p.to_path_buf()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestPayload {
        progress: f64,
    }

    fn mock_complex_tree() -> Vec<RawNode<TestPayload>> {
        vec![
            RawNode {
                name: "root1".to_string(),
                full_path: PathBuf::from("root1"),
                is_dir: true,
                payload: TestPayload { progress: 0.0 },
                children: vec![
                    RawNode {
                        name: "sub1".to_string(),
                        full_path: PathBuf::from("root1/sub1"),
                        is_dir: true,
                        payload: TestPayload { progress: 0.0 },
                        children: vec![
                            RawNode { name: "leaf1".to_string(), full_path: PathBuf::from("root1/sub1/leaf1"), is_dir: false, payload: TestPayload { progress: 1.0 }, children: vec![] },
                            RawNode { name: "leaf2".to_string(), full_path: PathBuf::from("root1/sub1/leaf2"), is_dir: false, payload: TestPayload { progress: 1.0 }, children: vec![] },
                        ],
                    },
                    RawNode { name: "leaf3".to_string(), full_path: PathBuf::from("root1/leaf3"), is_dir: false, payload: TestPayload { progress: 1.0 }, children: vec![] },
                ],
            },
            RawNode { name: "root_leaf".to_string(), full_path: PathBuf::from("root_leaf"), is_dir: false, payload: TestPayload { progress: 1.0 }, children: vec![] },
        ]
    }

    #[test]
    fn test_initial_state() {
        let tree = mock_complex_tree();
        let state = TreeViewState::default();
        let list = TreeMathHelper::get_visible_slice(&tree, &state, TreeFilter::from_text(""), 10);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_scrolling_down_triggers_offset() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("root1"));
        
        let max_height = 2;
        state.cursor_path = Some(PathBuf::from("root1"));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Down, TreeFilter::from_text(""), max_height);
        assert_eq!(state.top_most_offset, 0);

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Down, TreeFilter::from_text(""), max_height);
        assert_eq!(state.top_most_offset, 1);
    }

    #[test]
    fn test_scrolling_behavior_on_zoom_change() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("root1"));
        
        state.cursor_path = Some(PathBuf::from("root_leaf")); 
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Up, TreeFilter::from_text(""), 10); 
        assert_eq!(state.top_most_offset, 0);

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, TreeFilter::from_text(""), 2);
        assert_eq!(state.top_most_offset, 1); 
    }

    #[test]
    fn test_left_collapses_dir() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        let path = PathBuf::from("root1");
        state.expanded_paths.insert(path.clone());
        state.cursor_path = Some(path.clone());

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, TreeFilter::from_text(""), 10);
        assert!(!state.expanded_paths.contains(&path));
    }

    #[test]
    fn test_search_auto_expands_and_filters() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        
        let list = TreeMathHelper::get_visible_slice(&tree, &state, TreeFilter::from_text("leaf1"), 10);
        
        assert_eq!(list.len(), 3);
        assert!(list[0].is_expanded); 
        assert!(list[1].is_expanded); 
        assert_eq!(list[2].node.name, "leaf1");
    }

    #[test]
    fn test_lazy_loading_simulation() {
        let mut tree = vec![
            RawNode {
                name: "photos".to_string(),
                full_path: PathBuf::from("photos"),
                is_dir: true,
                payload: TestPayload { progress: 0.0 },
                children: vec![],
            }
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("photos"));
        
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, TreeFilter::from_text(""), 10);
        
        assert!(state.expanded_paths.contains(&PathBuf::from("photos")));
        let visible = TreeMathHelper::get_visible_slice(&tree, &state, TreeFilter::from_text(""), 10);
        assert_eq!(visible.len(), 1); 

        tree[0].children.push(RawNode {
            name: "vacation.jpg".to_string(),
            full_path: PathBuf::from("photos/vacation.jpg"),
            is_dir: false,
            payload: TestPayload { progress: 1.0 },
            children: vec![]
        });

        let visible_after_load = TreeMathHelper::get_visible_slice(&tree, &state, TreeFilter::from_text(""), 10);
        assert_eq!(visible_after_load.len(), 2);
        assert_eq!(visible_after_load[1].node.name, "vacation.jpg");
    }

    #[test]
    fn test_cursor_stability_on_item_removal() {
        let mut tree = vec![
            RawNode { name: "A".into(), full_path: PathBuf::from("A"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] },
            RawNode { name: "B".into(), full_path: PathBuf::from("B"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] },
            RawNode { name: "C".into(), full_path: PathBuf::from("C"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] },
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("B"));

        tree.remove(1);

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, TreeFilter::from_text(""), 10);
        assert_eq!(state.cursor_path, Some(PathBuf::from("A")));
    }

    #[test]
    fn test_list_reordering_preserves_cursor() {
        let mut tree = vec![
            RawNode { name: "Slow".into(), full_path: PathBuf::from("Slow"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] },
            RawNode { name: "Fast".into(), full_path: PathBuf::from("Fast"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] },
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("Fast"));

        tree.swap(0, 1);

        let visible = TreeMathHelper::get_visible_slice(&tree, &state, TreeFilter::from_text(""), 10);
        assert_eq!(visible[0].node.name, "Fast");
        assert!(visible[0].is_cursor); 
        assert!(!visible[1].is_cursor);
    }

    #[test]
    fn test_expanding_actually_empty_directory() {
        let tree = vec![
            RawNode { name: "EmptyDir".into(), full_path: PathBuf::from("EmptyDir"), is_dir: true, payload: TestPayload { progress: 0.0 }, children: vec![] }
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("EmptyDir"));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, TreeFilter::from_text(""), 10);
        assert!(state.expanded_paths.contains(&PathBuf::from("EmptyDir")));
        
        let old_cursor = state.cursor_path.clone();
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, TreeFilter::from_text(""), 10);
        assert_eq!(state.cursor_path, old_cursor);
    }

    #[test]
    fn test_smart_nav_right_descends_into_child() {
        let tree = vec![
            RawNode { 
                name: "Root".into(), full_path: PathBuf::from("Root"), is_dir: true, payload: TestPayload { progress: 0.0 }, 
                children: vec![
                    RawNode { name: "Child".into(), full_path: PathBuf::from("Root/Child"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("Root"));
        state.cursor_path = Some(PathBuf::from("Root"));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, TreeFilter::from_text(""), 10);
        assert_eq!(state.cursor_path, Some(PathBuf::from("Root/Child")));
    }

    #[test]
    fn test_smart_nav_left_jumps_to_parent() {
        let tree = vec![
            RawNode { 
                name: "Root".into(), full_path: PathBuf::from("Root"), is_dir: true, payload: TestPayload { progress: 0.0 }, 
                children: vec![
                    RawNode { name: "Child".into(), full_path: PathBuf::from("Root/Child"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("Root"));
        state.cursor_path = Some(PathBuf::from("Root/Child"));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, TreeFilter::from_text(""), 10);
        assert_eq!(state.cursor_path, Some(PathBuf::from("Root")));
        assert!(state.expanded_paths.contains(&PathBuf::from("Root")));
    }

    #[test]
    fn test_selection_persists_after_collapse() {
        let tree = vec![
            RawNode { 
                name: "Root".into(), full_path: PathBuf::from("Root"), is_dir: true, payload: TestPayload { progress: 0.0 }, 
                children: vec![
                    RawNode { name: "Child".into(), full_path: PathBuf::from("Root/Child"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        let child_path = PathBuf::from("Root/Child");
        
        state.expanded_paths.insert(PathBuf::from("Root"));
        state.selected_paths.insert(child_path.clone());
        state.cursor_path = Some(PathBuf::from("Root"));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, TreeFilter::from_text(""), 10);
        assert!(!state.expanded_paths.contains(&PathBuf::from("Root")));
        assert!(state.selected_paths.contains(&child_path));
    }

    #[test]
    fn test_jump_to_path_centers_view() {
        let nodes: Vec<RawNode<TestPayload>> = (0..20)
            .map(|i| RawNode {
                name: format!("Item {}", i),
                full_path: PathBuf::from(format!("Item {}", i)),
                is_dir: false,
                payload: TestPayload { progress: 0.0 },
                children: vec![],
            })
            .collect();
        
        let mut state = TreeViewState::default();
        let max_height = 5;

        let target_path = PathBuf::from("Item 10");
        let found = TreeMathHelper::jump_to_path(&mut state, &nodes, &target_path, TreeFilter::from_text(""), max_height);

        assert!(found);
        assert_eq!(state.cursor_path, Some(target_path));
        assert_eq!(state.top_most_offset, 8);
    }

    #[test]
    fn test_jump_to_nonexistent_path() {
        let nodes = vec![
            RawNode { name: "A".into(), full_path: PathBuf::from("A"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] }
        ];
        let mut state = TreeViewState::default();
        
        let found = TreeMathHelper::jump_to_path(
            &mut state, &nodes, &PathBuf::from("Ghost"), TreeFilter::from_text(""), 10
        );

        assert!(!found);
        assert_eq!(state.cursor_path, None);
    }

    #[test]
    fn test_navigation_and_expansion_logic_survives_rename() {
        let mut tree = vec![
            RawNode { 
                name: "docs".into(), 
                full_path: PathBuf::from("docs"),
                is_dir: true, 
                payload: TestPayload { progress: 0.0 }, 
                children: vec![
                    RawNode { name: "work".into(), full_path: PathBuf::from("docs/work"), is_dir: false, payload: TestPayload { progress: 0.0 }, children: vec![] }
                ] 
            }
        ];
        
        let mut state = TreeViewState::default();
        let old_root = PathBuf::from("docs");
        
        state.expanded_paths.insert(old_root.clone());
        state.cursor_path = Some(old_root.clone());

        tree[0].name = "files".to_string();
        tree[0].full_path = PathBuf::from("files");
        tree[0].children[0].full_path = PathBuf::from("files/work");
        
        let new_root = PathBuf::from("files");
        state.rename_path(&old_root, &new_root);

        assert_eq!(state.cursor_path, Some(new_root.clone()));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, TreeFilter::from_text(""), 10);
        let expected_child = new_root.join("work");
        assert_eq!(state.cursor_path, Some(expected_child.clone()));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, TreeFilter::from_text(""), 10);
        assert_eq!(state.cursor_path, Some(new_root.clone()));

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, TreeFilter::from_text(""), 10);
        assert!(!state.expanded_paths.contains(&new_root));
    }

    #[test]
    fn test_deep_tree_merging() {
        // 1. Create an initial "Skeleton" tree (mimicking first crawl)
        let mut tree = vec![
            RawNode {
                name: "docs".to_string(),
                full_path: PathBuf::from("/home/user/docs"),
                is_dir: true,
                payload: (), 
                children: vec![
                    RawNode {
                        name: "work".to_string(),
                        full_path: PathBuf::from("/home/user/docs/work"),
                        is_dir: true,
                        payload: (),
                        children: vec![], // Currently empty/not loaded
                    }
                ],
            },
            RawNode {
                name: "photos".to_string(),
                full_path: PathBuf::from("/home/user/photos"),
                is_dir: true,
                payload: (),
                children: vec![],
            },
        ];

        // 2. Simulate a background crawl result for the "work" subfolder
        let crawl_result = RawNode {
            name: "work".to_string(),
            full_path: PathBuf::from("/home/user/docs/work"),
            is_dir: true,
            payload: (),
            children: vec![
                RawNode {
                    name: "resume.pdf".to_string(),
                    full_path: PathBuf::from("/home/user/docs/work/resume.pdf"),
                    is_dir: false,
                    payload: (),
                    children: vec![],
                }
            ],
        };

        // 3. Perform the merge
        let mut merged = false;
        for root_node in tree.iter_mut() {
            if root_node.merge_subtree(crawl_result.clone()) {
                merged = true;
                break;
            }
        }

        // 4. Assertions
        assert!(merged, "The merge should report success");
        assert_eq!(tree.len(), 2, "The root level should still have exactly 2 nodes (docs and photos)");
        
        let docs_node = &tree[0];
        assert_eq!(docs_node.children.len(), 1, "Docs should still have 1 child (work)");
        
        let work_node = &docs_node.children[0];
        assert_eq!(work_node.children.len(), 1, "Work node should now contain the merged resume.pdf");
        assert_eq!(work_node.children[0].name, "resume.pdf");
        
        let photos_node = &tree[1];
        assert_eq!(photos_node.name, "photos", "The sibling node 'photos' should remain unaffected");
    }
}

