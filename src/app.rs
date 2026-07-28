use crate::git::{FileChange, Snapshot, Status};
use std::collections::HashMap;
use std::time::Instant;

/// How long a changed row keeps its fading green highlight.
const FLASH: std::time::Duration = std::time::Duration::from_millis(800);

pub struct Row {
    pub file: FileChange,
    pub changed_at: Instant,
}

impl Row {
    /// Returns 1.0 right after a change, linearly fading to 0.0 when the highlight ends.
    pub fn flash_strength(&self, now: Instant) -> f32 {
        let elapsed = now.duration_since(self.changed_at);
        if elapsed >= FLASH {
            return 0.0;
        }
        1.0 - elapsed.as_secs_f32() / FLASH.as_secs_f32()
    }
}

pub struct App {
    pub branch: Option<String>,
    pub rows: Vec<Row>,
    pub last_change: Option<Instant>,
    fingerprints: HashMap<String, (Status, Option<u32>, Option<u32>)>,
}

impl App {
    pub fn new() -> Self {
        App {
            branch: None,
            rows: Vec::new(),
            last_change: None,
            fingerprints: HashMap::new(),
        }
    }

    /// Merge a fresh snapshot. Returns true if anything visibly changed.
    pub fn apply(&mut self, snap: Snapshot, now: Instant) -> bool {
        self.branch = snap.branch;

        let mut next_fp = HashMap::new();
        let mut existing: HashMap<String, Instant> =
            self.rows.iter().map(|r| (r.file.path.clone(), r.changed_at)).collect();

        let mut changed = false;
        let mut rows = Vec::with_capacity(snap.files.len());

        for file in snap.files {
            let fp = (file.status, file.insertions, file.deletions);
            let was = self.fingerprints.get(&file.path);
            let is_new_or_changed = was != Some(&fp);
            if is_new_or_changed {
                changed = true;
            }
            let changed_at = if is_new_or_changed {
                now
            } else {
                existing.remove(&file.path).unwrap_or(now)
            };
            next_fp.insert(file.path.clone(), fp);
            rows.push(Row { file, changed_at });
        }

        // Files that disappeared (reverted / committed) count as a change.
        if !existing.is_empty() {
            changed = true;
        }

        // Newest changes float to the top.
        rows.sort_by(|a, b| b.changed_at.cmp(&a.changed_at).then(a.file.path.cmp(&b.file.path)));

        self.rows = rows;
        self.fingerprints = next_fp;
        if changed {
            self.last_change = Some(now);
        }
        changed
    }

    pub fn count(&self, status: Status) -> usize {
        self.rows.iter().filter(|r| r.file.status == status).count()
    }
}
