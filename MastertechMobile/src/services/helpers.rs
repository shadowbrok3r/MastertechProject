use database::schema::LiveTaskPayload;

/// Merge two task lists by id, preserving order: existing first, then new uniques in order.
pub fn merge_tasks(mut existing: Vec<LiveTaskPayload>, new: Vec<LiveTaskPayload>) -> Vec<LiveTaskPayload> {
    for t in new {
        if !existing.iter().any(|e| e.id == t.id) {
            existing.push(t);
        }
    }
    existing
}

/// Calculate the next start offset for pagination.
pub fn next_start(current_start: i32, page_size: i32, fetched: usize) -> i32 {
    if (fetched as i32) < page_size { current_start } else { current_start + page_size }
}

#[cfg(test)]
mod tests {
    use super::*;
    use database::schema::{LiveTaskPayload, RecordId};

    fn task_with_id(id: &str) -> LiveTaskPayload {
        LiveTaskPayload { id: RecordId::new("task", id), ..Default::default() }
    }

    #[test]
    fn merge_adds_new_uniques() {
        let a = vec![task_with_id("1"), task_with_id("2")];
        let b = vec![task_with_id("2"), task_with_id("3")];
        let merged = merge_tasks(a, b);
        assert_eq!(merged.len(), 3);
        assert!(merged.iter().any(|t| t.id == RecordId::new("task", "1")));
        assert!(merged.iter().any(|t| t.id == RecordId::new("task", "2")));
        assert!(merged.iter().any(|t| t.id == RecordId::new("task", "3")));
    }

    #[test]
    fn next_start_increments_only_on_full_page() {
        assert_eq!(next_start(0, 50, 50), 50);
        assert_eq!(next_start(50, 50, 49), 50);
    }
}
