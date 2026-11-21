//! The mouse path of the screen loop: wheel, hover, click, double-click guard and footer buttons.
//! Exports `mouse`.

use serde_json::json;

use super::{Flow, Guards, MouseStep, Screen, is_twin, log_key};
use crate::frame::RowMap;
use crate::input::{MouseEvent, synthetic_key};
use crate::ui::{Activation, Hit, Term, hit_at};

pub(super) fn mouse<S: Screen>(
    screen: &mut S,
    term: &mut dyn Term,
    m: &MouseEvent,
    map: Option<&RowMap>,
    g: &mut Guards,
) -> MouseStep<S::Output> {
    let top = term.window_top();
    if m.wheel_up() || m.wheel_down() {
        let delta = if m.wheel_up() { -1 } else { 1 };
        g.selected = None;
        if !screen.wheel(delta)
            && let Some(n) = screen.rows().filter(|n| *n > 0)
        {
            let c = screen.cursor();
            c.index = (c.index + delta).clamp(0, n - 1);
        }
        return MouseStep::Redraw(true);
    }
    let hit = hit_at(map, m.x, m.y, top);
    if m.is_move {
        // A move PAINTS, never selects.
        let btn = if let Hit::Footer { index, .. } = &hit {
            i32::try_from(*index).unwrap_or(-1)
        } else {
            -1
        };
        let (row, val) = match &hit {
            Hit::ListRow(r) => (*r, String::new()),
            Hit::Cell { row, value, .. } => (i32::try_from(*row).unwrap_or(-1), value.clone()),
            Hit::Action { value } => (-1, value.clone().unwrap_or_default()),
            // A row outside its value cells paints its label or name.
            Hit::LaunchRow { row, .. } => (i32::try_from(*row).unwrap_or(-1), String::new()),
            Hit::None | Hit::Footer { .. } => (-1, String::new()),
        };
        let c = screen.cursor();
        let changed = btn != c.hover || row != c.hover_row || val != c.hover_value;
        c.hover = btn;
        c.hover_row = row;
        c.hover_value = val;
        // Last move wins: the loop holds the frame while more input waits.
        return MouseStep::Redraw(changed);
    }
    if !m.left {
        return MouseStep::Redraw(true);
    }
    let now = term.record_time();
    if let Some(last) = term.last_activation().take()
        && m.x == last.x
        && m.y == last.y
        && (m.is_double_click || last.queued || is_twin(now, last.at))
    {
        *term.last_activation() = Some(last);
        return MouseStep::Redraw(false);
    }
    if let Hit::Footer { span, index } = &hit {
        let idx = i32::try_from(*index).unwrap_or(-1);
        if m.is_double_click || (m.y == g.footer_y && idx == g.footer_index && is_twin(now, g.footer_at)) {
            return MouseStep::Redraw(false);
        }
        g.footer_y = m.y;
        g.footer_index = idx;
        g.footer_at = now;
        g.acted = false;
        return MouseStep::Key(synthetic_key(&span.key, span.ch));
    }
    let index = screen.cursor().index;
    // A row a click selected acts on the next click on it: a list row while the cursor still
    // stands there, a launch-shaped row when its screen says it is still the selected one.
    let still = match &hit {
        Hit::ListRow(r) => *r == index,
        Hit::LaunchRow { .. } => true,
        Hit::None | Hit::Footer { .. } | Hit::Action { .. } | Hit::Cell { .. } => false,
    };
    if still && g.selected.as_ref() == Some(&hit) {
        if g.acted && g.acted_hit.as_ref() == Some(&hit) && (m.is_double_click || is_twin(now, g.acted_at)) {
            return MouseStep::Redraw(false);
        }
        *term.last_activation() = Some(Activation {
            x: m.x,
            y: m.y,
            at: now,
            queued: false,
        });
        g.settle = true;
        if let Some(flow) = screen.activate(&hit, term) {
            g.acted = true;
            g.acted_hit = Some(hit.clone());
            g.acted_at = now;
            log_key(term, screen, "click", &[("button", json!("row"))]);
            if let Flow::Done(res) = flow {
                return MouseStep::End(res);
            }
            return MouseStep::Redraw(true);
        }
        *term.last_activation() = None;
    }
    if hit == Hit::None {
        g.acted = false;
        return MouseStep::Redraw(true);
    }
    g.acted = false;
    let flow = screen.click(&hit);
    g.selected = match &hit {
        Hit::ListRow(r) => (screen.cursor().index == *r).then(|| hit.clone()),
        Hit::LaunchRow { .. } => Some(hit.clone()),
        Hit::None | Hit::Footer { .. } | Hit::Action { .. } | Hit::Cell { .. } => None,
    };
    match flow {
        Flow::Done(res) => MouseStep::End(res),
        Flow::Continue => MouseStep::Redraw(true),
    }
}
