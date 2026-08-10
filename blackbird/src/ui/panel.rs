//! Shared UI behaviours for scrollable panels: mouse-hover row detection and
//! cursor-region scroll dispatch.
//!
//! All content panels (library, search, queue, logs, settings, sidebar
//! components) implement the same two interactions:
//!
//! * **Hover underline**: the row under the cursor is underlined (except when
//!   it is the keyboard-selected row). Panels compute their hovered row with
//!   [`hovered_row`], then apply `Modifier::UNDERLINED` to that row's spans.
//! * **Cursor-scrolled scrolling**: a mouse wheel scrolls the panel under the
//!   cursor, not the keyboard-focused panel. Panels call [`scroll_region_at`]
//!   to resolve the cursor position to a region, then dispatch to the
//!   appropriate handler.

use ratatui::layout::Rect;

use crate::app::App;

/// Computes the hovered content row for a scrollable list.
///
/// `results` is the inner rect of the list (the area rows are rendered into),
/// `scroll_line` is the first visible row (the scroller's `line`), and
/// `mouse` is the cursor position. Returns the row index the cursor is over,
/// or `None` when the cursor is outside the list or the position is unknown.
pub fn hovered_row(mouse: Option<(u16, u16)>, results: Rect, scroll_line: usize) -> Option<usize> {
    let (mx, my) = mouse?;
    if mx < results.x
        || mx >= results.x + results.width
        || my < results.y
        || my >= results.y + results.height
    {
        return None;
    }
    Some(scroll_line + (my - results.y) as usize)
}

/// Which region the cursor is over, for scroll dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollRegion {
    /// Over the sidebar (one of its components). Contains the component index
    /// in the sidebar order.
    Sidebar { component: usize },
    /// Over the library (the framed inner content area, or the full content
    /// when no sidebar is present).
    Library,
    /// Over no scrollable region (now-playing bar, scrub bar, help bar).
    None,
}

/// Resolves the cursor position to the scrollable region it is over.
///
/// `content` is the library's content rect (framed-inner when a sidebar is
/// present, else the full content area); `sidebar_area` is the sidebar rect
/// (if visible).
pub fn scroll_region_at(
    mouse: Option<(u16, u16)>,
    content: Rect,
    sidebar_area: Option<Rect>,
    app: &App,
) -> ScrollRegion {
    let Some((mx, my)) = mouse else {
        return ScrollRegion::None;
    };

    // Sidebar components take priority (they overlap nothing else on screen).
    if let Some(sidebar_area) = sidebar_area
        && my >= sidebar_area.y
        && my < sidebar_area.y + sidebar_area.height
        && mx >= sidebar_area.x
        && mx < sidebar_area.x + sidebar_area.width
    {
        // Which component is under the cursor?
        let component_rects = crate::ui::sidebar::layout_for(app, sidebar_area);
        for (i, rect) in component_rects.iter().enumerate() {
            if my >= rect.y && my < rect.y + rect.height {
                return ScrollRegion::Sidebar { component: i };
            }
        }
        return ScrollRegion::Sidebar { component: 0 };
    }

    if my >= content.y
        && my < content.y + content.height
        && mx >= content.x
        && mx < content.x + content.width
    {
        return ScrollRegion::Library;
    }

    ScrollRegion::None
}
