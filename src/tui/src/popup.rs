//! Bottom-up command popup shared by `/status` and `/agent`: a drill-down menu
//! anchored above the input that grows upward and sizes to its content.
//!
//! Navigation is a small stack — categories → items → (status) detail — so the
//! same visual and key handling serve both, while the action taken on an item
//! differs per kind (status inspects, agent selects context).

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupKind {
    Status,
    Agent,
}

impl PopupKind {
    pub(crate) fn title(self) -> &'static str {
        match self {
            Self::Status => "runtime status",
            Self::Agent => "agent config",
        }
    }

    /// The fixed top-level categories, in display order.
    pub(crate) fn categories(self) -> &'static [&'static str] {
        match self {
            Self::Status => &["channels", "tunnels", "agents", "sessions"],
            Self::Agent => &["agents", "profiles", "workspaces", "sessions", "close"],
        }
    }

    pub(crate) fn is_close_category(self, index: usize) -> bool {
        matches!(self, Self::Agent) && self.categories().get(index) == Some(&"close")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum PopupLevel {
    Categories,
    Items { category: usize },
    Detail { category: usize, item: usize },
}

#[derive(Debug, Clone)]
pub(crate) struct Popup {
    pub(crate) kind: PopupKind,
    pub(crate) level: PopupLevel,
    pub(crate) cursor: usize,
}

impl Popup {
    pub(crate) fn new(kind: PopupKind) -> Self {
        Self {
            kind,
            level: PopupLevel::Categories,
            cursor: 0,
        }
    }

    pub(crate) fn move_up(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor == 0 {
            self.cursor = len - 1;
        } else {
            self.cursor -= 1;
        }
    }

    pub(crate) fn move_down(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
        } else {
            self.cursor = (self.cursor + 1) % len;
        }
    }

    pub(crate) fn clamp(&mut self, len: usize) {
        if len == 0 {
            self.cursor = 0;
        } else if self.cursor >= len {
            self.cursor = len - 1;
        }
    }

    /// Drill from the category list into the selected category's items.
    pub(crate) fn open_category(&mut self) {
        if matches!(self.level, PopupLevel::Categories) {
            self.level = PopupLevel::Items {
                category: self.cursor,
            };
            self.cursor = 0;
        }
    }

    /// Drill from the item list into the selected item's detail.
    pub(crate) fn open_detail(&mut self) {
        if let PopupLevel::Items { category } = self.level {
            self.level = PopupLevel::Detail {
                category,
                item: self.cursor,
            };
        }
    }

    /// Step back one level, restoring the cursor to where the user drilled in.
    /// Returns `true` when already at the top, i.e. the popup should close.
    pub(crate) fn back(&mut self) -> bool {
        match self.level {
            PopupLevel::Categories => true,
            PopupLevel::Items { category } => {
                self.level = PopupLevel::Categories;
                self.cursor = category;
                false
            }
            PopupLevel::Detail { category, item } => {
                self.level = PopupLevel::Items { category };
                self.cursor = item;
                false
            }
        }
    }

    /// The category being browsed, if past the top level.
    pub(crate) fn category(&self) -> Option<usize> {
        match self.level {
            PopupLevel::Categories => None,
            PopupLevel::Items { category } | PopupLevel::Detail { category, .. } => Some(category),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drill_in_and_back_restores_cursor() {
        let mut popup = Popup::new(PopupKind::Status);
        popup.move_down(4); // category 1 (tunnels)
        assert_eq!(popup.cursor, 1);
        popup.open_category();
        assert_eq!(popup.level, PopupLevel::Items { category: 1 });
        assert_eq!(popup.cursor, 0);

        popup.move_down(3); // item 1
        popup.open_detail();
        assert_eq!(
            popup.level,
            PopupLevel::Detail {
                category: 1,
                item: 1
            }
        );

        assert!(!popup.back());
        assert_eq!(popup.level, PopupLevel::Items { category: 1 });
        assert_eq!(popup.cursor, 1, "cursor returns to the item");
        assert!(!popup.back());
        assert_eq!(popup.level, PopupLevel::Categories);
        assert_eq!(popup.cursor, 1, "cursor returns to the category");
        assert!(popup.back(), "top level closes");
    }

    #[test]
    fn navigation_wraps_and_clamps() {
        let mut popup = Popup::new(PopupKind::Agent);
        popup.move_up(4);
        assert_eq!(popup.cursor, 3);
        popup.move_down(4);
        assert_eq!(popup.cursor, 0);
        popup.cursor = 9;
        popup.clamp(3);
        assert_eq!(popup.cursor, 2);
        popup.clamp(0);
        assert_eq!(popup.cursor, 0);
    }
}
