//! Desktop GUI for the Razer Laptop Cooling Pad: live temperature, a preset picker and a
//! draggable fan-curve editor. Changes are applied through `pkexec razer-coolingpad`.
//! `cargo run` opens the window; `cargo run -- --snap gui.png` renders it headless.

#![forbid(unsafe_code)]

mod model;
mod system;

use model::{Config, Point, Preset, percent_to_rpm};
use system::Service;
use rune::tok::{self, Theme, motion, radius, space, type_};
use rune::{Align, AppSpec, Color, Cursor, Cx, Drag, El, LineCap, Phase, Shape, TextStyle, Transition, Tx, Waker, canvas, col, el, oklch, row, text};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::Duration;

/// The chart's temperature axis, °C. Wide enough for every packaged preset (45–95 °C).
const TEMP_RANGE: (f32, f32) = (20.0, 100.0);
/// Plot inset inside the chart (left, top, right, bottom): room for the axis labels.
const PLOT: [f32; 4] = [40.0, 12.0, 8.0, 28.0];
/// A curve point's dot, and the largest round hit target around it.
const DOT: f32 = 13.0;
const HIT: f32 = 30.0;
/// The temperature, set large with tight tracking and tabular figures so it doesn't jitter.
const HERO: TextStyle = TextStyle::new(52.0, 500.0).tracking(-1.5).features("'tnum' 1");
const FIGURE: TextStyle = TextStyle::new(20.0, 500.0).features("'tnum' 1");

/// What background threads send the UI thread.
enum Msg {
    /// Once a second: the hottest sensor, the daemon's state and whether the pad is plugged in.
    Live { temp: Option<f32>, service: Service, pad: bool },
    /// A pkexec action finished.
    Done(Result<String, String>),
}

struct State {
    presets: Vec<Preset>,
    /// The config file as last read; the daemon runs this.
    saved: Config,
    /// The config being edited; Apply writes it.
    draft: Config,
    /// The last edited curve that matches no preset, kept while a preset is tried so the
    /// "Custom" segment can bring it back.
    custom: Option<Config>,
    temp: Option<f32>,
    /// `None` until the first check.
    service: Option<Service>,
    pad: bool,
    busy: bool,
    notice: Option<Result<String, String>>,
    /// The point being dragged and where it was at press (restored if the drag is cancelled).
    grab: Option<(usize, Point)>,
    tx: Sender<Msg>,
    rx: Receiver<Msg>,
    waker: Option<Waker>,
}

impl State {
    fn active_preset(&self) -> Option<&Preset> {
        self.presets.iter().find(|p| p.config == self.draft)
    }

    fn dirty(&self) -> bool {
        self.draft != self.saved
    }

    /// Run a privileged action on a thread; its result lands in `poll`.
    fn start(&mut self, action: system::Action) {
        self.busy = true;
        self.notice = None;
        let (tx, waker) = (self.tx.clone(), self.waker.clone());
        std::thread::spawn(move || {
            let _ = tx.send(Msg::Done(system::run(action)));
            if let Some(wake) = waker {
                wake();
            }
        });
    }

    fn apply(&mut self) {
        let action = match self.active_preset() {
            Some(p) => system::Action::Preset(p.name.clone()),
            None => system::Action::Config(self.draft.clone()),
        };
        self.start(action);
    }
}

fn init() -> State {
    let presets = model::load_presets(&system::curve_dir());
    let (saved, notice) = match Config::load(&system::config_path()) {
        Ok(config) => (config, None),
        // The daemon falls back to its built-in curve too, so this is what it runs.
        Err(e) => (Config::default(), Some(Err(e))),
    };
    let (tx, rx) = channel();
    State {
        presets,
        draft: saved.clone(),
        saved,
        custom: None,
        temp: None,
        service: None,
        pad: false,
        busy: false,
        notice,
        grab: None,
        tx,
        rx,
        waker: None,
    }
}

/// Starts the once-a-second sensor and service reader. It re-reads the config each time so
/// it follows the daemon's sensors after an Apply.
fn setup(s: &mut State, waker: Waker) {
    let (tx, wake) = (s.tx.clone(), waker.clone());
    std::thread::spawn(move || {
        loop {
            let sensors = Config::load(&system::config_path()).ok().and_then(|c| c.sensor_paths());
            let msg = Msg::Live {
                temp: system::max_temp(&sensors.unwrap_or_else(system::thermal_zones)),
                service: system::service(),
                pad: system::pad_connected(),
            };
            if tx.send(msg).is_err() {
                break;
            }
            wake();
            std::thread::sleep(Duration::from_secs(1));
        }
    });
    s.waker = Some(waker);
}

fn poll(s: &mut State, _: f64, _: &mut Tx) -> bool {
    let mut changed = false;
    while let Ok(msg) = s.rx.try_recv() {
        changed = true;
        match msg {
            Msg::Live { temp, service, pad } => {
                s.temp = temp;
                s.service = Some(service);
                s.pad = pad;
            }
            Msg::Done(result) => {
                s.busy = false;
                // Even a failed apply may have written the file (a restart can fail after).
                match Config::load(&system::config_path()) {
                    Ok(config) => s.saved = config,
                    Err(e) => s.notice = Some(Err(e)),
                }
                s.notice.get_or_insert(result);
            }
        }
    }
    changed
}

/// Colours every part of the view shares. Only the heat ramp is coloured; the rest is ink.
#[derive(Clone, Copy)]
struct Paint {
    t: Theme,
    dark: bool,
    danger: Color,
    hair: f32,
}

impl Paint {
    /// The heat ramp: steel blue when cool, a muted neutral midway, brick red when hot.
    fn heat(&self, temp: f32) -> Color {
        // Lightness per stop, tuned so each reads on the theme's background.
        let (cool, warm, hot) = if self.dark { (0.70, 0.74, 0.66) } else { (0.58, 0.66, 0.56) };
        let stops = [
            (35.0, oklch(cool, 0.11, 245.0)),
            (62.0, oklch(warm, 0.05, 85.0)),
            (88.0, oklch(hot, 0.17, 30.0)),
        ];
        if temp <= stops[0].0 {
            return stops[0].1;
        }
        for w in stops.windows(2) {
            let ((t0, c0), (t1, c1)) = (w[0], w[1]);
            if temp <= t1 {
                return mix(c0, c1, (temp - t0) / (t1 - t0));
            }
        }
        stops[2].1
    }
}

fn mix(a: Color, b: Color, k: f32) -> Color {
    let l = |x: f32, y: f32| x + (y - x) * k;
    Color { r: l(a.r, b.r), g: l(a.g, b.g), b: l(a.b, b.b), a: l(a.a, b.a) }
}

fn view(s: &State, cx: &mut Cx<State>) -> El {
    let dark = cx.system_dark();
    let p = Paint {
        t: tok::theme(dark),
        dark,
        danger: if dark { oklch(0.7, 0.15, 28.0) } else { oklch(0.55, 0.18, 28.0) },
        hair: 1.0 / cx.env().scale,
    };
    let chart_size = cx.layout_size("chart").unwrap_or([504.0, 300.0]);
    col()
        .key("window")
        .w_full()
        .h_full()
        .fill(p.t.bg)
        .px(28.0)
        .pt(22.0)
        .pb(20.0)
        .child(top_bar(p, s))
        .child(readout(p, s).mt(26.0))
        .child(chart(p, s, chart_size).mt(18.0))
        .child(footer(p, s).mt(14.0))
}

/// Presets on the left, the daemon's state on the right.
fn top_bar(p: Paint, s: &State) -> El {
    row()
        .key("top")
        .w_full()
        .center_y()
        .justify_between()
        .child(preset_picker(p, s))
        .child(status(p, s))
}

fn status(p: Paint, s: &State) -> El {
    let (dot, label) = match (s.service, s.pad) {
        (None, _) => (p.t.text_secondary, "Checking"),
        (Some(_), false) => (p.danger, "Pad not connected"),
        (Some(Service::Active), true) => (oklch(0.7, 0.1, 150.0), "Running"),
        (Some(Service::Activating), true) => (p.t.text_secondary, "Starting"),
        (Some(Service::Stopped), true) => (p.t.text_secondary, "Stopped"),
    };
    let mut status = row()
        .key("service")
        .role("status")
        .label(label)
        .gap(space::S2)
        .center_y()
        .child(el().key("dot").size(7.0, 7.0).radius(3.5).fill(dot).a11y_hidden())
        .child(text(label).key("label").font(type_::LABEL).color(p.t.text_secondary).a11y_hidden());
    if s.service == Some(Service::Stopped) && s.pad && !s.busy {
        status = status.child(
            el().key("start")
                .role("button")
                .label("Start service")
                .focusable(true)
                .h(24.0)
                .px(space::S2)
                .center()
                .radius(radius::SM)
                .hover(p.t.hover)
                .press(p.t.press)
                .on_press(|s: &mut State| s.start(system::Action::StartService))
                .child(text("Start").key("label").font(type_::TITLE).color(p.t.text).a11y_hidden()),
        );
    }
    status
}

/// The temperature, in its heat colour, and what the fan is told to do about it.
fn readout(p: Paint, s: &State) -> El {
    let k = rune_kit::Kit::neutral(p.dark);
    let temp = s.temp.map(f32::round);
    let rpm = match (s.temp, s.service, s.pad) {
        (Some(t), Some(Service::Active), true) => Some(percent_to_rpm(s.saved.percent_at(t))),
        _ => None,
    };
    let temp_text = temp.map_or("—".into(), |t| format!("{t:.0}°"));
    let rpm_text = rpm.map_or("—".into(), |r| r.to_string());
    row()
        .key("readout")
        .items_end()
        .gap(20.0)
        .label(format!("{temp_text}C, fan {rpm_text} rpm"))
        .child(
            rune_kit::RollingText::new("temp", temp_text)
                .value(temp.unwrap_or(0.0) as f64)
                .font(HERO)
                .color(temp.map_or(p.t.text_secondary, |t| p.heat(t)))
                .build(&k)
                .a11y_hidden(),
        )
        .child(
            row()
                .key("fan")
                .items_end()
                .gap(5.0)
                // Lifts the smaller figures onto the temperature's baseline.
                .pb(10.0)
                .child(
                    rune_kit::RollingText::new("rpm", rpm_text)
                        .value(rpm.unwrap_or(0) as f64)
                        .font(FIGURE)
                        .color(p.t.text)
                        .build(&k),
                )
                .child(text("rpm").key("unit").font(type_::BODY).color(p.t.text_secondary).pb(2.0))
                .a11y_hidden(),
        )
}

/// One segment per preset, plus "Custom" while there's an edited curve to come back to.
fn preset_picker(p: Paint, s: &State) -> El {
    let (seg, h, pad) = (102.0, 28.0, 2.0);
    let active = s.presets.iter().position(|pr| pr.config == s.draft);
    let mut names: Vec<String> = s.presets.iter().map(|pr| title_case(&pr.name)).collect();
    if active.is_none() || s.custom.is_some() {
        names.push("Custom".into());
    }
    let idx = active.unwrap_or(names.len() - 1);
    let mut segs = row().key("segments");
    for (i, name) in names.iter().enumerate() {
        let selected = i == idx;
        let mut segment = el()
            .key(name.as_str())
            .role("radio")
            .label(name.as_str())
            .checked(selected)
            .size(seg, h - 2.0 * pad)
            .center()
            .radius(radius::SM)
            .child(
                text(name.as_str())
                    .key("label")
                    .font(type_::LABEL)
                    .color(if selected { p.t.text } else { p.t.text_secondary })
                    .a11y_hidden(),
            );
        if !selected {
            segment = segment.focusable(true).hover(p.t.hover).on_press(move |s: &mut State| {
                s.notice = None;
                if i == s.presets.len() {
                    s.draft = s.custom.clone().unwrap_or_else(|| s.draft.clone());
                    return;
                }
                if s.active_preset().is_none() {
                    s.custom = Some(s.draft.clone());
                }
                s.draft = s.presets[i].config.clone();
            });
        }
        segs = segs.child(segment);
    }
    el().key("presets")
        .role("radiogroup")
        .label("Preset")
        .size(seg * names.len() as f32 + 2.0 * pad, h)
        .padding(pad)
        .radius(radius::MD)
        .fill(p.t.control_track)
        .motion(motion::SNAPPY)
        .child(
            el().key("indicator")
                .size(seg, h - 2.0 * pad)
                .absolute(pad + idx as f32 * seg, pad)
                .radius(radius::SM)
                .fill(p.t.indicator)
                .shadow(1.0, 2.0, p.t.shadow_soft)
                .lead(motion::LEAD, motion::DEFAULT)
                .a11y_hidden(),
        )
        .child(segs)
}

/// The curve, shaded along the heat ramp, with the current temperature on it and a
/// draggable handle per point.
fn chart(p: Paint, s: &State, [w, h]: [f32; 2]) -> El {
    let (t0, t1) = TEMP_RANGE;
    let [x0, y0] = [PLOT[0], PLOT[1]];
    let [x1, y1] = [(w - PLOT[2]).max(x0 + 1.0), (h - PLOT[3]).max(y0 + 1.0)];
    let fx = move |t: f32| x0 + (t.clamp(t0, t1) - t0) / (t1 - t0) * (x1 - x0);
    let fy = move |pct: f32| y1 - pct / 100.0 * (y1 - y0);
    let draft = &s.draft;

    let mut shape = Shape::new();
    for pct in [50, 100] {
        shape = shape.line([x0, fy(pct as f32)], [x1, fy(pct as f32)], p.hair, LineCap::Butt, p.t.separator);
    }
    shape = shape.line([x0, y1], [x1, y1], p.hair, LineCap::Butt, p.t.text_secondary.alpha(0.5));
    // Every whole degree plus the curve's own corners, so each slice gets its own heat colour
    // and the corners stay sharp.
    let mut temps: Vec<f32> = (t0 as i32..=t1 as i32).map(|t| t as f32).collect();
    temps.extend(draft.curve.iter().map(|pt| pt.temp.clamp(t0, t1)));
    temps.sort_by(f32::total_cmp);
    temps.dedup();
    for pair in temps.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        let c = p.heat((a + b) / 2.0);
        let (ya, yb) = (fy(draft.percent_at(a)), fy(draft.percent_at(b)));
        shape = shape
            .fill(vec![[fx(a), ya], [fx(b), yb], [fx(b), y1], [fx(a), y1]], c.alpha(if p.dark { 0.16 } else { 0.14 }))
            .line([fx(a), ya], [fx(b), yb], 2.5, LineCap::Round, c);
    }

    let mut chart = el().key("chart").w_full().grow(1.0).min_h(200.0)
        .child(canvas(shape).key("plot").absolute(0.0, 0.0).size(w, h).a11y_hidden());
    for t in (20..=100).step_by(20) {
        let label = text(format!("{t}°")).font(type_::LABEL).color(p.t.text_secondary).w(40.0).align(Align::Center);
        chart = chart.child(label.key(format!("x{t}")).absolute(fx(t as f32) - 20.0, y1 + 8.0).a11y_hidden());
    }
    for pct in [0, 50, 100] {
        let label = text(format!("{pct}%")).font(type_::LABEL).color(p.t.text_secondary).w(x0 - 10.0).align(Align::End);
        chart = chart.child(label.key(format!("y{pct}")).absolute(0.0, fy(pct as f32) - 8.0).a11y_hidden());
    }
    if let Some(t) = s.temp {
        let (x, y, c) = (fx(t), fy(draft.percent_at(t)), p.heat(t));
        chart = chart
            .child(el().key("now-line").absolute(x - 0.5, y).size(1.0, (y1 - y).max(0.0)).fill(c.alpha(0.5)).a11y_hidden())
            .child(el().key("now-halo").size(22.0, 22.0).radius(11.0).absolute(x - 11.0, y - 11.0).fill(c.alpha(0.22)).a11y_hidden())
            .child(el().key("now-dot").size(9.0, 9.0).radius(4.5).absolute(x - 4.5, y - 4.5).fill(c).a11y_hidden());
    }

    // Window points per degree and per percent, for turning drag travel into curve units.
    let (per_deg, per_pct) = ((x1 - x0) / (t1 - t0), (y1 - y0) / 100.0);
    let curve = &draft.curve;
    for (i, pt) in curve.iter().enumerate() {
        let (x, y) = (fx(pt.temp), fy(pt.percent));
        let dragging = s.grab.is_some_and(|(g, _)| g == i);
        // Shrink the hit target as neighbours close in, so one point's never covers another's.
        let gap = [i.checked_sub(1), Some(i + 1)]
            .into_iter()
            .flatten()
            .filter_map(|j| curve.get(j))
            .map(|q| (fx(q.temp) - x).hypot(fy(q.percent) - y))
            .fold(f32::INFINITY, f32::min);
        let hit = gap.clamp(DOT + 2.0, HIT);
        chart = chart.child(
            el().key(format!("point-{i}"))
                .role("slider")
                .label(format!("Curve point {}: {:.0}% at {:.0}°C", i + 1, pt.percent, pt.temp))
                .size(hit, hit)
                .absolute(x - hit / 2.0, y - hit / 2.0)
                .center()
                .radius(hit / 2.0)
                .hover(p.t.text.alpha(0.07))
                .cursor(if dragging { Cursor::Grabbing } else { Cursor::Grab })
                .on_drag(move |s: &mut State, d: &mut Drag, _| match d.phase {
                    Phase::Down => s.grab = Some((i, s.draft.curve[i])),
                    Phase::Move => {
                        if let Some((_, at)) = s.grab {
                            s.draft.move_point(i, at.temp + d.dx / per_deg, at.percent - d.dy / per_pct, TEMP_RANGE);
                        }
                    }
                    Phase::Up => {
                        if let (true, Some((_, at))) = (d.cancelled(), s.grab) {
                            s.draft.curve[i] = at;
                        }
                        s.grab = None;
                    }
                })
                .child(
                    el().key("dot")
                        .size(DOT, DOT)
                        .radius(DOT / 2.0)
                        .fill(p.t.bg)
                        .border(2.5, p.heat(pt.temp))
                        .a11y_hidden(),
                ),
        );
        if dragging {
            chart = chart.child(
                el().key("readout").absolute(x - 44.0, (y - 36.0).max(0.0)).w(88.0).h(24.0).center()
                    .radius(radius::SM).fill(p.t.text)
                    .child(text(format!("{:.0}% at {:.0}°", pt.percent, pt.temp)).key("text")
                        .font(type_::LABEL).color(p.t.bg))
                    .a11y_hidden(),
            );
        }
    }
    chart
}

/// Results on the left; Revert and Apply only while there's something to apply.
fn footer(p: Paint, s: &State) -> El {
    let notice = s.notice.as_ref().map(|n| match n {
        Ok(m) => (m.clone(), p.t.text_secondary),
        Err(m) => (m.clone(), p.danger),
    });
    let dirty = s.dirty();
    row()
        .key("footer")
        .w_full()
        .h(32.0)
        .center_y()
        .gap(space::S2)
        .child(el().key("spacer").grow(1.0).children(notice.map(|(m, c)| {
            text(m).key("notice").role("status").font(type_::BODY).color(c).truncate()
        })))
        .children((dirty && !s.busy).then(|| {
            button(p, "revert", "Revert", false, |s| {
                s.draft = s.saved.clone();
                s.custom = None;
                s.notice = None;
            })
        }))
        .children((dirty || s.busy).then(|| {
            let label = if s.busy { "Applying" } else { "Apply" };
            button(p, "apply", label, true, State::apply).disabled(s.busy).opacity(if s.busy { 0.5 } else { 1.0 })
        }))
}

fn button(p: Paint, key: &'static str, label: &'static str, primary: bool, action: fn(&mut State)) -> El {
    el().key(key)
        .role("button")
        .label(label)
        .focusable(true)
        .h(32.0)
        .px(16.0)
        .center()
        .radius(radius::MD)
        .fill(if primary { p.t.text } else { Color::TRANSPARENT })
        .hover(if primary { p.t.bg.alpha(0.12) } else { p.t.hover })
        .press(p.t.press)
        .transition(Transition::opacity())
        .on_press(action)
        .child(text(label).key("label").font(type_::TITLE).color(if primary { p.t.bg } else { p.t.text }).a11y_hidden())
}

fn title_case(name: &str) -> String {
    let mut chars = name.chars();
    chars.next().map_or_else(String::new, |c| c.to_uppercase().chain(chars).collect())
}

fn main() {
    // Matches the .desktop file name, so docks show its icon for the window.
    rune::set_app_id("razer-coolingpad-gui");
    rune::run(AppSpec::<State> {
        min_size: Some((520.0, 460.0)),
        setup: Some(setup),
        poll: Some(poll),
        scenes: &[("custom", |s| s.draft.curve[1].percent += 15.0)],
        ..AppSpec::new("Cooling Pad", (600.0, 540.0), init, view)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use rune::{Env, Runtime};

    #[test]
    fn a_dragged_curve_survives_trying_a_preset_and_reverts_to_the_saved_one() {
        let mut rt = Runtime::new(init(), view, (600.0, 540.0), Env::new(true, 1.0));
        rt.update(|s, _| {
            s.presets = model::load_presets(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../curves")));
            s.draft = s.presets[0].config.clone();
            s.saved = s.draft.clone();
        });
        rt.frame(0.0);
        rt.frame(0.0);
        assert!(rt.find("Custom").is_none());
        let before = rt.state.draft.curve[1];

        let (x, y) = rt.find("point-1").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_move(x, y - 40.0);
        rt.pointer_up(x, y - 40.0);
        rt.frame(1.0 / 60.0);
        let after = rt.state.draft.curve[1];
        assert_eq!(after.temp, before.temp);
        assert!(after.percent > before.percent, "{before:?} -> {after:?}");
        assert!(rt.find("Custom").is_some() && rt.find("revert").is_some());
        let edited = rt.state.draft.clone();

        let press = |rt: &mut Runtime<State>, key: &str, t: f64| {
            let (x, y) = rt.find(key).unwrap();
            rt.pointer_down(x, y);
            rt.pointer_up(x, y);
            rt.frame(t);
        };
        press(&mut rt, "Balanced", 2.0 / 60.0);
        assert_eq!(rt.state.active_preset().unwrap().name, "balanced");
        press(&mut rt, "Custom", 3.0 / 60.0);
        assert_eq!(rt.state.draft, edited);

        let (x, y) = rt.find("revert").unwrap();
        rt.pointer_down(x, y);
        rt.pointer_up(x, y);
        rt.frame(4.0 / 60.0);
        assert_eq!(rt.state.draft, rt.state.saved);
        assert!(rt.find("Custom").is_none());
    }
}
