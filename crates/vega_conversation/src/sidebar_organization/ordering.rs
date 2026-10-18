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
    threads.sort_by(|a, b| {
        if preferences.sort == SidebarTaskSort::Manual {
            match (ranks.get(a.id.as_str()), ranks.get(b.id.as_str())) {
                (Some(a), Some(b)) => a.cmp(b),
                (None, Some(_)) => std::cmp::Ordering::Less,
                (Some(_), None) => std::cmp::Ordering::Greater,
                (None, None) => b
                    .updated_at
                    .cmp(&a.updated_at)
                    .then_with(|| a.id.cmp(&b.id)),
            }
        } else {
            let time = |thread: &Thread| match preferences.sort {
                SidebarTaskSort::Created => thread.created_at,
                _ => thread.updated_at,
            };
            b.pinned
                .cmp(&a.pinned)
                .then_with(|| time(b).cmp(&time(a)))
                .then_with(|| a.id.cmp(&b.id))
        }
    });
    threads
}
