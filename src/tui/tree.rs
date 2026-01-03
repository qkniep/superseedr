// SPDX-FileCopyrightText: 2025 The superseedr Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use std::collections::HashSet;
use std::path::PathBuf;
use std::path::Path;

/// The raw data structure (The "Truth")
#[derive(Debug, Clone, PartialEq)] 
pub struct RawNode<T> {
    pub name: String,
    pub children: Vec<RawNode<T>>,
    pub payload: T, 
    pub is_dir: bool,
}

/// Criteria used to prune the tree. Supports multiple terms (AND logic).
#[derive(Debug, Default, Clone)]
pub struct TreeFilter {
    pub queries: Vec<String>,
}

impl TreeFilter {
    /// Parses a raw input string into multiple search terms.
    pub fn from_input(input: &str) -> Self {
        let queries = input
            .split_whitespace()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_lowercase())
            .collect();
        Self { queries }
    }

    /// Returns true if the node name matches ALL search terms.
    pub fn matches(&self, name: &str) -> bool {
        if self.queries.is_empty() { return true; }
        let name_lower = name.to_lowercase();
        self.queries.iter().all(|q| name_lower.contains(q))
    }

    /// Returns true if this node or any of its descendants match the filter.
    pub fn any_matches<T>(&self, node: &RawNode<T>) -> bool {
        if self.matches(&node.name) { return true; }
        node.children.iter().any(|child| self.any_matches(child))
    }
}

/// The "Memory" of the tree (what is open, selected, focused)
#[derive(Debug, Clone, Default)]
pub struct TreeViewState {
    pub cursor_path: Option<PathBuf>,
    pub expanded_paths: HashSet<PathBuf>,
    pub selected_paths: HashSet<PathBuf>,
    /// The index of the first item to be displayed in the TUI window.
    pub top_most_offset: usize,
}

impl TreeViewState {
    pub fn new() -> Self { Self::default() }

    pub fn rename_path(&mut self, old_path: &Path, new_path: &Path) {
        // 1. Helper to rewrite a path prefix
        let rewrite = |path: &PathBuf| -> Option<PathBuf> {
            if let Ok(suffix) = path.strip_prefix(old_path) {
                Some(new_path.join(suffix))
            } else {
                None
            }
        };

        // 2. Migrate Expanded Paths
        let expanded_replacements: Vec<_> = self.expanded_paths.iter()
            .filter_map(|p| rewrite(p).map(|new| (p.clone(), new)))
            .collect();
        
        for (old, new) in expanded_replacements {
            self.expanded_paths.remove(&old);
            self.expanded_paths.insert(new);
        }

        // 3. Migrate Selected Paths
        let selected_replacements: Vec<_> = self.selected_paths.iter()
            .filter_map(|p| rewrite(p).map(|new| (p.clone(), new)))
            .collect();

        for (old, new) in selected_replacements {
            self.selected_paths.remove(&old);
            self.selected_paths.insert(new);
        }

        // 4. Migrate Cursor
        if let Some(cursor) = &self.cursor_path {
            if let Some(new_cursor) = rewrite(cursor) {
                self.cursor_path = Some(new_cursor);
            }
        }
    }
}

/// The "View" item (What is actually drawn to the screen)
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
    // --- PUBLIC API ---

    /// Entry point for the View layer. Returns only the items that fit in the current viewport.
    pub fn get_visible_slice<'a, T>(
        nodes: &'a [RawNode<T>],
        state: &TreeViewState,
        filter_input: &str,
        max_height: usize,
    ) -> Vec<RenderItem<'a, T>> {
        let mut full_list = Vec::new();
        let filter = TreeFilter::from_input(filter_input);
        Self::project_recursive(nodes, state, &filter, PathBuf::from(""), 0, &mut full_list);

        let start = state.top_most_offset.min(full_list.len());
        let end = (start + max_height).min(full_list.len());

        if start < end {
            full_list[start..end].to_vec()
        } else {
            Vec::new()
        }
    }

    /// Entry point for the Events layer. Handles internal projection to ensure the
    /// navigation logic always works against the full visible set.
    pub fn apply_action<T>(
        state: &mut TreeViewState,
        nodes: &[RawNode<T>],
        action: TreeAction,
        filter_input: &str,
        max_height: usize,
    ) -> bool {
        let mut full_list = Vec::new();
        let filter = TreeFilter::from_input(filter_input);
        Self::project_recursive(nodes, state, &filter, PathBuf::from(""), 0, &mut full_list);

        Self::handle_action(state, &full_list, action, max_height)
    }

    pub fn jump_to_path<T>(
        state: &mut TreeViewState,
        nodes: &[RawNode<T>],
        target_path: &Path,
        filter_input: &str,
        max_height: usize,
    ) -> bool {
        // 1. Flatten the tree to find where the item "lives" visually
        let mut full_list = Vec::new();
        let filter = TreeFilter::from_input(filter_input);
        Self::project_recursive(nodes, state, &filter, PathBuf::from(""), 0, &mut full_list);

        // 2. Find the index of the target
        if let Some(target_idx) = full_list.iter().position(|item| item.path == target_path) {
            state.cursor_path = Some(target_path.to_path_buf());

            // 3. Calculate Smart Offset (Keep it in the middle if possible)
            let effective_height = max_height.max(1);
            
            // If it's already visible, do nothing (optional preference)
            if target_idx >= state.top_most_offset && target_idx < state.top_most_offset + effective_height {
                return true;
            }

            // Otherwise, center it
            let half_height = effective_height / 2;
            state.top_most_offset = target_idx.saturating_sub(half_height);
            return true;
        }
        false
    }

    // --- PRIVATE HELPERS ---

    fn project_recursive<'a, T>(
        nodes: &'a [RawNode<T>],
        state: &TreeViewState,
        filter: &TreeFilter,
        current_path: PathBuf,
        depth: usize,
        output: &mut Vec<RenderItem<'a, T>>,
    ) {
        let is_searching = !filter.queries.is_empty();
        let visible_nodes: Vec<_> = nodes.iter().filter(|node| filter.any_matches(node)).collect();

        let len = visible_nodes.len();
        for (i, node) in visible_nodes.into_iter().enumerate() {
            let path = current_path.join(&node.name);
            let expanded = if is_searching { true } else { state.expanded_paths.contains(&path) };

            output.push(RenderItem {
                node, path: path.clone(), depth, is_last: i == len - 1,
                is_expanded: expanded, is_selected: state.selected_paths.contains(&path),
                is_cursor: state.cursor_path.as_ref() == Some(&path),
            });

            if node.is_dir && expanded {
                Self::project_recursive(&node.children, state, filter, path, depth + 1, output);
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
                    } else if current_idx < full_list.len() - 1 { new_idx = current_idx + 1; }
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

        // --- SCROLLING LOGIC ---
        let effective_height = max_height.max(1);
        if new_idx < state.top_most_offset {
            state.top_most_offset = new_idx;
        } else if new_idx >= state.top_most_offset + effective_height {
            state.top_most_offset = (new_idx + 1).saturating_sub(effective_height);
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq)]
    struct TestPayload;

    fn mock_complex_tree() -> Vec<RawNode<TestPayload>> {
        vec![
            RawNode {
                name: "root1".to_string(),
                is_dir: true,
                payload: TestPayload,
                children: vec![
                    RawNode {
                        name: "sub1".to_string(),
                        is_dir: true,
                        payload: TestPayload,
                        children: vec![
                            RawNode { name: "leaf1".to_string(), is_dir: false, payload: TestPayload, children: vec![] },
                            RawNode { name: "leaf2".to_string(), is_dir: false, payload: TestPayload, children: vec![] },
                        ],
                    },
                    RawNode { name: "leaf3".to_string(), is_dir: false, payload: TestPayload, children: vec![] },
                ],
            },
            RawNode { name: "root_leaf".to_string(), is_dir: false, payload: TestPayload, children: vec![] },
        ]
    }

    #[test]
    fn test_initial_state() {
        let tree = mock_complex_tree();
        let state = TreeViewState::default();
        let list = TreeMathHelper::get_visible_slice(&tree, &state, "", 10);
        assert_eq!(list.len(), 2);
    }

    #[test]
    fn test_scrolling_down_triggers_offset() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("root1"));
        
        // List: [root1, sub1, leaf3, root_leaf] indices [0, 1, 2, 3]
        let max_height = 2; // Viewport height
        state.cursor_path = Some(PathBuf::from("root1")); // idx 0

        // Move to index 1 (sub1) - no scroll
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Down, "", max_height);
        assert_eq!(state.top_most_offset, 0);

        // Move to index 2 (leaf3) - MUST scroll because index 2 >= 0 + 2
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Down, "", max_height);
        assert_eq!(state.top_most_offset, 1);
    }

    #[test]
    fn test_scrolling_behavior_on_zoom_change() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("root1"));
        
        // Start height 10. List len 4. Offset 0.
        state.cursor_path = Some(PathBuf::from("root_leaf")); // index 3
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Up, "", 10); // idx 2
        assert_eq!(state.top_most_offset, 0);

        // ZOOM IN: height shrinks to 2. Index 2 is now out of view (offset 0..1)
        // Next action should snap offset.
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, "", 2);
        assert_eq!(state.top_most_offset, 1); // Viewport now shows [sub1, leaf3] (indices 1, 2)
    }

    #[test]
    fn test_left_collapses_dir() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        let path = PathBuf::from("root1");
        state.expanded_paths.insert(path.clone());
        state.cursor_path = Some(path.clone());

        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, "", 10);
        assert!(!state.expanded_paths.contains(&path));
    }

    #[test]
    fn test_search_auto_expands_and_filters() {
        let tree = mock_complex_tree();
        let mut state = TreeViewState::default();
        
        // Search for "leaf1"
        let list = TreeMathHelper::get_visible_slice(&tree, &state, "leaf1", 10);
        
        // Result should include path to leaf: [root1, sub1, leaf1]
        assert_eq!(list.len(), 3);
        assert!(list[0].is_expanded); // root1 expanded
        assert!(list[1].is_expanded); // sub1 expanded
        assert_eq!(list[2].node.name, "leaf1");
    }

    #[test]
    fn test_lazy_loading_simulation() {
        // 1. Setup: A tree with a directory "photos" that is INITIALLY empty (unloaded)
        let mut tree = vec![
            RawNode {
                name: "photos".to_string(),
                is_dir: true,
                payload: TestPayload,
                children: vec![], // Empty!
            }
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("photos"));
        
        // 2. Action: User tries to Expand/Right
        // The helper attempts to expand, but since it's empty, visually nothing changes below it.
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, "", 10);
        
        // Assert: It is marked as expanded in state, but no children are visible yet
        assert!(state.expanded_paths.contains(&PathBuf::from("photos")));
        let visible = TreeMathHelper::get_visible_slice(&tree, &state, "", 10);
        assert_eq!(visible.len(), 1); // Still just "photos"

        // 3. SIMULATE LAZY LOAD: The Event Handler detects the empty expansion and fetches data
        // We modify the tree data explicitly here (simulating fs::read_dir)
        tree[0].children.push(RawNode {
            name: "vacation.jpg".to_string(),
            is_dir: false,
            payload: TestPayload,
            children: vec![]
        });

        // 4. Action: The View redraws (get_visible_slice called again)
        let visible_after_load = TreeMathHelper::get_visible_slice(&tree, &state, "", 10);

        // Assert: The tree now correctly displays the new child without needing a reset
        assert_eq!(visible_after_load.len(), 2);
        assert_eq!(visible_after_load[1].node.name, "vacation.jpg");
    }

    #[test]
    fn test_cursor_stability_on_item_removal() {
        // Setup: [Item A, Item B, Item C]
        // Cursor is on "Item B"
        let mut tree = vec![
            RawNode { name: "A".into(), is_dir: false, payload: TestPayload, children: vec![] },
            RawNode { name: "B".into(), is_dir: false, payload: TestPayload, children: vec![] },
            RawNode { name: "C".into(), is_dir: false, payload: TestPayload, children: vec![] },
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("B"));

        // ACTION: Remove "B" from the data (Simulate Deletion)
        tree.remove(1);

        // ACTION: Trigger an update. 
        // We use 'Toggle' here instead of 'Down' to verify the "Safety Reset" 
        // without adding movement logic on top of it.
        // Logic: Path "B" is gone -> Helper defaults to Index 0 ("A") -> Toggle "A" (no-op) -> Remains "A".
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, "", 10);

        // ASSERT: The cursor safely snapped to the top item "A"
        assert_eq!(state.cursor_path, Some(PathBuf::from("A")));
    }

    #[test]
    fn test_list_reordering_preserves_cursor() {
        // Setup: [Slow_Torrent, Fast_Torrent]
        // Cursor is on "Fast_Torrent" (Index 1)
        let mut tree = vec![
            RawNode { name: "Slow".into(), is_dir: false, payload: TestPayload, children: vec![] },
            RawNode { name: "Fast".into(), is_dir: false, payload: TestPayload, children: vec![] },
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("Fast"));

        // ACTION: Re-sort list by speed (Swap positions)
        // New list: [Fast_Torrent, Slow_Torrent]
        tree.swap(0, 1);

        // Render
        let visible = TreeMathHelper::get_visible_slice(&tree, &state, "", 10);

        // ASSERT: 
        // 1. "Fast" should now be at Index 0 visually.
        assert_eq!(visible[0].node.name, "Fast");
        // 2. The cursor should still be highlighting "Fast" (is_cursor == true)
        // Even though it moved from Index 1 to Index 0.
        assert!(visible[0].is_cursor); 
        assert!(!visible[1].is_cursor);
    }

    #[test]
    fn test_expanding_actually_empty_directory() {
        // Setup: A directory that is truly empty
        let tree = vec![
            RawNode { name: "EmptyDir".into(), is_dir: true, payload: TestPayload, children: vec![] }
        ];
        let mut state = TreeViewState::default();
        state.cursor_path = Some(PathBuf::from("EmptyDir"));

        // ACTION: Try to expand it (Toggle)
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Toggle, "", 10);

        // ASSERT:
        // 1. It should be marked expanded
        assert!(state.expanded_paths.contains(&PathBuf::from("EmptyDir")));
        
        // 2. Logic check: If we try to move Right (into it), what happens?
        // Since it has no children, we generally shouldn't descend or crash.
        let old_cursor = state.cursor_path.clone();
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, "", 10);
        
        // Cursor should NOT move because there is nowhere to go
        assert_eq!(state.cursor_path, old_cursor);
    }

    #[test]
    fn test_smart_nav_right_descends_into_child() {
        // Setup: Root (Expanded) -> Child
        // Cursor is on "Root"
        let tree = vec![
            RawNode { 
                name: "Root".into(), is_dir: true, payload: TestPayload, 
                children: vec![
                    RawNode { name: "Child".into(), is_dir: false, payload: TestPayload, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("Root")); // Pre-expand
        state.cursor_path = Some(PathBuf::from("Root"));

        // ACTION: Press Right on an ALREADY expanded folder
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, "", 10);

        // ASSERT: Cursor should move DOWN to the first child
        // (Standard file explorer behavior)
        assert_eq!(state.cursor_path, Some(PathBuf::from("Root").join("Child")));
    }

    #[test]
    fn test_smart_nav_left_jumps_to_parent() {
        // Setup: Root (Expanded) -> Child
        // Cursor is on "Child"
        let tree = vec![
            RawNode { 
                name: "Root".into(), is_dir: true, payload: TestPayload, 
                children: vec![
                    RawNode { name: "Child".into(), is_dir: false, payload: TestPayload, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        state.expanded_paths.insert(PathBuf::from("Root"));
        state.cursor_path = Some(PathBuf::from("Root").join("Child")); // Cursor on leaf

        // ACTION: Press Left on a child item
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, "", 10);

        // ASSERT: Cursor should jump UP to "Root"
        assert_eq!(state.cursor_path, Some(PathBuf::from("Root")));
        
        // BONUS ASSERT: The parent should NOT collapse yet (it requires a second Left press)
        assert!(state.expanded_paths.contains(&PathBuf::from("Root")));
    }

    #[test]
    fn test_selection_persists_after_collapse() {
        // Setup: Root -> Child
        // Child is SELECTED
        let tree = vec![
            RawNode { 
                name: "Root".into(), is_dir: true, payload: TestPayload, 
                children: vec![
                    RawNode { name: "Child".into(), is_dir: false, payload: TestPayload, children: vec![] }
                ] 
            }
        ];
        let mut state = TreeViewState::default();
        let child_path = PathBuf::from("Root").join("Child");
        
        state.expanded_paths.insert(PathBuf::from("Root"));
        state.selected_paths.insert(child_path.clone());
        state.cursor_path = Some(PathBuf::from("Root"));

        // ACTION: Collapse the Root folder (Press Left on Root)
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, "", 10);

        // ASSERT: Root is collapsed...
        assert!(!state.expanded_paths.contains(&PathBuf::from("Root")));
        
        // ...BUT Child is still selected in the state (memory)
        // This ensures that if the user expands it again later, the selection wasn't lost.
        assert!(state.selected_paths.contains(&child_path));
    }

    #[test]
    fn test_rename_path_migrates_state_deeply() {
        let mut state = TreeViewState::default();
        
        // SETUP: A deep hierarchy state
        // Expanded: "photos", "photos/summer"
        // Selected: "photos/summer/beach.jpg"
        // Cursor:   "photos/summer"
        let old_root = PathBuf::from("photos");
        let old_sub = old_root.join("summer");
        let old_file = old_sub.join("beach.jpg");

        state.expanded_paths.insert(old_root.clone());
        state.expanded_paths.insert(old_sub.clone());
        state.selected_paths.insert(old_file.clone());
        state.cursor_path = Some(old_sub.clone());

        // ACTION: Rename "photos" -> "images"
        state.rename_path(&old_root, &PathBuf::from("images"));

        // ASSERT: Old paths are gone
        assert!(!state.expanded_paths.contains(&old_root));
        assert!(!state.expanded_paths.contains(&old_sub));
        
        // ASSERT: New paths exist and structure is preserved
        let new_root = PathBuf::from("images");
        let new_sub = new_root.join("summer");
        let new_file = new_sub.join("beach.jpg");

        assert!(state.expanded_paths.contains(&new_root));
        assert!(state.expanded_paths.contains(&new_sub));
        assert!(state.selected_paths.contains(&new_file));
        assert_eq!(state.cursor_path, Some(new_sub));
    }

    #[test]
    fn test_rename_path_ignores_unrelated_paths() {
        let mut state = TreeViewState::default();
        
        // SETUP: "docs" is expanded, "photos" is renamed
        let docs = PathBuf::from("docs");
        state.expanded_paths.insert(docs.clone());
        state.cursor_path = Some(docs.clone());

        // ACTION: Rename "photos" -> "images" (Should affect nothing)
        state.rename_path(&PathBuf::from("photos"), &PathBuf::from("images"));

        // ASSERT: State is unchanged
        assert!(state.expanded_paths.contains(&docs));
        assert_eq!(state.cursor_path, Some(docs));
    }

    #[test]
    fn test_jump_to_path_centers_view() {
        // SETUP: A flat list of 20 items: "Item 0" to "Item 19"
        let nodes: Vec<RawNode<TestPayload>> = (0..20)
            .map(|i| RawNode {
                name: format!("Item {}", i),
                is_dir: false,
                payload: TestPayload,
                children: vec![],
            })
            .collect();
        
        let mut state = TreeViewState::default();
        let max_height = 5;

        // ACTION: Jump to "Item 10" (Target Index: 10)
        let target_path = PathBuf::from("Item 10");
        let found = TreeMathHelper::jump_to_path(&mut state, &nodes, &target_path, "", max_height);

        // ASSERT:
        assert!(found);
        assert_eq!(state.cursor_path, Some(target_path));

        // SCROLL MATH CHECK:
        // Target is 10. Height is 5. Half height is 2.
        // Expected Top Offset = 10 - 2 = 8.
        // View range should be indices [8, 9, 10, 11, 12]. 10 is centered.
        assert_eq!(state.top_most_offset, 8);
    }

    #[test]
    fn test_jump_to_path_does_not_scroll_if_already_visible() {
        // SETUP: List of 20 items. 
        // View is ALREADY scrolled to offset 5 (showing indices 5-9).
        let nodes: Vec<RawNode<TestPayload>> = (0..20)
            .map(|i| RawNode {
                name: format!("Item {}", i),
                is_dir: false,
                payload: TestPayload,
                children: vec![],
            })
            .collect();
        
        let mut state = TreeViewState::default();
        state.top_most_offset = 5; 
        let max_height = 5; // Visible range: [5, 6, 7, 8, 9]

        // ACTION: Jump to "Item 6" (Index 6)
        // This is already inside the visible range [5..10].
        let target_path = PathBuf::from("Item 6");
        TreeMathHelper::jump_to_path(&mut state, &nodes, &target_path, "", max_height);

        // ASSERT: Offset should NOT change. It shouldn't jump to center if it's already there.
        assert_eq!(state.top_most_offset, 5);
        assert_eq!(state.cursor_path, Some(target_path));
    }
    
    #[test]
    fn test_jump_to_nonexistent_path() {
        let nodes = vec![
            RawNode { name: "A".into(), is_dir: false, payload: TestPayload, children: vec![] }
        ];
        let mut state = TreeViewState::default();
        
        // ACTION: Jump to "Ghost"
        let found = TreeMathHelper::jump_to_path(
            &mut state, &nodes, &PathBuf::from("Ghost"), "", 10
        );

        // ASSERT: Should return false and change nothing
        assert!(!found);
        assert_eq!(state.cursor_path, None);
    }

    #[test]
    fn test_navigation_and_expansion_logic_survives_rename() {
        // 1. SETUP: A simple hierarchy "docs" -> "work"
        let mut tree = vec![
            RawNode { 
                name: "docs".into(), 
                is_dir: true, 
                payload: TestPayload, 
                children: vec![
                    RawNode { name: "work".into(), is_dir: false, payload: TestPayload, children: vec![] }
                ] 
            }
        ];
        
        let mut state = TreeViewState::default();
        let old_root = PathBuf::from("docs");
        
        // Initial State: "docs" is EXPANDED and cursor is on "docs"
        state.expanded_paths.insert(old_root.clone());
        state.cursor_path = Some(old_root.clone());

        // 2. ACTION: Rename "docs" -> "files"
        // CRITICAL: We must update BOTH the Data (RawNode) AND the State (TreeViewState)
        
        // Update Data
        tree[0].name = "files".to_string();
        
        // Update State using our new helper
        let new_root = PathBuf::from("files");
        state.rename_path(&old_root, &new_root);

        // Verify sanity check: Cursor should be on "files" now
        assert_eq!(state.cursor_path, Some(new_root.clone()));

        // 3. TEST BEHAVIOR: "Smart Right" (Dive In)
        // If the state migration worked, the Helper sees "files" is expanded.
        // Therefore, pressing Right should move the cursor DOWN to "work".
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Right, "", 10);

        // Assert: We successfully navigated into the renamed folder
        let expected_child = new_root.join("work");
        assert_eq!(state.cursor_path, Some(expected_child.clone()));

        // 4. TEST BEHAVIOR: "Smart Left" (Jump Up)
        // Pressing Left on the child should jump back to the new parent "files"
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, "", 10);
        assert_eq!(state.cursor_path, Some(new_root.clone()));

        // 5. TEST BEHAVIOR: Collapse
        // Pressing Left on "files" should remove it from expanded_paths
        TreeMathHelper::apply_action(&mut state, &tree, TreeAction::Left, "", 10);
        assert!(!state.expanded_paths.contains(&new_root));
    }
}
