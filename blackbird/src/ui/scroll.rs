//! Shared mouse-driven scrolling: wheel, content drag (with inertia), and
//! scrollbar drag. Owners pair this with their own selection model and call
//! into it from their draw/handle functions.
//!
//! Geometry is in *lines*. Owners with single-line items can treat lines and
//! item indices interchangeably; owners with variable-height entries (the
//! library) translate `Scroller::line` into an item offset themselves.

use ratatui::{
    Frame,
    layout::{Position, Rect},
    style::{Color, Style},
};

use super::layout::{
    DRAG_VELOCITY_SMOOTHING, INERTIA_FRICTION, INERTIA_INITIAL_BOOST, INERTIA_STOP_THRESHOLD,
};

#[derive(Debug, Default)]
pub struct Scroller {
    /// First visible line.
    pub line: usize,
    /// Rows available to the list in the most recent draw. Reading this in
    /// handlers avoids threading the area through every call site.
    pub visible_height: usize,

    pub dragging: bool,
    pub drag_last_y: Option<u16>,
    pub scrollbar_dragging: bool,

    /// Smoothed velocity tracked during a content drag, used to seed inertia
    /// on release. Positive = viewport moves down.
    pub drag_velocity: f64,
    /// Active inertia velocity in lines per tick.
    pub inertia_velocity: f64,
}

/// Outcome of [`Scroller::end_drag`] — owners use this to decide whether to
/// snap their selection cursor to the new viewport position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndDragOutcome {
    /// No drag was in progress (e.g. only a click happened).
    Idle,
    /// Drag ended with enough velocity to start inertia.
    InertiaStarted,
    /// Drag ended without inertia (slow release or scrollbar drag).
    Settled,
}

/// Outcome of [`Scroller::tick_inertia`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InertiaTick {
    /// Inertia is not running.
    Inactive,
    /// Viewport line changed this tick.
    Moved,
    /// Velocity is still non-zero but didn't move pixels this tick.
    Tickless,
    /// Velocity just dropped below the stop threshold and inertia ended.
    /// Owners may snap their selection cursor in response.
    Stopped,
}

impl Scroller {
    pub fn new() -> Self {
        Self::default()
    }

    /// Greatest valid `line` for the given total content length.
    pub fn max_line(&self, total_lines: usize) -> usize {
        total_lines.saturating_sub(self.visible_height)
    }

    /// Clamp `line` into `[0, max_line(total_lines)]`.
    pub fn clamp(&mut self, total_lines: usize) {
        self.line = self.line.min(self.max_line(total_lines));
    }

    /// Apply a wheel scroll. Stops any inertia.
    pub fn apply_wheel(&mut self, direction: i32, steps: usize, total_lines: usize) {
        self.inertia_velocity = 0.0;
        let max = self.max_line(total_lines) as i32;
        let delta = direction * steps as i32;
        self.line = (self.line as i32 + delta).clamp(0, max) as usize;
    }

    /// Apply a content drag at row `y`. Updates `line` based on the delta
    /// from the previous Y and accumulates a smoothed drag velocity.
    pub fn apply_content_drag(&mut self, y: u16, total_lines: usize) {
        if !self.dragging {
            self.drag_velocity = 0.0;
        }
        self.dragging = true;
        self.inertia_velocity = 0.0;

        if let Some(last_y) = self.drag_last_y {
            let delta = y as i32 - last_y as i32;
            if delta != 0 {
                let max = self.max_line(total_lines) as i32;
                self.line = (self.line as i32 - delta).clamp(0, max) as usize;
                // Drag down (positive delta) → viewport moves up. Velocity
                // convention: positive = viewport moves down → negate.
                let raw_velocity = -delta as f64;
                self.drag_velocity = self.drag_velocity * DRAG_VELOCITY_SMOOTHING
                    + raw_velocity * (1.0 - DRAG_VELOCITY_SMOOTHING);
            }
        }
        self.drag_last_y = Some(y);
    }

    /// Jump to the position pointed at by `y` within `[area_y, area_y + area_height)`,
    /// as a fraction of `total_lines`. Marks the drag as a scrollbar drag so
    /// `end_drag` won't trigger inertia.
    pub fn apply_scrollbar_drag(
        &mut self,
        y: u16,
        total_lines: usize,
        area_y: u16,
        area_height: u16,
    ) {
        if area_height == 0 {
            return;
        }
        let inner_y = y.saturating_sub(area_y);
        let ratio = inner_y as f32 / area_height as f32;
        let max = self.max_line(total_lines);
        self.line = ((total_lines as f32 * ratio) as usize).min(max);
        self.scrollbar_dragging = true;
        self.dragging = true;
    }

    /// Finalize a drag. Either seeds inertia from the recent drag velocity or
    /// reports that the viewport has settled.
    pub fn end_drag(&mut self) -> EndDragOutcome {
        let was_dragging = self.dragging;
        let was_scrollbar = self.scrollbar_dragging;
        self.dragging = false;
        self.drag_last_y = None;
        self.scrollbar_dragging = false;

        if !was_dragging {
            self.drag_velocity = 0.0;
            return EndDragOutcome::Idle;
        }
        if was_scrollbar {
            self.drag_velocity = 0.0;
            return EndDragOutcome::Settled;
        }

        let velocity = self.drag_velocity;
        self.drag_velocity = 0.0;
        if velocity.abs() >= INERTIA_STOP_THRESHOLD {
            self.inertia_velocity = velocity * INERTIA_INITIAL_BOOST;
            EndDragOutcome::InertiaStarted
        } else {
            EndDragOutcome::Settled
        }
    }

    /// Stop any active inertia animation. Returns true if inertia was running.
    pub fn cancel_inertia(&mut self) -> bool {
        if self.inertia_velocity != 0.0 {
            self.inertia_velocity = 0.0;
            true
        } else {
            false
        }
    }

    /// Whether inertia animation is actively running. Used to bump the redraw
    /// tick rate for smooth scrolling.
    pub fn inertia_active(&self) -> bool {
        self.inertia_velocity != 0.0 && !self.dragging
    }

    /// Whether a scrollbar should be visible — content overflows the viewport.
    pub fn needs_scrollbar(&self, total_lines: usize) -> bool {
        self.visible_height > 0 && total_lines > self.visible_height
    }

    /// Returns the thumb's row range within `[0, visible_height)`, or `None`
    /// if no scrollbar is needed (content fits the viewport).
    pub fn thumb_range(&self, total_lines: usize) -> Option<(u16, u16)> {
        if !self.needs_scrollbar(total_lines) {
            return None;
        }
        let vh = self.visible_height as f32;
        let start_frac = self.line as f32 / total_lines as f32;
        let size_frac = self.visible_height as f32 / total_lines as f32;
        let start = (start_frac * vh) as u16;
        let end = ((start_frac + size_frac) * vh).ceil() as u16;
        Some((start, end))
    }

    /// Render a simple scrollbar in the rightmost column of `area`. The
    /// library has its own renderer that overlays sort-order labels.
    pub fn render_scrollbar(
        &self,
        frame: &mut Frame,
        area: Rect,
        total_lines: usize,
        track_color: Color,
        thumb_color: Color,
    ) {
        let Some((start, end)) = self.thumb_range(total_lines) else {
            return;
        };

        let buf = frame.buffer_mut();
        let x = area.x + area.width.saturating_sub(1);
        for row in 0..area.height {
            let is_thumb = row >= start && row < end;
            let (ch, color) = if is_thumb {
                ('█', thumb_color)
            } else {
                ('│', track_color)
            };
            let pos = Position::new(x, area.y + row);
            if area.contains(pos) {
                let cell = &mut buf[pos];
                cell.set_char(ch);
                cell.set_style(Style::default().fg(color));
            }
        }
    }

    /// Advance the inertia animation by one tick.
    pub fn tick_inertia(&mut self, total_lines: usize) -> InertiaTick {
        if self.dragging {
            return InertiaTick::Inactive;
        }
        if self.inertia_velocity.abs() < INERTIA_STOP_THRESHOLD {
            if self.inertia_velocity != 0.0 {
                self.inertia_velocity = 0.0;
                return InertiaTick::Stopped;
            }
            return InertiaTick::Inactive;
        }
        let max = self.max_line(total_lines);
        let old = self.line;
        let new_v = (old as f64 + self.inertia_velocity).clamp(0.0, max as f64);
        let new_int = new_v.round() as usize;
        let moved = new_int != old;
        self.line = new_int;
        self.inertia_velocity *= INERTIA_FRICTION;
        if moved {
            InertiaTick::Moved
        } else {
            InertiaTick::Tickless
        }
    }
}

/// Returns true if `x` falls within the rightmost `scrollbar_width` columns
/// of `area`. Owners use this to dispatch clicks on the scrollbar column to
/// `apply_scrollbar_drag` instead of content handling.
pub fn is_in_scrollbar_column(area: Rect, x: u16, scrollbar_width: u16) -> bool {
    let start = area.x + area.width.saturating_sub(scrollbar_width);
    x >= start
}
