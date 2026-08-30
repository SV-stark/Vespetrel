//! Tasks / VTODO view model §7 Phase 3 Extension
use chrono::{DateTime, Utc};
use vespetrel_core::TaskItem;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskFilter {
    All,
    Pending,
    Completed,
    Overdue,
}

#[derive(Debug, Clone)]
pub struct TaskListView {
    pub tasks: Vec<TaskItem>,
    pub filter: TaskFilter,
    pub search_query: String,
    pub selected_task_id: Option<String>,
}

impl TaskListView {
    pub fn new(tasks: Vec<TaskItem>) -> Self {
        Self {
            tasks,
            filter: TaskFilter::All,
            search_query: String::new(),
            selected_task_id: None,
        }
    }

    pub fn set_filter(&mut self, filter: TaskFilter) {
        self.filter = filter;
    }

    pub fn set_search(&mut self, query: impl Into<String>) {
        self.search_query = query.into().trim().to_string();
    }

    pub fn filtered_tasks(&self) -> Vec<&TaskItem> {
        let now = Utc::now();
        let q_bytes = self.search_query.as_bytes();

        self.tasks
            .iter()
            .filter(|t| {
                // Filter by completion/status
                let status_match = match self.filter {
                    TaskFilter::All => true,
                    TaskFilter::Pending => !t.is_completed,
                    TaskFilter::Completed => t.is_completed,
                    TaskFilter::Overdue => {
                        !t.is_completed && t.due_at.map(|d| d < now).unwrap_or(false)
                    }
                };
                if !status_match {
                    return false;
                }

                // Filter by search query with zero-allocation ASCII comparison
                if !q_bytes.is_empty() {
                    let title_match = crate::views::message_list::contains_ignore_case_ascii(
                        t.title.as_bytes(),
                        q_bytes,
                    );
                    let desc_match = t
                        .description
                        .as_deref()
                        .map(|d| {
                            crate::views::message_list::contains_ignore_case_ascii(
                                d.as_bytes(),
                                q_bytes,
                            )
                        })
                        .unwrap_or(false);
                    title_match || desc_match
                } else {
                    true
                }
            })
            .collect()
    }

    pub fn add_task(
        &mut self,
        calendar_id: impl Into<String>,
        title: impl Into<String>,
    ) -> &TaskItem {
        let task = TaskItem::new(calendar_id, title);
        self.tasks.push(task);
        self.tasks.last().unwrap()
    }

    pub fn toggle_completion(&mut self, id: &str) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.is_completed = !t.is_completed;
            t.completed_at = if t.is_completed {
                Some(Utc::now())
            } else {
                None
            };
        }
    }

    pub fn delete_task(&mut self, id: &str) {
        self.tasks.retain(|t| t.id != id);
        if self.selected_task_id.as_deref() == Some(id) {
            self.selected_task_id = None;
        }
    }

    pub fn set_due_date(&mut self, id: &str, due: Option<DateTime<Utc>>) {
        if let Some(t) = self.tasks.iter_mut().find(|t| t.id == id) {
            t.due_at = due;
        }
    }
}

impl Default for TaskListView {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration;

    #[test]
    fn test_task_list_filtering_and_lifecycle() {
        let mut t1 = TaskItem::new("cal1", "Write Thunderbird migration guide");
        t1.due_at = Some(Utc::now() - Duration::hours(2)); // Overdue

        let mut t2 = TaskItem::new("cal1", "Review PR for SIMD accelerations");
        t2.due_at = Some(Utc::now() + Duration::days(1)); // Pending

        let mut t3 = TaskItem::new("cal1", "Setup OAuth PKCE");
        t3.is_completed = true;

        let mut view = TaskListView::new(vec![t1.clone(), t2.clone(), t3.clone()]);
        assert_eq!(view.filtered_tasks().len(), 3);

        view.set_filter(TaskFilter::Pending);
        assert_eq!(view.filtered_tasks().len(), 2);

        view.set_filter(TaskFilter::Completed);
        assert_eq!(view.filtered_tasks().len(), 1);
        assert_eq!(view.filtered_tasks()[0].title, "Setup OAuth PKCE");

        view.set_filter(TaskFilter::Overdue);
        assert_eq!(view.filtered_tasks().len(), 1);
        assert_eq!(
            view.filtered_tasks()[0].title,
            "Write Thunderbird migration guide"
        );

        view.set_filter(TaskFilter::All);
        view.set_search("SIMD");
        assert_eq!(view.filtered_tasks().len(), 1);
        assert_eq!(
            view.filtered_tasks()[0].title,
            "Review PR for SIMD accelerations"
        );

        view.set_search("");
        view.toggle_completion(&t1.id);
        assert!(
            view.tasks
                .iter()
                .find(|t| t.id == t1.id)
                .unwrap()
                .is_completed
        );

        view.delete_task(&t1.id);
        assert_eq!(view.tasks.len(), 2);
    }
}
