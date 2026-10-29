use std::collections::VecDeque;

use crate::model::Project;

const MAX_STEPS: usize = 500;

/// Undo/redo over whole-project snapshots: projects are small, and snapshots
/// need no per-operation inverse logic.
pub struct History {
    committed: Project,
    undo: VecDeque<Project>,
    redo: Vec<Project>,
}

impl History {
    pub fn new(project: &Project) -> Self {
        Self {
            committed: project.clone(),
            undo: VecDeque::new(),
            redo: Vec::new(),
        }
    }

    /// Records `current` as a new step if it changed since the last commit.
    pub fn commit(&mut self, current: &Project) {
        if *current == self.committed {
            return;
        }
        let previous = std::mem::replace(&mut self.committed, current.clone());
        self.undo.push_back(previous);
        if self.undo.len() > MAX_STEPS {
            self.undo.pop_front();
        }
        self.redo.clear();
    }

    pub fn undo(&mut self, current: &mut Project) -> bool {
        self.commit(current);
        let Some(previous) = self.undo.pop_back() else {
            return false;
        };
        self.redo
            .push(std::mem::replace(&mut self.committed, previous));
        current.clone_from(&self.committed);
        true
    }

    pub fn redo(&mut self, current: &mut Project) -> bool {
        self.commit(current);
        let Some(next) = self.redo.pop() else {
            return false;
        };
        self.undo
            .push_back(std::mem::replace(&mut self.committed, next));
        current.clone_from(&self.committed);
        true
    }

    pub fn can_undo(&self) -> bool {
        !self.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.redo.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project(width: u32) -> Project {
        let mut project = Project::default();
        project.settings.width = width;
        project
    }

    #[test]
    fn unchanged_project_adds_no_step() {
        let mut history = History::new(&project(1));
        history.commit(&project(1));
        assert!(!history.can_undo());
    }

    #[test]
    fn undo_and_redo_walk_the_steps() {
        let mut current = project(1);
        let mut history = History::new(&current);
        current = project(2);
        history.commit(&current);
        current = project(3);
        history.commit(&current);

        assert!(history.undo(&mut current));
        assert_eq!(current, project(2));
        assert!(history.undo(&mut current));
        assert_eq!(current, project(1));
        assert!(!history.undo(&mut current));
        assert_eq!(current, project(1));

        assert!(history.redo(&mut current));
        assert!(history.redo(&mut current));
        assert_eq!(current, project(3));
        assert!(!history.redo(&mut current));
    }

    #[test]
    fn new_change_drops_redo() {
        let mut current = project(1);
        let mut history = History::new(&current);
        current = project(2);
        history.commit(&current);
        history.undo(&mut current);
        current = project(5);
        history.commit(&current);
        assert!(!history.can_redo());
        history.undo(&mut current);
        assert_eq!(current, project(1));
    }

    #[test]
    fn undo_commits_pending_change_first() {
        let mut current = project(1);
        let mut history = History::new(&current);
        current = project(2);
        assert!(history.undo(&mut current));
        assert_eq!(current, project(1));
        assert!(history.redo(&mut current));
        assert_eq!(current, project(2));
    }

    #[test]
    fn oldest_steps_are_dropped() {
        let mut current = project(0);
        let mut history = History::new(&current);
        for width in 1..=MAX_STEPS as u32 + 10 {
            current = project(width);
            history.commit(&current);
        }
        while history.undo(&mut current) {}
        assert_eq!(current, project(10));
    }
}
