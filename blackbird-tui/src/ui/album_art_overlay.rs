use ratatui::{
    Frame,
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Borders, Clear, Paragraph},
};

use crate::app::App;

use super::StyleExt;

/// Draws the album art overlay centered on the screen.
/// The overlay is 90% of the window width and shows a large quantised
/// view of the cover art using half-block characters.
pub fn draw(frame: &mut Frame, app: &mut App, size: Rect) {
    let Some(ref overlay) = app.album_art_overlay else {
        return;
    };
    let cover_art_id = overlay.cover_art_id.clone();
    let title_text = format!(" {} ", overlay.title);

    let background_color = app.config.style.background_color();
    let text_color = app.config.style.text_color();

    let overlay_rect = super::layout::overlay_rect(size);

    // Art area inside border: subtract 2 for left/right border.
    let art_cols = (overlay_rect.width - 2) as usize;
    // Recompute actual art rows based on available height.
    let actual_art_term_rows =
        (overlay_rect.height - super::layout::OVERLAY_BORDER_OVERHEAD) as usize;
    let actual_art_pixel_rows = actual_art_term_rows * 2;

    // Clear the area behind the overlay.
    frame.render_widget(Clear, overlay_rect);

    // Draw border.
    let border_block = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Rounded)
        .border_style(Style::default().fg(text_color))
        .style(Style::default().bg(background_color));
    frame.render_widget(border_block, overlay_rect);

    // Draw title bar with X button.
    // Title on left, X on right, inside the top border row.
    let title_y = overlay_rect.y;
    let title_area = Rect::new(
        overlay_rect.x + 2,
        title_y,
        overlay_rect.width - super::layout::OVERLAY_X_BUTTON_OFFSET,
        1,
    );
    let title_line = Line::from(vec![Span::styled(
        &title_text,
        Style::default().fg(text_color).add_modifier(Modifier::BOLD),
    )]);
    frame.render_widget(Paragraph::new(title_line), title_area);

    // X button at top-right corner of border.
    let x_button_area = Rect::new(
        overlay_rect.x + overlay_rect.width - super::layout::OVERLAY_X_BUTTON_OFFSET,
        title_y,
        3,
        1,
    );
    let x_button = Line::from(vec![Span::styled(
        "[X]",
        Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    )]);
    frame.render_widget(Paragraph::new(x_button), x_button_area);

    // Compute art color grid.
    let (grid, loading) =
        app.cover_art_cache
            .get_art_grid(Some(&cover_art_id), art_cols, actual_art_pixel_rows);

    // Render art using half-block characters inside the border.
    let art_x = overlay_rect.x + 1; // inside left border
    let art_y = overlay_rect.y + 1; // below top border

    for term_row in 0..actual_art_term_rows {
        let color_row_top = term_row * 2;
        let color_row_bot = color_row_top + 1;

        let mut spans = Vec::with_capacity(art_cols);
        for col in 0..art_cols {
            let fg = if color_row_top < grid.rows {
                grid.colors[color_row_top][col]
            } else {
                background_color
            };
            let bg = if color_row_bot < grid.rows {
                grid.colors[color_row_bot][col]
            } else {
                background_color
            };

            spans.push(Span::styled("\u{2580}", Style::default().fg(fg).bg(bg)));
        }

        let row_rect = Rect::new(art_x, art_y + term_row as u16, art_cols as u16, 1);
        frame.render_widget(Paragraph::new(Line::from(spans)), row_rect);
    }

    // Show loading indicator centered over the art while high-res is computing.
    if loading {
        let label = " Loading\u{2026} ";
        let label_len = label.len() as u16;
        if label_len < overlay_rect.width - 2 {
            let label_x = overlay_rect.x + (overlay_rect.width - label_len) / 2;
            let label_y = overlay_rect.y + overlay_rect.height / 2;
            let label_rect = Rect::new(label_x, label_y, label_len, 1);
            let label_widget = Paragraph::new(Line::from(Span::styled(
                label,
                Style::default()
                    .fg(text_color)
                    .bg(background_color)
                    .add_modifier(Modifier::BOLD),
            )));
            frame.render_widget(label_widget, label_rect);
        }
    }
}

/// Returns the overlay rect for hit testing, or None if no overlay is shown.
pub fn overlay_rect(app: &App, size: Rect) -> Option<Rect> {
    app.album_art_overlay.as_ref()?;
    Some(super::layout::overlay_rect(size))
}

/// Returns true if the given coordinates hit the X button of the overlay.
pub fn is_x_button_click(app: &App, size: Rect, x: u16, y: u16) -> bool {
    if let Some(rect) = overlay_rect(app, size) {
        // X button is at top-right: [X] occupying 3 chars at (rect.x + rect.width - OVERLAY_X_BUTTON_OFFSET, rect.y)
        let x_start = rect.x + rect.width - super::layout::OVERLAY_X_BUTTON_OFFSET;
        let x_end = x_start + 3;
        y == rect.y && x >= x_start && x < x_end
    } else {
        false
    }
}
