//! What a scan produces: categories of clutter, each holding concrete items.

use serde::Serialize;
use std::path::PathBuf;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Risk {
    /// Caches & junk that regenerate on their own.
    Safe,
    /// Safe to remove, but costs a re-download or rebuild.
    Moderate,
    /// Personal files — only the human can decide.
    Review,
}

impl Risk {
    pub fn title(self) -> &'static str {
        match self {
            Risk::Safe => "SAFE TO ZAP",
            Risk::Moderate => "PROBABLY FINE",
            Risk::Review => "YOUR CALL",
        }
    }
    pub fn blurb(self) -> &'static str {
        match self {
            Risk::Safe => "caches & junk that grow back on their own",
            Risk::Moderate => "safe, but costs a re-download or rebuild",
            Risk::Review => "your files — never touched unless you pick them",
        }
    }
    pub fn tag(self) -> &'static str {
        match self {
            Risk::Safe => "SAFE",
            Risk::Moderate => "CHECK",
            Risk::Review => "YOURS",
        }
    }
    pub fn color(self) -> u8 {
        match self {
            Risk::Safe => crate::ui::GREEN,
            Risk::Moderate => crate::ui::ORANGE,
            Risk::Review => crate::ui::RED,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Action {
    /// Permanently delete each item.
    Delete,
    /// Move each item to the Trash (recoverable).
    Trash,
    /// Let the owning tool clean up after itself.
    Command {
        program: &'static str,
        args: &'static [&'static str],
    },
}

impl Action {
    pub fn verb(&self) -> String {
        match self {
            Action::Delete => "delete permanently".into(),
            Action::Trash => "move to Trash".into(),
            Action::Command { program, args } => format!("run `{} {}`", program, args.join(" ")),
        }
    }
}

/// Static description of a category (see `catalog.rs`).
#[derive(Debug)]
pub struct Meta {
    pub id: &'static str,
    pub name: &'static str,
    pub risk: Risk,
    pub action: Action,
    /// Let the user pick individual items when cleaning interactively.
    pub pick_items: bool,
    pub desc: &'static str,
    pub quip: &'static str,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Estimate {
    Exact,
    About,
    UpTo,
}

impl Estimate {
    pub fn prefix(self) -> &'static str {
        match self {
            Estimate::Exact => "",
            Estimate::About => "~",
            Estimate::UpTo => "≤",
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct Item {
    pub label: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    pub paths: Vec<PathBuf>,
    pub bytes: u64,
    /// days since this was last touched (modified / opened / added)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub age_days: Option<u64>,
    /// bytes shared through hard links (e.g. pnpm) that may not be freed
    #[serde(skip_serializing_if = "is_zero")]
    pub linked_bytes: u64,
    /// pre-checked in the interactive picker
    pub preselect: bool,
}

fn is_zero(n: &u64) -> bool {
    *n == 0
}

impl Item {
    #[cfg(test)]
    pub fn new(label: impl Into<String>, path: PathBuf, bytes: u64) -> Item {
        Item {
            label: label.into(),
            detail: None,
            paths: vec![path],
            bytes,
            age_days: None,
            linked_bytes: 0,
            preselect: true,
        }
    }
}

#[derive(Clone, Debug)]
pub struct Category {
    pub meta: &'static Meta,
    pub items: Vec<Item>,
    pub bytes: u64,
    pub estimate: Estimate,
    pub denied: u64,
    pub note: Option<String>,
}

impl Category {
    pub fn from_items(id: &str, mut items: Vec<Item>, denied: u64) -> Category {
        items.retain(|i| i.bytes > 0);
        items.sort_by_key(|i| std::cmp::Reverse(i.bytes));
        let bytes = items.iter().map(|i| i.bytes).sum();
        Category {
            meta: crate::catalog::meta(id),
            items,
            bytes,
            estimate: Estimate::Exact,
            denied,
            note: None,
        }
    }

    pub fn estimated(id: &str, bytes: u64, estimate: Estimate) -> Category {
        Category {
            meta: crate::catalog::meta(id),
            items: Vec::new(),
            bytes,
            estimate,
            denied: 0,
            note: None,
        }
    }

    pub fn with_note(mut self, note: impl Into<String>) -> Category {
        self.note = Some(note.into());
        self
    }

    pub fn recompute(&mut self) {
        self.items.retain(|i| !i.paths.is_empty() && i.bytes > 0);
        if self.estimate == Estimate::Exact {
            self.bytes = self.items.iter().map(|i| i.bytes).sum();
        }
    }

    pub fn is_empty(&self) -> bool {
        self.bytes == 0
    }

    pub fn size_str(&self) -> String {
        format!("{}{}", self.estimate.prefix(), crate::ui::size(self.bytes))
    }

    pub fn linked_bytes(&self) -> u64 {
        self.items.iter().map(|i| i.linked_bytes).sum()
    }
}

/// Serializable view for `--json`.
#[derive(Serialize)]
pub struct CategoryJson<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub risk: Risk,
    pub action: String,
    pub bytes: u64,
    pub estimate: Estimate,
    pub denied: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<&'a str>,
    pub items: &'a [Item],
}

impl<'a> From<&'a Category> for CategoryJson<'a> {
    fn from(c: &'a Category) -> Self {
        CategoryJson {
            id: c.meta.id,
            name: c.meta.name,
            risk: c.meta.risk,
            action: c.meta.action.verb(),
            bytes: c.bytes,
            estimate: c.estimate,
            denied: c.denied,
            note: c.note.as_deref(),
            items: &c.items,
        }
    }
}
