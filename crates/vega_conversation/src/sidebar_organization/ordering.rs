use super::*;
use std::collections::HashMap;

pub fn sorted_threads(threads: &[Thread], preferences: &SidebarPreferences) -> Vec<Thread> {
    let ranks: HashMap<_, _> = preferences
        .manual_thread_order
        .iter()
        .enumerate()
        .map(|(rank, id)| (id.as_str(), rank))
        .collect();
    let mut threads = threads.to_vec();
    threads.sort_by(
        |a, b| match (ranks.get(a.id.as_str()), ranks.get(b.id.as_str())) {
            (Some(a), Some(b)) => a.cmp(b),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (None, None) => b
                .created_at
                .cmp(&a.created_at)
                .then_with(|| a.id.cmp(&b.id)),
        },
    );
    threads
}
