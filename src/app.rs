//! Application shell: asset loading, camera, input and HUD.

use std::cell::RefCell;
use std::collections::HashMap;
use std::f32::consts::{PI, TAU};
use std::rc::Rc;

use wasm_bindgen::closure::Closure;
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;
use web_sys::{
    Document, Event, HtmlCanvasElement, HtmlImageElement, KeyboardEvent, PointerEvent,
    WebGl2RenderingContext as Gl, WheelEvent, Window,
};

use crate::gl::{EarthRenderer, FrameParams, Texture, Textures};
use crate::math::{Mat4, Vec3};
use crate::shaders::{
    ATMOSPHERE_FS, ATMOSPHERE_VS, BACKGROUND_FS, BACKGROUND_VS, EARTH_FS, EARTH_VS,
};

const INITIAL_YAW: f32 = 0.30;
const INITIAL_PITCH: f32 = 0.16;
const INITIAL_DISTANCE: f32 = 3.75;
const MIN_DISTANCE: f32 = 1.78;
const MAX_DISTANCE: f32 = 7.6;
const FOV_Y: f32 = 40.0;

const SUN_DIRECTION: Vec3 = Vec3::new(-0.43, 0.27, 0.86);

pub struct App {
    window: Window,
    document: Document,
    canvas: HtmlCanvasElement,
    renderer: EarthRenderer,

    yaw: f32,
    pitch: f32,
    target_yaw: f32,
    target_pitch: f32,
    yaw_velocity: f32,
    pitch_velocity: f32,
    distance: f32,
    target_distance: f32,

    spin: f32,
    spin_on: bool,
    clouds_on: bool,
    cloud_opacity: f32,
    night_on: bool,
    night_intensity: f32,
    cloud_offset: f32,
    sun_dir: Vec3,

    pointers: HashMap<i32, (f64, f64)>,
    last_pointer: Option<(f64, f64)>,
    pinch_start_span: f64,
    pinch_start_distance: f32,

    last_frame: f64,
    fps_accum: f64,
    fps_frames: u32,
    hud_accum: f64,
    clock_accum: f64,
    elapsed: f32,

    width: u32,
    height: u32,
    dpr: f64,
    needs_resize: bool,
}

impl App {
    pub fn frame(&mut self, now_ms: f64) {
        if self.last_frame == 0.0 {
            self.last_frame = now_ms;
        }
        let raw_dt = (now_ms - self.last_frame) / 1000.0;
        let dt = raw_dt.clamp(0.0, 0.05) as f32;
        self.last_frame = now_ms;
        self.elapsed += dt;

        if self.needs_resize {
            self.perform_resize();
        }

        if self.spin_on {
            self.spin = (self.spin + dt * 0.024).rem_euclid(TAU);
        }
        self.cloud_offset = (self.cloud_offset + dt * 0.00048).rem_euclid(1.0);

        let cloud_target = if self.clouds_on { 1.0 } else { 0.0 };
        self.cloud_opacity += (cloud_target - self.cloud_opacity) * (1.0 - (-dt * 7.5).exp());
        let night_target = if self.night_on { 1.0 } else { 0.0 };
        self.night_intensity += (night_target - self.night_intensity) * (1.0 - (-dt * 7.5).exp());

        // Inertial orbit after the pointer is released.
        if self.pointers.is_empty() {
            self.target_yaw += self.yaw_velocity;
            self.target_pitch = (self.target_pitch + self.pitch_velocity).clamp(-1.18, 1.18);
            let decay = (-dt * 3.0).exp();
            self.yaw_velocity *= decay;
            self.pitch_velocity *= decay;
        }

        let orbit_ease = 1.0 - (-dt * 15.0).exp();
        self.yaw += (self.target_yaw - self.yaw) * orbit_ease;
        self.pitch += (self.target_pitch - self.pitch) * orbit_ease;
        let zoom_ease = 1.0 - (-dt * 6.0).exp();
        self.distance += (self.target_distance - self.distance) * zoom_ease;

        let eye = self.eye();
        let forward = (-eye).normalize();
        let right = forward.cross(Vec3::new(0.0, 1.0, 0.0)).normalize();
        let up = right.cross(forward);
        let aspect = self.width.max(1) as f32 / self.height.max(1) as f32;
        // Portrait screens need a wider vertical field of view so the whole
        // planet fits across the narrow dimension at the default zoom.
        let base_tan = (FOV_Y.to_radians() * 0.5).tan();
        let initial_radius_tan = 1.0 / (INITIAL_DISTANCE * INITIAL_DISTANCE - 1.0).sqrt();
        let fit_tan = initial_radius_tan / (0.86 * aspect.max(0.1));
        let tan_half_fov = base_tan.max(fit_tan);
        let fov_y = 2.0 * tan_half_fov.atan();
        let view = Mat4::look_at(eye, Vec3::ZERO, Vec3::new(0.0, 1.0, 0.0));
        let proj = Mat4::perspective(fov_y, aspect, 0.05, 30.0);
        let view_proj = proj.mul(view);
        let earth_radius_tan = 1.0 / (self.distance * self.distance - 1.0).max(0.01).sqrt();
        let focal_px = (self.height.max(1) as f32 * 0.5) / tan_half_fov * self.dpr as f32;
        let earth_radius_px = (focal_px * earth_radius_tan).max(1.0);
        let screen_center = [
            self.width.max(1) as f32 * 0.5 * self.dpr as f32,
            self.height.max(1) as f32 * 0.5 * self.dpr as f32,
        ];
        let model = Mat4::rotation_y(self.spin);

        let params = FrameParams {
            view_proj: view_proj.m,
            model: model.m,
            camera_pos: eye.to_array(),
            camera_right: right.to_array(),
            camera_up: up.to_array(),
            camera_forward: forward.to_array(),
            tan_half_fov,
            aspect,
            sun_dir: self.sun_dir.to_array(),
            cloud_offset: self.cloud_offset,
            cloud_opacity: self.cloud_opacity,
            night_intensity: self.night_intensity,
            time: self.elapsed,
            screen_center,
            earth_radius_px,
        };
        self.renderer.render(&params);

        self.fps_frames += 1;
        self.fps_accum += dt as f64;
        self.hud_accum += dt as f64;
        self.clock_accum += dt as f64;

        if self.hud_accum >= 0.12 {
            self.hud_accum = 0.0;
            self.update_orbital_hud(&eye);
        }
        if self.fps_accum >= 0.7 {
            let fps = self.fps_frames as f64 / self.fps_accum.max(0.001);
            set_text(&self.document, "hud-fps", &format!("{fps:.0}"));
            self.fps_accum = 0.0;
            self.fps_frames = 0;
        }
        if self.clock_accum >= 1.0 {
            self.clock_accum = 0.0;
            self.update_clock();
        }
    }

    fn eye(&self) -> Vec3 {
        let cos_pitch = self.pitch.cos();
        Vec3::new(
            self.distance * cos_pitch * self.yaw.sin(),
            self.distance * self.pitch.sin(),
            self.distance * cos_pitch * self.yaw.cos(),
        )
    }

    fn perform_resize(&mut self) {
        self.width = self
            .window
            .inner_width()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as u32;
        self.height = self
            .window
            .inner_height()
            .ok()
            .and_then(|v| v.as_f64())
            .unwrap_or(1.0) as u32;
        self.dpr = self.window.device_pixel_ratio().clamp(1.0, 2.0);
        self.renderer.resize(self.width, self.height, self.dpr);
        self.needs_resize = false;
    }

    fn update_orbital_hud(&self, eye: &Vec3) {
        let center = eye.normalize();
        // Undo the globe's own spin to find the map coordinate under the camera.
        let (s, c) = self.spin.sin_cos();
        let base_x = c * center.x - s * center.z;
        let base_z = s * center.x + c * center.z;
        let base_y = center.y;

        let lat = base_y.clamp(-1.0, 1.0).asin().to_degrees();
        let lon = base_z.atan2(-base_x).to_degrees();
        let lat_h = if lat >= 0.0 { "N" } else { "S" };
        let lon_h = if lon >= 0.0 { "E" } else { "W" };
        set_text(
            &self.document,
            "hud-lat",
            &format!("{:.1}° {}", lat.abs(), lat_h),
        );
        set_text(
            &self.document,
            "hud-lon",
            &format!("{:.1}° {}", lon.abs(), lon_h),
        );
        let altitude = (self.distance - 1.0).max(0.0) * 6371.0;
        let altitude_text = format_km(altitude);
        set_text(&self.document, "hud-alt", &altitude_text);

        set_text(
            &self.document,
            "m-lat",
            &format!("{:.1}° {}", lat.abs(), lat_h),
        );
        set_text(
            &self.document,
            "m-lon",
            &format!("{:.1}° {}", lon.abs(), lon_h),
        );
        set_text(&self.document, "m-alt", &altitude_text);

        let sun_dot = center.dot(self.sun_dir);
        let phase = if sun_dot > 0.42 {
            "DAYLIGHT"
        } else if sun_dot > -0.20 {
            "TERMINATOR"
        } else {
            "NIGHT SIDE"
        };
        set_text(&self.document, "hud-phase", phase);
    }

    fn update_clock(&self) {
        let now = js_sys::Date::new_0();
        let h = now.get_utc_hours();
        let m = now.get_utc_minutes();
        let s = now.get_utc_seconds();
        set_text(
            &self.document,
            "utc-clock",
            &format!("{h:02}:{m:02}:{s:02} UTC"),
        );
        const MONTHS: [&str; 12] = [
            "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
        ];
        let month = MONTHS[(now.get_utc_month() as usize).min(11)];
        set_text(
            &self.document,
            "utc-date",
            &format!(
                "{:02} {} {}",
                now.get_utc_date(),
                month,
                now.get_utc_full_year()
            ),
        );
    }

    fn sync_controls(&self) {
        set_button_active(&self.document, "spin", self.spin_on);
        set_button_active(&self.document, "clouds", self.clouds_on);
        set_button_active(&self.document, "night", self.night_on);
    }

    fn reset_view(&mut self) {
        self.target_yaw = self.yaw + shortest_angle(self.yaw, INITIAL_YAW);
        self.target_pitch = INITIAL_PITCH;
        self.target_distance = INITIAL_DISTANCE;
        self.yaw_velocity = 0.0;
        self.pitch_velocity = 0.0;
    }

    fn toggle_fullscreen(&self) {
        if self.document.fullscreen_element().is_some() {
            self.document.exit_fullscreen();
        } else if let Some(root) = self.document.document_element() {
            let _ = root.request_fullscreen();
        }
    }

    fn set_dragging_class(&self, dragging: bool) {
        let cl = self.canvas.class_list();
        if dragging {
            let _ = cl.add_1("is-dragging");
        } else {
            let _ = cl.remove_1("is-dragging");
        }
    }
}

pub async fn create(gl: Gl) -> Result<Rc<RefCell<App>>, JsValue> {
    let window = web_sys::window().ok_or_else(|| JsValue::from_str("No browser window"))?;
    let document = window
        .document()
        .ok_or_else(|| JsValue::from_str("No document available"))?;
    let canvas = gl
        .canvas()
        .ok_or_else(|| JsValue::from_str("No canvas attached to WebGL context"))?
        .dyn_into::<HtmlCanvasElement>()?;

    set_loader(6, "ESTABLISHING ORBITAL LINK");
    let day_image = load_image(&document, "assets/earth-day.jpg").await?;
    set_loader(20, "RESOLVING CONTINENTAL IMAGERY");
    let night_image = load_image(&document, "assets/earth-night.jpg").await?;
    set_loader(34, "MAPPING CITY LIGHTS");
    let water_image = load_image(&document, "assets/earth-water.jpg").await?;
    set_loader(46, "READING OCEAN MASK");
    let clouds_image = load_image(&document, "assets/earth-clouds.png").await?;
    set_loader(58, "SEPARATING CLOUD LAYERS");
    let normal_image = load_image(&document, "assets/earth-normal.jpg").await?;
    set_loader(70, "SAMPLING TERRAIN RELIEF");
    let sky_image = load_image(&document, "assets/night-sky.png").await?;
    set_loader(82, "CATALOGUING DEEP SPACE");

    let textures = Textures {
        day: Texture::from_image(&gl, &day_image, true, true)?,
        night: Texture::from_image(&gl, &night_image, true, true)?,
        water: Texture::from_image(&gl, &water_image, false, true)?,
        clouds: Texture::from_image(&gl, &clouds_image, true, true)?,
        normal: Texture::from_image(&gl, &normal_image, false, true)?,
        sky: Texture::from_image(&gl, &sky_image, true, true)?,
    };

    set_loader(90, "COMPILING ORBITAL SHADERS");
    let renderer = EarthRenderer::new(
        gl,
        textures,
        (
            BACKGROUND_VS,
            BACKGROUND_FS,
            EARTH_VS,
            EARTH_FS,
            ATMOSPHERE_VS,
            ATMOSPHERE_FS,
        ),
    )?;

    set_loader(97, "CALIBRATING ATTITUDE");
    let width = window
        .inner_width()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as u32;
    let height = window
        .inner_height()
        .ok()
        .and_then(|v| v.as_f64())
        .unwrap_or(1.0) as u32;
    let dpr = window.device_pixel_ratio().clamp(1.0, 2.0);
    renderer.resize(width, height, dpr);

    let mut app = App {
        window,
        document,
        canvas,
        renderer,
        yaw: INITIAL_YAW - 0.22,
        pitch: INITIAL_PITCH - 0.08,
        target_yaw: INITIAL_YAW,
        target_pitch: INITIAL_PITCH,
        yaw_velocity: 0.0,
        pitch_velocity: 0.0,
        distance: 5.25,
        target_distance: INITIAL_DISTANCE,
        spin: 0.0,
        spin_on: true,
        clouds_on: true,
        cloud_opacity: 0.0,
        night_on: true,
        night_intensity: 0.0,
        cloud_offset: 0.0,
        sun_dir: SUN_DIRECTION.normalize(),
        pointers: HashMap::new(),
        last_pointer: None,
        pinch_start_span: 0.0,
        pinch_start_distance: INITIAL_DISTANCE,
        last_frame: 0.0,
        fps_accum: 0.0,
        fps_frames: 0,
        hud_accum: 0.0,
        clock_accum: 0.0,
        elapsed: 0.0,
        width,
        height,
        dpr,
        needs_resize: false,
    };
    app.clock_accum = 1.0;
    app.update_clock();
    app.update_orbital_hud(&app.eye());
    app.sync_controls();

    let app = Rc::new(RefCell::new(app));
    attach_events(&app)?;
    set_loader(100, "ORBIT ACQUIRED");
    Ok(app)
}

pub fn begin(app: Rc<RefCell<App>>) {
    hide_loader();
    let window = app.borrow().window.clone();
    let window_for_loop = window.clone();
    let loop_ref: Rc<RefCell<Option<Closure<dyn FnMut(f64)>>>> = Rc::new(RefCell::new(None));
    let loop_for_callback = loop_ref.clone();
    let app_for_loop = app.clone();

    *loop_ref.borrow_mut() = Some(Closure::wrap(Box::new(move |time: f64| {
        app_for_loop.borrow_mut().frame(time);
        if let Some(cb) = loop_for_callback.borrow().as_ref() {
            let _ = window_for_loop.request_animation_frame(cb.as_ref().unchecked_ref());
        }
    }) as Box<dyn FnMut(f64)>));

    let initial = loop_ref.borrow();
    if let Some(cb) = initial.as_ref() {
        let _ = window.request_animation_frame(cb.as_ref().unchecked_ref());
    }
}

fn attach_events(app: &Rc<RefCell<App>>) -> Result<(), JsValue> {
    attach_pointer_events(app)?;
    attach_wheel_event(app)?;
    attach_button_events(app)?;

    let app_key = app.clone();
    let key_cb =
        Closure::<dyn FnMut(KeyboardEvent)>::new(move |event: KeyboardEvent| {
            match event.key().to_ascii_lowercase().as_str() {
                " " => {
                    event.prevent_default();
                    toggle_action(&app_key, "spin");
                }
                "c" => toggle_action(&app_key, "clouds"),
                "n" => toggle_action(&app_key, "night"),
                "r" => toggle_action(&app_key, "reset"),
                "f" => toggle_action(&app_key, "fullscreen"),
                _ => {}
            }
        });
    app.borrow()
        .window
        .add_event_listener_with_callback("keydown", key_cb.as_ref().unchecked_ref())?;
    key_cb.forget();

    let app_resize = app.clone();
    let resize_cb = Closure::<dyn FnMut(Event)>::new(move |_| {
        app_resize.borrow_mut().needs_resize = true;
    });
    app.borrow()
        .window
        .add_event_listener_with_callback("resize", resize_cb.as_ref().unchecked_ref())?;
    resize_cb.forget();

    Ok(())
}

fn attach_pointer_events(app: &Rc<RefCell<App>>) -> Result<(), JsValue> {
    let canvas = app.borrow().canvas.clone();

    let app_down = app.clone();
    let canvas_down = canvas.clone();
    let down_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
        event.prevent_default();
        let mut state = app_down.borrow_mut();
        state.pointers.insert(
            event.pointer_id(),
            (event.client_x() as f64, event.client_y() as f64),
        );
        if state.pointers.len() == 1 {
            state.last_pointer = Some((event.client_x() as f64, event.client_y() as f64));
            state.yaw_velocity = 0.0;
            state.pitch_velocity = 0.0;
            state.set_dragging_class(true);
        } else if state.pointers.len() == 2 {
            state.pinch_start_span = pointer_span(&state.pointers);
            state.pinch_start_distance = state.target_distance;
            state.yaw_velocity = 0.0;
            state.pitch_velocity = 0.0;
        }
        let _ = canvas_down.set_pointer_capture(event.pointer_id());
    });
    canvas.add_event_listener_with_callback("pointerdown", down_cb.as_ref().unchecked_ref())?;
    down_cb.forget();

    let app_move = app.clone();
    let move_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
        let mut state = app_move.borrow_mut();
        let id = event.pointer_id();
        if !state.pointers.contains_key(&id) {
            return;
        }
        state
            .pointers
            .insert(id, (event.client_x() as f64, event.client_y() as f64));

        if state.pointers.len() >= 2 {
            let span = pointer_span(&state.pointers);
            if state.pinch_start_span > 1.0 && span > 1.0 {
                state.target_distance = (state.pinch_start_distance
                    * (state.pinch_start_span / span) as f32)
                    .clamp(MIN_DISTANCE, MAX_DISTANCE);
            }
            state.last_pointer = None;
            return;
        }

        if let Some((last_x, last_y)) = state.last_pointer {
            let dx = event.client_x() as f64 - last_x;
            let dy = event.client_y() as f64 - last_y;
            let yaw_delta = -(dx as f32) * 0.0045;
            let pitch_delta = (dy as f32) * 0.0034;
            state.target_yaw += yaw_delta;
            state.target_pitch = (state.target_pitch + pitch_delta).clamp(-1.18, 1.18);
            state.yaw_velocity = yaw_delta;
            state.pitch_velocity = pitch_delta;
        }
        state.last_pointer = Some((event.client_x() as f64, event.client_y() as f64));
    });
    canvas.add_event_listener_with_callback("pointermove", move_cb.as_ref().unchecked_ref())?;
    move_cb.forget();

    let app_up = app.clone();
    let up_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
        let mut state = app_up.borrow_mut();
        state.pointers.remove(&event.pointer_id());
        if state.pointers.is_empty() {
            state.last_pointer = None;
            state.set_dragging_class(false);
        } else {
            state.pinch_start_span = 0.0;
            if let Some((_, &(x, y))) = state.pointers.iter().next() {
                state.last_pointer = Some((x, y));
            }
        }
    });
    canvas.add_event_listener_with_callback("pointerup", up_cb.as_ref().unchecked_ref())?;
    up_cb.forget();

    let app_cancel = app.clone();
    let cancel_cb = Closure::<dyn FnMut(PointerEvent)>::new(move |event: PointerEvent| {
        let mut state = app_cancel.borrow_mut();
        state.pointers.remove(&event.pointer_id());
        if state.pointers.is_empty() {
            state.last_pointer = None;
            state.set_dragging_class(false);
        }
    });
    canvas.add_event_listener_with_callback("pointercancel", cancel_cb.as_ref().unchecked_ref())?;
    cancel_cb.forget();

    let app_double = app.clone();
    let double_cb = Closure::<dyn FnMut(Event)>::new(move |_| {
        toggle_action(&app_double, "reset");
    });
    canvas.add_event_listener_with_callback("dblclick", double_cb.as_ref().unchecked_ref())?;
    double_cb.forget();

    Ok(())
}

fn attach_wheel_event(app: &Rc<RefCell<App>>) -> Result<(), JsValue> {
    let canvas = app.borrow().canvas.clone();
    let app_wheel = app.clone();
    let cb = Closure::<dyn FnMut(WheelEvent)>::new(move |event: WheelEvent| {
        event.prevent_default();
        let mut state = app_wheel.borrow_mut();
        let factor = (1.0 + event.delta_y() as f32 * 0.0011).clamp(0.82, 1.22);
        state.target_distance = (state.target_distance * factor).clamp(MIN_DISTANCE, MAX_DISTANCE);
    });
    canvas.add_event_listener_with_callback("wheel", cb.as_ref().unchecked_ref())?;
    cb.forget();
    Ok(())
}

fn attach_button_events(app: &Rc<RefCell<App>>) -> Result<(), JsValue> {
    let document = app.borrow().document.clone();
    let nodes = document.query_selector_all("[data-action]")?;
    for index in 0..nodes.length() {
        let Some(node) = nodes.item(index) else {
            continue;
        };
        let element = node.dyn_into::<web_sys::Element>()?;
        let app_click = app.clone();
        let target = element.clone();
        let cb = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
            event.prevent_default();
            if let Some(action) = target.get_attribute("data-action") {
                toggle_action(&app_click, &action);
            }
        });
        element.add_event_listener_with_callback("click", cb.as_ref().unchecked_ref())?;
        cb.forget();
    }
    Ok(())
}

fn toggle_action(app: &Rc<RefCell<App>>, action: &str) {
    let mut state = app.borrow_mut();
    match action {
        "spin" => {
            state.spin_on = !state.spin_on;
            state.sync_controls();
        }
        "clouds" => {
            state.clouds_on = !state.clouds_on;
            state.sync_controls();
        }
        "night" => {
            state.night_on = !state.night_on;
            state.sync_controls();
        }
        "reset" => state.reset_view(),
        "fullscreen" => state.toggle_fullscreen(),
        _ => {}
    }
}

fn pointer_span(pointers: &HashMap<i32, (f64, f64)>) -> f64 {
    let mut iter = pointers.values();
    if let (Some(a), Some(b)) = (iter.next(), iter.next()) {
        let dx = a.0 - b.0;
        let dy = a.1 - b.1;
        (dx * dx + dy * dy).sqrt()
    } else {
        0.0
    }
}

fn shortest_angle(from: f32, to: f32) -> f32 {
    let mut delta = (to - from) % TAU;
    if delta > PI {
        delta -= TAU;
    } else if delta < -PI {
        delta += TAU;
    }
    delta
}

fn format_km(km: f32) -> String {
    let value = km.round().max(0.0) as u64;
    let digits = value.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3 + 3);
    for (i, ch) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(ch);
    }
    out.push_str(" KM");
    out
}

fn set_text(document: &Document, id: &str, value: &str) {
    if let Some(el) = document.get_element_by_id(id) {
        el.set_text_content(Some(value));
    }
}

fn set_button_active(document: &Document, action: &str, active: bool) {
    if let Ok(Some(element)) = document.query_selector(&format!("[data-action=\"{action}\"]")) {
        let classes = element.class_list();
        if active {
            let _ = classes.add_1("is-active");
        } else {
            let _ = classes.remove_1("is-active");
        }
        let _ = element.set_attribute("aria-pressed", if active { "true" } else { "false" });
    }
}

async fn load_image(document: &Document, path: &str) -> Result<HtmlImageElement, JsValue> {
    let image = document
        .create_element("img")?
        .dyn_into::<HtmlImageElement>()?;
    image.set_alt("");
    image.set_src(path);
    JsFuture::from(image.decode()).await?;
    Ok(image)
}

fn set_loader(percent: u32, label: &str) {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    set_text(&document, "loader-label", label);
    set_text(&document, "loader-percent", &format!("{percent:03}%"));
    if let Some(bar) = document.get_element_by_id("loader-bar") {
        let _ = bar.set_attribute("style", &format!("width:{percent}%"));
    }
}

fn hide_loader() {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(loader) = document.get_element_by_id("loader") {
            let _ = loader.class_list().add_1("is-hidden");
        }
    }
}

pub fn show_fatal(message: &str) {
    if let Some(document) = web_sys::window().and_then(|w| w.document()) {
        if let Some(error) = document.get_element_by_id("error") {
            let _ = error.remove_attribute("hidden");
        }
        if let Some(detail) = document.get_element_by_id("error-detail") {
            detail.set_text_content(Some(message));
        }
        if let Some(loader) = document.get_element_by_id("loader") {
            let _ = loader.class_list().add_1("is-hidden");
        }
    }
    web_sys::console::error_1(&JsValue::from_str(message));
}
