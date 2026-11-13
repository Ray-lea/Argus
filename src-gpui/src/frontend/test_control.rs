//! Test-only control: `--test` instances read JSON lines that switch pages,
//! drive the pointer and keyboard through the normal dispatch path, and print
//! what the interface laid out. Never reachable outside test mode.
use super::*;
impl Desktop {
    pub(in crate::frontend) fn test_control(
        &mut self,
        control: &Value,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.options.fixture.is_some()
            && let Some(fields) = control["formValues"].as_object()
        {
            for (key, value) in fields {
                if let (Some(input), Some(value)) = (self.forms.get(key).cloned(), value.as_str()) {
                    input.update(cx, |state, cx| state.set_value(value, window, cx));
                }
            }
        }
        if self.options.fixture.is_some()
            && let Some(restored) = control.get("restored")
        {
            let _ = self.host.emit("argus-restored", restored.clone());
        }
        if self.options.fixture.is_some()
            && let Some(saved) = control.get("restoreWindow")
            && let Ok(saved) =
                serde_json::from_value::<argus_app::preferences::WindowPlacement>(saved.clone())
            && saved.valid()
            && let Some(native) = self.host.get_window("main")
            && native.is_visible().is_ok_and(|visible| !visible)
        {
            let file = self
                .host
                .path()
                .app_config_dir()
                .unwrap()
                .join("preferences.json");
            argus_app::preferences::save_window(&file, saved).expect("test window placement");
            crate::window_state::install(native.hwnd().unwrap(), file);
        }
        if self.options.fixture.is_some()
            && let Some(dimensions) = control["nativeResize"].as_array().filter(|a| a.len() == 2)
            && let Some(width) = dimensions[0]
                .as_f64()
                .filter(|n| n.is_finite() && (64. ..=8192.).contains(n))
            && let Some(height) = dimensions[1]
                .as_f64()
                .filter(|n| n.is_finite() && (64. ..=8192.).contains(n))
        {
            // Exercise synchronous WM_SIZE while GPUI's window is borrowed,
            // as happens when native placement is restored during startup.
            self.host
                .get_window("main")
                .expect("test window")
                .prepare_hidden_size(width as f32, height as f32)
                .expect("isolated native resize");
        }
        if self.options.fixture.is_some()
            && let Some(scale) = control["contentZoom"]
                .as_f64()
                .filter(|scale| scale.is_finite() && (0.5..=3.).contains(scale))
        {
            window.defer(cx, move |window, cx| {
                window.set_content_zoom(scale as f32, cx);
            });
        }
        #[cfg(debug_assertions)]
        if let Some(delay) = control["delayProcessQuery"].as_u64() {
            argus_core::process::testing::delay_next_counter_reply(delay);
        }
        if let Some(label) = control["inspectProcessSampling"].as_str() {
            #[cfg(debug_assertions)]
            let delayed = argus_core::process::testing::counter_reply_delayed();
            #[cfg(not(debug_assertions))]
            let delayed = false;
            let requested = values(&control["pids"]);
            let processes = values(&self.snapshot["processes"]);
            let selected: Vec<_> = processes.iter().filter(|p| requested.contains(&p["pid"])).map(|p| {
                let row = model::Row::process(p.clone(),0);
                json!({"pid":p["pid"],"created":p["createTime"],"key":model::identity(p),"sequence":p["sequenceNumber"],
                    "pending":p["countersPending"],"reused":p["sampleReused"],"cpu":row.metric("cpu"),"memory":row.metric("mem")})
            }).collect();
            println!(
                "{}",
                json!({"event":"process-sampling","label":label,"seq":self.sequence,"timestamp":self.snapshot["timestamp"],
                "delayed":delayed,"delaySupported":cfg!(debug_assertions),"cpuPoints":self.timeline.points("system.cpu").map_or(0,|p|p.len()),
                "selected":self.selected.get("processes"),
                "count":processes.len(),"roster":processes.iter().filter(|p|boolean(p,"listedByRoster")).count(),
                "reused":processes.iter().filter(|p|boolean(p,"sampleReused")).count(),"pending":processes.iter().filter(|p|boolean(p,"countersPending")).count(),"targets":selected})
            );
        }
        if let Some(label) = control["inspectVitals"].as_str() {
            let bounds = self
                .ui
                .anchors
                .borrow()
                .iter()
                .filter(|(key, _)| key.starts_with("vital-"))
                .map(|(key, b)| {
                    (
                        key.to_string(),
                        json!([
                            f32::from(b.left()),
                            f32::from(b.top()),
                            f32::from(b.size.width),
                            f32::from(b.size.height)
                        ]),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            println!(
                "{}",
                json!({"event":"ui-vitals","label":label,"bounds":bounds})
            );
        }
        if control["testTrayUpdate"].as_bool() == Some(true)
            && crate::shell::test_tray_delay().is_some()
        {
            crate::shell::set_tray_update(&self.host, Some("test"));
        }
        if let Some(label) = control["inspectMenu"].as_str() {
            println!(
                "{}",
                json!({"event":"ui-menu","label":label,"state":self.menu_state(window),
                    "window":{"active":window.is_window_active()}})
            );
        }
        if let Some(label) = control["inspectColumns"].as_str() {
            println!(
                "{}",
                json!({"event":"ui-process-columns","label":label,"state":self.process_columns_diagnostics(cx)})
            );
        }
        if self.options.fixture.is_some()
            && let Some(value) = control["columnSearch"].as_str()
            && let Some(input) = self.forms.get("columnsSearch").cloned()
        {
            input.update(cx, |state, cx| {
                state.replace_all(value.to_owned(), window, cx)
            });
        }
        if let Some(groups) = control["freezeChartGroups"].as_array()
            && let Some(layer) = self.chart_layer.clone()
        {
            layer.update(cx, |layer, _| {
                layer.frozen_groups = groups
                    .iter()
                    .filter_map(|group| group.as_str().map(str::to_owned))
                    .collect();
            });
            window.refresh();
        }
        if self.options.fixture.is_some()
            && let Some(time) = control["chartTime"]
                .as_f64()
                .filter(|time| time.is_finite())
        {
            self.chart_clock.borrow_mut().pause_at(time);
            if let Some(layer) = self.chart_layer.clone() {
                window.on_next_frame(move |window, cx| {
                    layer.update(cx, |layer, cx| {
                        let before = layer.fast_frames;
                        layer.advance(window, cx);
                        println!(
                            "{}",
                            json!({"event":"ui-chart-step","retained":layer.fast_frames>before})
                        );
                    });
                });
            }
            return;
        }
        if self.options.fixture.is_some() && control["fullPaint"].as_bool() == Some(true) {
            window.invalidate_render_cache();
            return;
        }
        if control["exit"].as_bool() == Some(true) {
            self.host.exit(0);
            return;
        }
        if control["pointerOutside"].as_bool() == Some(true) {
            window.defer(cx, |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: point(px(-1.), px(-1.)),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        if let Some(wheel) = control.get("wheel") {
            // A mouse-wheel notch at a point (test only): {"x":…,"y":…,"lines":…}.
            let event = ScrollEvent {
                position: point(px(number(wheel, "x") as f32), px(number(wheel, "y") as f32)),
                delta: ScrollDelta::Lines(point(
                    number(wheel, "linesX") as f32,
                    number(wheel, "lines") as f32,
                )),
                modifiers: Modifiers {
                    shift: boolean(wheel, "shift"),
                    ..Default::default()
                },
                touch_phase: TouchPhase::Moved,
                source: ScrollSource::Pointer,
            };
            window.defer(cx, move |window, cx| {
                if let Some(ms) = scrollbar::trace_ms() {
                    println!("{}", json!({"event":"wheel","ms":ms}));
                }
                window.dispatch_event(PlatformInput::Scroll(event), cx);
            });
        }
        if let Some(ms) = control["scrollTrace"].as_u64() {
            // Print every scrollbar's offset and thumb on each frame (test only).
            scrollbar::trace(ms);
            window.refresh();
        }
        if let Some(mouse) = control.get("mouse") {
            // A left-button pointer step at a point (test only): {"type":"move|down|up","x":…,"y":…}.
            let position = point(px(number(mouse, "x") as f32), px(number(mouse, "y") as f32));
            let button = if mouse["button"].as_str() == Some("right") {
                MouseButton::Right
            } else {
                MouseButton::Left
            };
            let input = match mouse["type"].as_str() {
                Some("down") => PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button,
                    click_count: 1,
                    ..Default::default()
                }),
                Some("up") => PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    button,
                    click_count: 1,
                    ..Default::default()
                }),
                Some("drag") => PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    pressed_button: Some(button),
                    ..Default::default()
                }),
                _ => PlatformInput::MouseMove(MouseMoveEvent {
                    position,
                    ..Default::default()
                }),
            };
            window.defer(cx, move |window, cx| {
                window.dispatch_event(input, cx);
            });
        }
        if let Some(bounds) = control["hoverChart"]
            .as_str()
            .and_then(|id| self.chart_bounds(id))
            .or_else(|| {
                control["hoverHeader"]
                    .as_str()
                    .and_then(|id| self.header_bounds.borrow().get(id).copied())
            })
        {
            let fraction = |index| {
                control["hoverAt"][index]
                    .as_f64()
                    .unwrap_or(0.5)
                    .clamp(0., 1.) as f32
            };
            let position = bounds.origin
                + point(
                    bounds.size.width * fraction(0),
                    bounds.size.height * fraction(1),
                );
            // Dispatch after releasing Desktop; hover listeners may update it.
            window.defer(cx, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        // Pointer input at a remembered element (`ui.anchors`): hover its
        // middle, click it, or drag it to a fraction of another's height.
        let anchor = |name: &Value| {
            name.as_str().and_then(|name| {
                self.ui
                    .anchors
                    .borrow()
                    .get(&SharedString::from(name.to_owned()))
                    .copied()
            })
        };
        let mut pointer = Vec::new();
        let moved = |position, pressed| {
            PlatformInput::MouseMove(MouseMoveEvent {
                position,
                pressed_button: pressed,
                ..Default::default()
            })
        };
        let press = |position, down: bool| {
            if down {
                PlatformInput::MouseDown(MouseDownEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                })
            } else {
                PlatformInput::MouseUp(MouseUpEvent {
                    position,
                    button: MouseButton::Left,
                    click_count: 1,
                    ..Default::default()
                })
            }
        };
        if let Some(bounds) = anchor(&control["hoverAnchor"]) {
            pointer.push(moved(bounds.center(), None));
        }
        // Presses act on what they reach, as keys do: with a fixture only, where
        // commands to processes and the system are not carried out (`request`).
        let presses = self.options.fixture.is_some();
        if presses && let Some(bounds) = anchor(&control["clickAnchor"]) {
            let at = bounds.center();
            pointer.extend([moved(at, None), press(at, true), press(at, false)]);
        }
        if presses
            && let (Some(from), Some(to)) = (
                anchor(&control["dragAnchor"][0]),
                anchor(&control["dragAnchor"][1]),
            )
        {
            let fraction = control["dragAnchor"][2].as_f64().unwrap_or(0.5) as f32;
            let start = from.center();
            let end = point(to.center().x, to.top() + to.size.height * fraction);
            pointer.extend([moved(start, None), press(start, true)]);
            for step in 1..=8 {
                let t = step as f32 / 8.;
                pointer.push(moved(start + (end - start) * t, Some(MouseButton::Left)));
            }
            pointer.push(press(end, false));
        }
        // One step of a slower gesture: a hover, down, a move holding the button or
        // up, at the middle of a remembered element (or `header:<key>`) moved by
        // `dx`, `dy`. Without `at`, it is measured from where the gesture began, so
        // a drag reads the same however the element moves under it.
        let step = &control["pointer"];
        let target = step["at"]
            .as_str()
            .and_then(|name| match name.strip_prefix("header:") {
                Some(key) => self.header_bounds.borrow().get(key).copied(),
                None => anchor(&step["at"]),
            })
            .map(|bounds| bounds.center());
        if target.is_some() {
            self.ui.gesture_origin.set(target);
        }
        if step.is_object()
            && let Some(origin) = target.or(self.ui.gesture_origin.get())
        {
            let at = origin
                + point(
                    px(step["dx"].as_f64().unwrap_or(0.) as f32),
                    px(step["dy"].as_f64().unwrap_or(0.) as f32),
                );
            match step["event"].as_str() {
                Some("hover") => pointer.push(moved(at, None)),
                _ if !presses => {}
                // A right press and release, as opens a context menu.
                Some("context") => pointer.extend([
                    moved(at, None),
                    PlatformInput::MouseDown(MouseDownEvent {
                        position: at,
                        button: MouseButton::Right,
                        click_count: 1,
                        ..Default::default()
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        position: at,
                        button: MouseButton::Right,
                        click_count: 1,
                        ..Default::default()
                    }),
                ]),
                // `clicks` 2: the second press of a double-click, held.
                Some("down") => pointer.push(PlatformInput::MouseDown(MouseDownEvent {
                    position: at,
                    button: MouseButton::Left,
                    click_count: step["clicks"].as_u64().map_or(1, |clicks| clicks as usize),
                    ..Default::default()
                })),
                Some("up") => pointer.push(press(at, false)),
                // A second click in place, as the platform reports one.
                Some("double") => pointer.extend([
                    moved(at, None),
                    press(at, true),
                    press(at, false),
                    PlatformInput::MouseDown(MouseDownEvent {
                        position: at,
                        button: MouseButton::Left,
                        click_count: 2,
                        ..Default::default()
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        position: at,
                        button: MouseButton::Left,
                        click_count: 2,
                        ..Default::default()
                    }),
                ]),
                _ => pointer.push(moved(at, Some(MouseButton::Left))),
            }
        }
        if !pointer.is_empty() {
            window.defer(cx, move |window, cx| {
                for input in pointer {
                    window.dispatch_event(input, cx);
                }
            });
            // An idle window would hold the input until its next frame.
            window.refresh();
        }
        if let Some(label) = control["inspectLayout"].as_str() {
            if std::env::var_os("ARGUS_GPUI_VIEW_TRACE").is_some() {
                use gpui_kit::gpui::profiler::{self, FrameEvent, FrameTimingCollector};
                let collector = self.frame_trace.get_or_insert_with(|| {
                    profiler::set_trace_enabled(true);
                    FrameTimingCollector::new()
                });
                let mut draws = collector
                    .collect_unseen()
                    .into_iter()
                    .filter_map(|event| match event {
                        FrameEvent::Draw(frame) => Some(frame),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                draws.sort_by_key(|frame| std::cmp::Reverse(frame.draw_duration()));
                println!(
                    "{}",
                    json!({"event":"ui-draw-phases","label":label,"draws":draws.len(),"slowest":draws.iter().take(12).map(|frame|json!({"totalUs":frame.draw_duration().as_micros(),"requestUs":frame.phases.request_layout.as_micros(),"prepaintUs":frame.phases.prepaint.as_micros(),"paintUs":frame.phases.paint.as_micros(),"invalidations":frame.invalidations})).collect::<Vec<_>>()})
                );
                use profiler::journal::{ForegroundEvent, ForegroundJournalEntry};
                let journal = self
                    .view_trace
                    .get_or_insert_with(|| cx.foreground_journal().collector());
                let mut views = journal
                    .collect_unseen()
                    .entries
                    .into_iter()
                    .filter_map(|entry| match entry {
                        ForegroundJournalEntry::Event(ForegroundEvent::View(view)) => Some(view),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                views.sort_by_key(|view| std::cmp::Reverse(view.self_duration));
                println!(
                    "{}",
                    json!({"event":"ui-view-phases","label":label,"slowest":views.iter().take(16).map(|view|json!({"name":view.name,"phase":view.phase.as_str(),"selfUs":view.self_duration.as_micros(),"elapsedUs":view.end.duration_since(view.start).as_micros()})).collect::<Vec<_>>()})
                );
            }
            println!(
                "{}",
                json!({"event":"ui-conditions","label":label,"processes":self.snapshot["system"]["processCount"],"intervalMs":self.interval,"dpiScale":window.scale_factor(),"gpus":values(&self.snapshot["hardware"]["gpus"]).iter().map(|gpu|json!({"id":gpu["info"]["id"],"clockMhz":gpu["clockMhz"],"clockMaxMhz":gpu["clockMaxMhz"],"boardPowerWatts":gpu["boardPower"]})).collect::<Vec<_>>()})
            );
            let fast_frames = self
                .chart_layer
                .as_ref()
                .map_or(0, |layer| layer.read(cx).fast_frames);
            println!(
                "{}",
                json!({"event":"ui-retained-frames","label":label,"fastFrames":fast_frames})
            );
            if let Some(layer) = &self.chart_layer {
                println!(
                    "{}",
                    json!({"event":"ui-chart-states","label":label,"charts":layer.read(cx).diagnostics(window)})
                );
            }
            println!(
                "{}",
                json!({"event":"ui-work","label":label,"serializeUs":self.work.serialize_us,"backgroundGroupUs":self.work.background_group_us,"timelineUs":self.work.timeline_us,"rowsUs":self.work.rows_us,"rowsMaxUs":self.work.rows_max_us,"rowBuilds":self.work.row_builds})
            );
            let (builds, micros, peak_bytes) = charts::raster_metrics();
            println!(
                "{}",
                json!({"event":"ui-chart-work","label":label,"rasterBuilds":builds,"rasterMicros":micros,"peakRasterBuildBytes":peak_bytes})
            );
            let frames = window.frame_duration_snapshot();
            let controls = window.window_control_regions();
            let control_matches = |area| {
                controls
                    .iter()
                    .find(|(kind, _)| *kind == area)
                    .is_some_and(|(_, bounds)| {
                        window.window_control_area_at(bounds.center()) == Some(area)
                    })
            };
            let drag_points = controls
                .iter()
                .filter(|(area, _)| *area == WindowControlArea::Drag)
                .map(|(_, bounds)| {
                    (0..(f32::from(bounds.size.width) / 8.) as usize)
                        .filter(|index| {
                            window.window_control_area_at(point(
                                bounds.left() + px(*index as f32 * 8.),
                                bounds.center().y,
                            )) == Some(WindowControlArea::Drag)
                        })
                        .count()
                })
                .sum::<usize>();
            let actions_are_client = window
                .window_control_area_at(self.toolbar_regions.borrow()[1].center())
                .is_none();
            println!(
                "{}",
                json!({"event":"ui-caption","label":label,"registered":controls.len(),"dragPoints":drag_points,"actionsAreClient":actions_are_client,
                "minimize":control_matches(WindowControlArea::Min),"maximize":control_matches(WindowControlArea::Max),"close":control_matches(WindowControlArea::Close)})
            );
            println!(
                "{}",
                json!({"event":"ui-frame-duration","label":label,
                "drawP50Ms":frames.draw_duration_histogram.value_at_quantile(0.5) as f64/1e6,
                "drawP95Ms":frames.draw_duration_histogram.value_at_quantile(0.95) as f64/1e6,
                "drawP99Ms":frames.draw_duration_histogram.value_at_quantile(0.99) as f64/1e6,
                "drawMaxMs":frames.draw_duration_histogram.max() as f64/1e6})
            );
            let temperature = self
                .timeline
                .points(model::CPU_TEMPERATURE_SERIES)
                .unwrap_or_default();
            let about_max = f32::from(self.about_scroll.max_offset().y);
            let about_offset = f32::from(self.about_scroll.offset().y).abs();
            let native_window = self.host.get_window("main").and_then(|native| {
                use windows::Win32::{Foundation::RECT, UI::WindowsAndMessaging::*};
                let hwnd = native.hwnd().ok()?;
                let mut client = RECT::default();
                unsafe { GetClientRect(hwnd, &mut client) }.ok()?;
                let viewport = window
                    .viewport_size()
                    .map(|length| length * window.scale_factor());
                Some(json!({
                    "maximized": unsafe { IsZoomed(hwnd).as_bool() },
                    "width": client.right, "height": client.bottom,
                    "viewportMatchesClient": (f32::from(viewport.width) - client.right as f32).abs() < 1.
                        && (f32::from(viewport.height) - client.bottom as f32).abs() < 1.
                }))
            });
            println!(
                "{}",
                json!({"event":"ui-layout","label":label,"page":self.page,"device":self.device,
                "vitals":self.vitals_band_state(),
                "visibility":{"enabled":self.prefs.get::<bool>(visibility::preference(&self.page),false),
                    "pids":self.rows.iter().filter(|row|row.section.is_none()).flat_map(|row| if row.members.is_empty(){std::slice::from_ref(&row.raw)}else{row.members.as_slice()}).filter_map(|p|p["pid"].as_u64()).collect::<std::collections::BTreeSet<_>>()},
                "processTree":{"view":self.view,"visibleRows":self.rows.len(),"visibleChildren":self.rows.iter().filter(|row|row.depth > 0).count(),"expandedParents":self.rows.iter().filter(|row|row.section.is_none() && row.expandable && row.expanded).count(),"searching":!self.query.trim().is_empty()},
                "animations":{"ui":!cx.reduce_motion(),"window":crate::window_motion::enabled(),"charts":format!("{:?}",self.chart_clock.borrow().mode)},
                "cpuTemperatureHistory":{"recentSamples":temperature.len().min(60),"recentMissing":temperature.iter().rev().take(60).filter(|(_,value)|value.is_none()).count(),"last":temperature.last().and_then(|(_,value)|*value)},
                "systemBackdrop":crate::platform::supports_backdrop(),"backgroundAlpha":theme::Palette::current(cx).background.a,
                "userGroups":{"mode":self.user_grouping().key(),"grouped":self.user_groups().is_some(),
                    "users":model::UserGroups::new(values(&self.snapshot["sessions"])).users(),
                    "headings":self.rows.iter().filter(|row|row.user.is_some()).map(|row|json!({"key":row.key,"title":row.title,"count":row.raw["entryCount"],"expanded":row.expanded})).collect::<Vec<_>>()},
                "dialog":self.dialog_test_state(),
                "toasts":self.ui.toasts.iter().filter(|toast| toast.leaving.is_none()).map(|toast| toast.text.as_str()).collect::<Vec<_>>(),
                "diagnosticCopy":if self.options.fixture.is_some() { self.data.get("diagnosticCopy") } else { None },
                "diagnosticCopyReady":self.options.fixture.is_some() && self.data.contains_key("diagnosticCopy"),
                "performance":self.performance_test_state(),
                "star":self.star_test_state(),
                "sequence":self.sequence,"timestamp":self.snapshot["timestamp"],"processIcons":self.ui.icons.borrow().len(),
                "allocationHeadroomBytes":crate::memory_pressure::available_memory(),
                "nativeWindow":native_window,
                "width":f32::from(window.viewport_size().width),"height":f32::from(window.viewport_size().height),"scaleFactor":window.scale_factor(),"remSize":f32::from(window.rem_size()),
                "cpuPlot":self.chart_bounds("cpu-overall-plot").map(|b|[f32::from(b.left()),f32::from(b.top()),f32::from(b.size.width),f32::from(b.size.height)]),
                "performanceScrollY":f32::from(self.device_scroll.max_offset().y),
                "performanceCardHeight":self.perf_card_chrome.get().map(|chrome| f32::from(window.viewport_size().height - chrome)),
                "aboutScroll":{"max":about_max,"offset":about_offset,"canScroll":about_max > 0.5,
                    "atEnd":about_max > 0.5 && (about_offset-about_max).abs() < 0.5},
                "settingsScrollY":f32::from(self.settings_scroll.max_offset().y),"settingsTop":self.settings_scroll.top_item()})
            );
        }
        if self.options.fixture.is_some()
            && let Some(key) = control["clickHeader"].as_str()
            && let Some(bounds) = self.header_bounds.borrow().get(key).copied()
        {
            // Exercise the real hit target, away from the text and sort indicator.
            let position = point(
                bounds.left() + bounds.size.width * 0.15,
                bounds.top() + bounds.size.height * 0.25,
            );
            window.defer(cx, move |window, cx| {
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseDown(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseUp(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    cx,
                );
                window.dispatch_event(
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position: point(px(-1.), px(-1.)),
                        ..Default::default()
                    }),
                    cx,
                );
            });
        }
        if self.options.fixture.is_some()
            && let Some(data) = control["data"].as_object()
        {
            for (key, value) in data {
                self.data.insert(key.clone(), value.clone());
            }
            self.take_connections();
        }
        // A sample made up from the current one (fixture only), accepted as a
        // collected one: taken `at` ms from now on the wall clock, with `set`
        // replacing values by JSON pointer. The first one starts the chart
        // clock as in a live session, so that a test can watch curves scroll
        // while samples arrive (`chartTime` still stops it).
        if self.options.fixture.is_some()
            && let Some(sample) = control.get("sample")
        {
            let wall = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64()
                * 1000.;
            let mut snapshot = self.snapshot.clone();
            snapshot["timestamp"] = json!((wall + number(sample, "at")).round());
            snapshot["seq"] = json!(self.sequence + 1);
            if let Some(set) = sample["set"].as_object() {
                for (pointer, value) in set {
                    if let Some(slot) = snapshot.pointer_mut(pointer) {
                        *slot = value.clone();
                    }
                }
            }
            let full = self.snapshot.is_null()
                || self.snapshot["hardware"]["cpuSensors"]["state"]
                    != snapshot["hardware"]["cpuSensors"]["state"];
            self.accept_snapshot(snapshot, window, cx);
            if !self.fixture_samples {
                self.fixture_samples = true;
                self.chart_clock.borrow_mut().set_paused(false);
            }
            if control.as_object().is_some_and(|object| object.len() == 1) {
                self.notify_snapshot(full, window, cx);
                return;
            }
            cx.notify();
        }
        // A command in flight (fixture only): held and never dispatched, so the
        // view shows what it shows while waiting, until `resolve` answers it.
        if self.options.fixture.is_some()
            && let Some(command) = control["hold"]["command"].as_str()
        {
            self.next_request += 1;
            self.pending.insert(
                self.next_request,
                Pending {
                    command: command.to_owned(),
                    cache: control["hold"]["cache"].as_str().map(str::to_owned),
                    args: control["hold"]["args"].clone(),
                },
            );
        }
        // Answers a held command, or one made up here, through the normal
        // completion path (fixture only): nothing is ended or restarted.
        if self.options.fixture.is_some()
            && let Some(command) = control["resolve"]["command"].as_str()
        {
            let answer = &control["resolve"];
            let held = self
                .pending
                .iter()
                .find(|(_, pending)| pending.command == command)
                .map(|(id, _)| *id);
            let id = held.unwrap_or_else(|| {
                self.next_request += 1;
                self.pending.insert(
                    self.next_request,
                    Pending {
                        command: command.to_owned(),
                        cache: answer["cache"].as_str().map(str::to_owned),
                        args: answer["args"].clone(),
                    },
                );
                self.next_request
            });
            let value = answer["value"].clone();
            let result = if boolean(answer, "ok") {
                Ok(value)
            } else {
                Err(value)
            };
            self.complete(id, result, window, cx);
        }
        if let Some(text) = control["toast"].as_str() {
            self.notify_toast(widgets::ToastKind::Info, text.to_owned());
        }
        // The star request, whatever the day (tests never get it on their own).
        if control["starPrompt"].as_bool() == Some(true) {
            self.show_star();
        }
        // A portable copy's notice, which tests never get on their own either.
        if control["portableNotice"].as_bool() == Some(true) {
            self.show_portable_notice();
        }
        if self.options.fixture.is_some() && control["updateNotice"].as_bool() == Some(true) {
            let status = self.data.get("updates").cloned().unwrap_or_default();
            self.show_update_notice(text(&status, "state"), text(&status, "version"));
        }
        if control["recordHotkey"].as_bool() == Some(true) {
            self.begin_hotkey(window, cx);
        }
        if self.options.fixture.is_some()
            && let Some(keys) = control["keys"].as_array()
        {
            // Synthetic keys through the normal dispatch path, for dialogs and the recorder.
            let strokes = keys
                .iter()
                .filter_map(Value::as_str)
                .filter_map(|k| Keystroke::parse_with_os(k, OperatingSystem::Windows).ok())
                .collect::<Vec<_>>();
            window.defer(cx, move |window, cx| {
                for stroke in strokes {
                    let modifiers = |modifiers| {
                        PlatformInput::ModifiersChanged(ModifiersChangedEvent {
                            modifiers,
                            capslock: Default::default(),
                        })
                    };
                    window.dispatch_event(modifiers(stroke.modifiers), cx);
                    window.dispatch_event(
                        PlatformInput::KeyDown(KeyDownEvent {
                            keystroke: stroke.clone(),
                            is_held: false,
                            prefer_character_input: false,
                        }),
                        cx,
                    );
                    window
                        .dispatch_event(PlatformInput::KeyUp(KeyUpEvent { keystroke: stroke }), cx);
                    window.dispatch_event(modifiers(Modifiers::default()), cx);
                }
            });
        }
        if self.options.fixture.is_some()
            && let Some(id) = control["clickSelect"].as_str()
            && let Some(bounds) = self.ui.anchors.borrow().get(id).copied()
        {
            let position = bounds.center();
            window.defer(cx, move |window, cx| {
                for event in [
                    PlatformInput::MouseMove(MouseMoveEvent {
                        position,
                        ..Default::default()
                    }),
                    PlatformInput::MouseDown(MouseDownEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                    PlatformInput::MouseUp(MouseUpEvent {
                        position,
                        button: MouseButton::Left,
                        click_count: 1,
                        ..Default::default()
                    }),
                ] {
                    window.dispatch_event(event, cx);
                }
            });
        }
        if let Some(section) = control["settingsSection"].as_str() {
            self.preference("settingsSection", json!(section), window, cx);
        }
        if let (Some(section), Some(item)) = (
            control["locate"]["section"].as_str(),
            control["locate"]["item"].as_str(),
        ) {
            // Points out a settings card as a notice does (test only).
            self.locate_setting(section, item, window, cx);
        }
        if let Some(label) = control["inspectLayout"].as_str() {
            println!(
                "{}",
                json!({"event":"ui-render-counts","label":label,"desktop":self.render_count,"chartTicks":self.chart_frame,"sort":self.sort,"descending":self.descending})
            );
        }
        // The page's rows as built, to inspect titles and ordering.
        if let Some(label) = control["dumpRows"].as_str() {
            let rows: Vec<Value> = self
                .rows
                .iter()
                .map(|row| {
                    let count = if row.section.is_some() {
                        row.raw["entryCount"].clone()
                    } else {
                        json!(row.members.len())
                    };
                    json!({"key":row.key,"title":row.title,"detail":row.detail,"depth":row.depth,"count":count,"section":row.section,"nested":row.nested,"cpu":row.metric("cpu"),"gpu":row.metric("gpu")})
                })
                .collect();
            println!(
                "{}",
                json!({"event":"rows","label":label,"page":self.page,"view":self.view,"sort":self.sort,"descending":self.descending,"rows":rows})
            );
        }
        if let Some(key) = control["sort"].as_str() {
            self.act(Action::Sort(key.into()), window, cx);
        }
        if let Some(index) = control["foldRow"].as_u64()
            && let Some(row) = self.rows.get(index as usize)
        {
            self.act(Action::Fold(row.key.clone()), window, cx);
        }
        // The details pane's width, as dragging its edge sets it (test only).
        if let Some(width) = control["detailsWidth"].as_f64() {
            self.details_width = Self::clamp_details_width(width as f32, window);
        }
        for key in [
            "navPosition",
            "navExpanded",
            "coreFilter",
            "diskPage",
            "uiMotion",
            "windowMotion",
            "historyRange",
            "netUnit",
            "fontSize",
            "uiScale",
        ] {
            if let Some(value) = control.get(key) {
                self.preference(key, value.clone(), window, cx);
            }
        }
        // Any preference, through the same path as the interface's own
        // controls: `{"preference": {"diskPage": "details"}}`.
        if let Some(preferences) = control["preference"].as_object() {
            for (key, value) in preferences {
                self.preference(key, value.clone(), window, cx);
            }
        }
        if let Some(mode) = control["motion"].as_str() {
            self.preference("motion.chartMode", json!(mode), window, cx);
        }
        if let Some(language) = control["language"].as_str() {
            self.request(
                "set_language",
                json!({"language":language}),
                Some("settings"),
                window,
                cx,
            );
        }
        if let Some(page) = control["page"].as_str() {
            self.act(Action::Page(page.into()), window, cx);
        }
        if let Some(query) = control["query"].as_str() {
            self.query = query.into();
            self.search
                .update(cx, |s, cx| s.set_value(query.to_owned(), window, cx));
            self.rebuild_rows(false, cx);
        }
        if let Some(index) = control["selectRow"].as_u64()
            && let Some(row) = self.rows.get(index as usize)
        {
            self.act(Action::Select(row.key.clone()), window, cx);
        }
        if let Some(key) = control["headerMenu"].as_str() {
            // As a right press at the start of that column's header would.
            let at = self
                .header_bounds
                .borrow()
                .get(key)
                .map(|bounds| point(bounds.left() + px(16.), bounds.bottom()));
            self.process_header_menu(Some(key.into()), at, window, cx);
        }
        // What a right press on that row does; `pointer` presses on it for real.
        if let Some(index) = control["rowMenu"].as_u64() {
            self.open_row_menu(index as usize, window, cx);
        }
        if control["dismiss"].as_bool() == Some(true) {
            self.act(Action::CloseOverlay, window, cx);
        }
        if let Some(view) = control["view"].as_str() {
            self.act(Action::View(view.into()), window, cx);
        }
        if let Some(device) = control["device"].as_str() {
            self.act(Action::Device(device.into()), window, cx);
        }
        if let Some(kind) = control["deviceKind"].as_str() {
            let device = match kind {
                "cpu" | "memory" => Some(kind.to_owned()),
                "gpu" => self.snapshot["hardware"]["gpus"][0]["info"]["id"]
                    .as_str()
                    .map(|id| format!("gpu:{id}")),
                "disk" => self.snapshot["hardware"]["disks"][0]["info"]["index"]
                    .as_u64()
                    .map(|id| format!("disk:{id}")),
                "network" => self.snapshot["hardware"]["nics"][0]["info"]["id"]
                    .as_str()
                    .map(|id| format!("nic:{id}")),
                _ => None,
            };
            if let Some(device) = device {
                self.act(Action::Device(device), window, cx);
            }
        }
        if let Some(mode) = control["cpuMode"].as_str() {
            self.act(Action::CpuMode(mode.into()), window, cx);
        }
        if !control["details"].is_null() {
            self.details_mode = DetailsMode::parse(&control["details"]);
        }
        if let Some(dialog) = control["dialog"].as_str() {
            self.open_dialog(dialog, control["args"].clone(), window, cx);
        }
        if let Some(theme) = control["theme"].as_str() {
            self.request(
                "set_theme",
                json!({"theme":theme}),
                Some("settings"),
                window,
                cx,
            );
        }
        cx.notify();
    }
}
