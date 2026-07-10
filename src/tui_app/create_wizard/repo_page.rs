use crate::cli::create_flow::RepoDraftEntry;

use super::CreateWizardApp;

pub fn repo_list_label(repo: &RepoDraftEntry, selected: bool, focused: bool) -> String {
    let cursor = if focused { ">" } else { " " };
    let selected = if selected { "[x]" } else { "[ ]" };
    format!("{cursor} {selected} {}", repo.display_name())
}

pub fn review_repo_label(repo: &RepoDraftEntry) -> String {
    format!("- {} -> {}", repo.display_name(), repo.target_branch)
}

impl CreateWizardApp {
    pub(super) fn toggle_current_repo(&mut self) {
        let visible = self.visible_repo_indices();
        if visible.is_empty() {
            return;
        }
        let idx = visible[self.repo_cursor.min(visible.len() - 1)];
        self.draft.repos[idx].selected = !self.draft.repos[idx].selected;
        self.clamp_active_cursor();
        self.refresh_current_step_errors();
        self.refresh_pages();
    }

    pub(super) fn move_repo_cursor(&mut self, delta: isize) {
        let len = self.visible_repo_indices().len();
        if len == 0 {
            self.repo_cursor = 0;
            return;
        }
        let len = len as isize;
        self.repo_cursor = (self.repo_cursor as isize + delta).rem_euclid(len) as usize;
    }

    pub(super) fn repo_filter_text(&self) -> String {
        self.repo_filter.text()
    }

    pub(super) fn visible_repo_indices(&self) -> Vec<usize> {
        let filter = self.repo_filter_text().trim().to_lowercase();
        self.draft
            .repos
            .iter()
            .enumerate()
            .filter_map(|(idx, repo)| {
                (filter.is_empty() || repo.name.to_lowercase().contains(&filter)).then_some(idx)
            })
            .collect()
    }

    pub(super) fn selected_repo_count(&self) -> usize {
        self.draft.repos.iter().filter(|repo| repo.selected).count()
    }

    pub(super) fn clamp_active_cursor(&mut self) {
        let visible_repo_count = self.visible_repo_indices().len();
        if visible_repo_count == 0 {
            self.repo_cursor = 0;
        } else {
            self.repo_cursor = self.repo_cursor.min(visible_repo_count - 1);
        }
        self.after_create_cursor = self.after_create_cursor.min(2);
        self.clamp_page_index();
    }

    pub(super) fn repo_window_start(visible_count: usize, cursor: usize, capacity: usize) -> usize {
        if capacity == 0 || visible_count <= capacity {
            0
        } else {
            cursor
                .saturating_sub(capacity - 1)
                .min(visible_count - capacity)
        }
    }
}
