//! Sidebar section visibility: fold persisted nav indices onto visible sections.

use melodia_app::services::settings::NavFlags;

/// When `idx` points at a hidden section, land on Search instead.
#[must_use]
pub fn fold_hidden_nav_index(idx: i32, nav: &NavFlags) -> i32 {
    if nav.section_visible(idx) {
        idx
    } else {
        0
    }
}

/// If the user is on a section that was just hidden, move to Search.
pub fn redirect_if_hidden(current: i32, nav: &NavFlags) -> i32 {
    fold_hidden_nav_index(current, nav)
}

#[cfg(test)]
mod tests {
    use super::*;
    use melodia_app::services::settings::NavFlags;

    #[test]
    fn hidden_favorites_folds_to_search() {
        let nav = NavFlags { hide_favorites: true, ..NavFlags::default() };
        assert_eq!(fold_hidden_nav_index(2, &nav), 0);
        assert_eq!(fold_hidden_nav_index(0, &nav), 0);
    }
}
