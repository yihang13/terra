//! Thin, purpose-built WebGL2 layer for the globe.

use std::collections::HashMap;

use js_sys::{Float32Array, Uint32Array};
use wasm_bindgen::{JsCast, JsValue};
use web_sys::{
    HtmlImageElement, WebGl2RenderingContext as Gl, WebGlBuffer, WebGlProgram, WebGlShader,
    WebGlTexture, WebGlUniformLocation, WebGlVertexArrayObject,
};

pub struct Program {
    id: WebGlProgram,
    uniforms: HashMap<String, Option<WebGlUniformLocation>>,
}

impl Program {
    pub fn new(gl: &Gl, vertex_src: &str, fragment_src: &str) -> Result<Self, JsValue> {
        let vs = compile(gl, Gl::VERTEX_SHADER, vertex_src)?;
        let fs = compile(gl, Gl::FRAGMENT_SHADER, fragment_src)?;

        let id = gl
            .create_program()
            .ok_or_else(|| JsValue::from_str("Unable to create WebGL program"))?;
        gl.attach_shader(&id, &vs);
        gl.attach_shader(&id, &fs);
        gl.link_program(&id);

        if !gl
            .get_program_parameter(&id, Gl::LINK_STATUS)
            .as_bool()
            .unwrap_or(false)
        {
            let log = gl
                .get_program_info_log(&id)
                .unwrap_or_else(|| "unknown link error".to_owned());
            gl.delete_shader(Some(&vs));
            gl.delete_shader(Some(&fs));
            gl.delete_program(Some(&id));
            return Err(JsValue::from_str(&format!("Shader link failed: {log}")));
        }

        gl.detach_shader(&id, &vs);
        gl.detach_shader(&id, &fs);
        gl.delete_shader(Some(&vs));
        gl.delete_shader(Some(&fs));

        let mut uniforms = HashMap::new();
        for name in [
            "u_cam_right",
            "u_cam_up",
            "u_cam_forward",
            "u_tan_half_fov",
            "u_aspect",
            "u_sun_dir",
            "u_sky",
            "u_time",
            "u_view_proj",
            "u_model",
            "u_camera_pos",
            "u_cloud_offset",
            "u_cloud_opacity",
            "u_night_intensity",
            "u_normal_strength",
            "u_strength",
            "u_screen_center",
            "u_earth_radius_px",
            "u_day",
            "u_night",
            "u_water",
            "u_clouds",
            "u_normal",
        ] {
            uniforms.insert(name.to_owned(), gl.get_uniform_location(&id, name));
        }

        Ok(Self { id, uniforms })
    }

    pub fn bind(&self, gl: &Gl) {
        gl.use_program(Some(&self.id));
    }

    fn loc(&self, name: &str) -> Option<&WebGlUniformLocation> {
        self.uniforms.get(name).and_then(|v| v.as_ref())
    }

    pub fn set_i32(&self, gl: &Gl, name: &str, value: i32) {
        gl.uniform1i(self.loc(name), value);
    }

    pub fn set_f32(&self, gl: &Gl, name: &str, value: f32) {
        gl.uniform1f(self.loc(name), value);
    }

    pub fn set_vec3(&self, gl: &Gl, name: &str, value: [f32; 3]) {
        gl.uniform3f(self.loc(name), value[0], value[1], value[2]);
    }

    pub fn set_vec2(&self, gl: &Gl, name: &str, value: [f32; 2]) {
        gl.uniform2f(self.loc(name), value[0], value[1]);
    }

    pub fn set_mat4(&self, gl: &Gl, name: &str, value: &[f32; 16]) {
        gl.uniform_matrix4fv_with_f32_array(self.loc(name), false, value);
    }

    pub fn set_texture(&self, gl: &Gl, name: &str, unit: u32, texture: &WebGlTexture) {
        gl.active_texture(Gl::TEXTURE0 + unit);
        gl.bind_texture(Gl::TEXTURE_2D, Some(texture));
        self.set_i32(gl, name, unit as i32);
    }
}

fn compile(gl: &Gl, kind: u32, source: &str) -> Result<WebGlShader, JsValue> {
    let shader = gl
        .create_shader(kind)
        .ok_or_else(|| JsValue::from_str("Unable to create shader"))?;
    gl.shader_source(&shader, source);
    gl.compile_shader(&shader);
    if !gl
        .get_shader_parameter(&shader, Gl::COMPILE_STATUS)
        .as_bool()
        .unwrap_or(false)
    {
        let log = gl
            .get_shader_info_log(&shader)
            .unwrap_or_else(|| "unknown compile error".to_owned());
        gl.delete_shader(Some(&shader));
        return Err(JsValue::from_str(&format!("Shader compile failed: {log}")));
    }
    Ok(shader)
}

pub struct Mesh {
    vao: WebGlVertexArrayObject,
    // Kept alive for the lifetime of the VAO.
    #[allow(dead_code)]
    buffer: WebGlBuffer,
    #[allow(dead_code)]
    index_buffer: WebGlBuffer,
    index_count: i32,
}

impl Mesh {
    /// A UV sphere whose texture seam is on the anti-meridian and whose
    /// prime meridian points toward -X (matching the supplied Blue Marble map).
    pub fn uv_sphere(gl: &Gl, segments: u32, rings: u32) -> Result<Self, JsValue> {
        let mut vertices: Vec<f32> =
            Vec::with_capacity(((segments + 1) * (rings + 1) * 11) as usize);
        let mut indices: Vec<u32> = Vec::with_capacity((segments * rings * 6) as usize);

        for i in 0..=rings {
            let v = i as f32 / rings as f32;
            let lat = (v - 0.5) * std::f32::consts::PI;
            let (lat_sin, lat_cos) = lat.sin_cos();

            for j in 0..=segments {
                let u = j as f32 / segments as f32;
                let lon = (u - 0.5) * std::f32::consts::TAU;
                let (lon_sin, lon_cos) = lon.sin_cos();

                let x = -lon_cos * lat_cos;
                let y = lat_sin;
                let z = lon_sin * lat_cos;

                vertices.extend_from_slice(&[
                    x, y, z, // position
                    x, y, z, // normal
                    u, v, // uv
                    lon_sin, 0.0, lon_cos, // tangent (east)
                ]);
            }
        }

        for i in 0..rings {
            for j in 0..segments {
                let a = i * (segments + 1) + j;
                let b = (i + 1) * (segments + 1) + j;
                let c = a + 1;
                let d = b + 1;
                indices.extend_from_slice(&[a, c, b, c, d, b]);
            }
        }

        let vao = gl
            .create_vertex_array()
            .ok_or_else(|| JsValue::from_str("Unable to create VAO"))?;
        gl.bind_vertex_array(Some(&vao));

        let buffer = gl
            .create_buffer()
            .ok_or_else(|| JsValue::from_str("Unable to create VBO"))?;
        gl.bind_buffer(Gl::ARRAY_BUFFER, Some(&buffer));
        let vertices_js = unsafe { Float32Array::view(&vertices) };
        gl.buffer_data_with_array_buffer_view(Gl::ARRAY_BUFFER, &vertices_js, Gl::STATIC_DRAW);

        let index_buffer = gl
            .create_buffer()
            .ok_or_else(|| JsValue::from_str("Unable to create EBO"))?;
        gl.bind_buffer(Gl::ELEMENT_ARRAY_BUFFER, Some(&index_buffer));
        let indices_js = Uint32Array::new_with_length(indices.len() as u32);
        indices_js.copy_from(&indices);
        gl.buffer_data_with_array_buffer_view(
            Gl::ELEMENT_ARRAY_BUFFER,
            &indices_js,
            Gl::STATIC_DRAW,
        );

        let stride = 11 * 4;
        for (location, size, offset) in [(0, 3, 0), (1, 3, 3 * 4), (2, 2, 6 * 4), (3, 3, 8 * 4)] {
            gl.enable_vertex_attrib_array(location);
            gl.vertex_attrib_pointer_with_i32(location, size, Gl::FLOAT, false, stride, offset);
        }

        gl.bind_vertex_array(None);

        Ok(Self {
            vao,
            buffer,
            index_buffer,
            index_count: indices.len() as i32,
        })
    }

    pub fn draw(&self, gl: &Gl) {
        gl.bind_vertex_array(Some(&self.vao));
        gl.draw_elements_with_i32(Gl::TRIANGLES, self.index_count, Gl::UNSIGNED_INT, 0);
        gl.bind_vertex_array(None);
    }
}

pub struct Texture {
    pub id: WebGlTexture,
}

impl Texture {
    pub fn from_image(
        gl: &Gl,
        image: &HtmlImageElement,
        srgb: bool,
        wrap_s_repeat: bool,
    ) -> Result<Self, JsValue> {
        let texture = gl
            .create_texture()
            .ok_or_else(|| JsValue::from_str("Unable to create texture"))?;
        gl.bind_texture(Gl::TEXTURE_2D, Some(&texture));
        gl.pixel_storei(Gl::UNPACK_FLIP_Y_WEBGL, 1);
        gl.pixel_storei(Gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL, 0);

        let internal = if srgb { Gl::SRGB8_ALPHA8 } else { Gl::RGBA8 };
        gl.tex_image_2d_with_u32_and_u32_and_html_image_element(
            Gl::TEXTURE_2D,
            0,
            internal as i32,
            Gl::RGBA,
            Gl::UNSIGNED_BYTE,
            image,
        )?;

        gl.tex_parameteri(
            Gl::TEXTURE_2D,
            Gl::TEXTURE_MIN_FILTER,
            Gl::LINEAR_MIPMAP_LINEAR as i32,
        );
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_MAG_FILTER, Gl::LINEAR as i32);
        gl.tex_parameteri(
            Gl::TEXTURE_2D,
            Gl::TEXTURE_WRAP_S,
            if wrap_s_repeat {
                Gl::REPEAT
            } else {
                Gl::CLAMP_TO_EDGE
            } as i32,
        );
        gl.tex_parameteri(Gl::TEXTURE_2D, Gl::TEXTURE_WRAP_T, Gl::CLAMP_TO_EDGE as i32);
        gl.generate_mipmap(Gl::TEXTURE_2D);

        Ok(Self { id: texture })
    }
}

pub struct Textures {
    pub day: Texture,
    pub night: Texture,
    pub water: Texture,
    pub clouds: Texture,
    pub normal: Texture,
    pub sky: Texture,
}

pub struct EarthRenderer {
    pub gl: Gl,
    pub background: Program,
    pub earth: Program,
    pub atmosphere: Program,
    pub mesh: Mesh,
    pub textures: Textures,
}

pub struct FrameParams {
    pub view_proj: [f32; 16],
    pub model: [f32; 16],
    pub camera_pos: [f32; 3],
    pub camera_right: [f32; 3],
    pub camera_up: [f32; 3],
    pub camera_forward: [f32; 3],
    pub tan_half_fov: f32,
    pub aspect: f32,
    pub sun_dir: [f32; 3],
    pub cloud_offset: f32,
    pub cloud_opacity: f32,
    pub night_intensity: f32,
    pub time: f32,
    pub screen_center: [f32; 2],
    pub earth_radius_px: f32,
}

impl EarthRenderer {
    pub fn new(
        gl: Gl,
        textures: Textures,
        shaders: (&str, &str, &str, &str, &str, &str),
    ) -> Result<Self, JsValue> {
        let background = Program::new(&gl, shaders.0, shaders.1)?;
        let earth = Program::new(&gl, shaders.2, shaders.3)?;
        let atmosphere = Program::new(&gl, shaders.4, shaders.5)?;
        let mesh = Mesh::uv_sphere(&gl, 192, 96)?;
        Ok(Self {
            gl,
            background,
            earth,
            atmosphere,
            mesh,
            textures,
        })
    }

    pub fn render(&self, p: &FrameParams) {
        let gl = &self.gl;

        gl.viewport(0, 0, gl.drawing_buffer_width(), gl.drawing_buffer_height());
        gl.clear_color(0.001, 0.003, 0.008, 1.0);
        gl.clear(Gl::COLOR_BUFFER_BIT | Gl::DEPTH_BUFFER_BIT);

        // 1. Deep space.
        gl.disable(Gl::DEPTH_TEST);
        gl.disable(Gl::BLEND);
        gl.disable(Gl::CULL_FACE);
        self.background.bind(gl);
        self.background.set_vec3(gl, "u_cam_right", p.camera_right);
        self.background.set_vec3(gl, "u_cam_up", p.camera_up);
        self.background
            .set_vec3(gl, "u_cam_forward", p.camera_forward);
        self.background
            .set_f32(gl, "u_tan_half_fov", p.tan_half_fov);
        self.background.set_f32(gl, "u_aspect", p.aspect);
        self.background.set_vec3(gl, "u_sun_dir", p.sun_dir);
        self.background.set_f32(gl, "u_time", p.time);
        self.background
            .set_texture(gl, "u_sky", 0, &self.textures.sky.id);
        gl.bind_vertex_array(None);
        gl.draw_arrays(Gl::TRIANGLES, 0, 3);

        // 2. The globe.
        gl.enable(Gl::DEPTH_TEST);
        gl.depth_func(Gl::LEQUAL);
        gl.depth_mask(true);
        gl.enable(Gl::CULL_FACE);
        gl.cull_face(Gl::BACK);
        gl.front_face(Gl::CCW);

        self.earth.bind(gl);
        self.earth.set_mat4(gl, "u_view_proj", &p.view_proj);
        self.earth.set_mat4(gl, "u_model", &p.model);
        self.earth.set_vec3(gl, "u_camera_pos", p.camera_pos);
        self.earth.set_vec3(gl, "u_sun_dir", p.sun_dir);
        self.earth.set_f32(gl, "u_cloud_offset", p.cloud_offset);
        self.earth.set_f32(gl, "u_cloud_opacity", p.cloud_opacity);
        self.earth
            .set_f32(gl, "u_night_intensity", p.night_intensity);
        self.earth.set_f32(gl, "u_normal_strength", 0.55);
        self.earth
            .set_texture(gl, "u_day", 1, &self.textures.day.id);
        self.earth
            .set_texture(gl, "u_night", 2, &self.textures.night.id);
        self.earth
            .set_texture(gl, "u_water", 3, &self.textures.water.id);
        self.earth
            .set_texture(gl, "u_clouds", 4, &self.textures.clouds.id);
        self.earth
            .set_texture(gl, "u_normal", 5, &self.textures.normal.id);
        self.mesh.draw(gl);

        // 3. Additive atmospheric shell. Render its far side so only the halo
        //    beyond the planet's silhouette remains visible.
        gl.enable(Gl::BLEND);
        gl.blend_func(Gl::ONE, Gl::ONE);
        gl.depth_mask(false);
        gl.cull_face(Gl::FRONT);

        let atmosphere_model = {
            let rot = [
                p.model[0],
                p.model[1],
                p.model[2],
                p.model[3],
                p.model[4],
                p.model[5],
                p.model[6],
                p.model[7],
                p.model[8],
                p.model[9],
                p.model[10],
                p.model[11],
                p.model[12],
                p.model[13],
                p.model[14],
                p.model[15],
            ];
            let mut m = rot;
            for i in 0..3 {
                for j in 0..3 {
                    m[i + j * 4] *= 1.055;
                }
            }
            m
        };

        self.atmosphere.bind(gl);
        self.atmosphere.set_mat4(gl, "u_view_proj", &p.view_proj);
        self.atmosphere.set_mat4(gl, "u_model", &atmosphere_model);
        self.atmosphere.set_vec3(gl, "u_camera_pos", p.camera_pos);
        self.atmosphere.set_vec3(gl, "u_sun_dir", p.sun_dir);
        self.atmosphere.set_f32(gl, "u_strength", 0.92);
        self.atmosphere
            .set_vec2(gl, "u_screen_center", p.screen_center);
        self.atmosphere
            .set_f32(gl, "u_earth_radius_px", p.earth_radius_px);
        self.mesh.draw(gl);

        gl.depth_mask(true);
        gl.disable(Gl::BLEND);
        gl.cull_face(Gl::BACK);
        gl.bind_vertex_array(None);
    }

    pub fn resize(&self, width: u32, height: u32, dpr: f64) {
        let physical_w = ((width as f64 * dpr).round() as u32).max(1);
        let physical_h = ((height as f64 * dpr).round() as u32).max(1);
        let canvas = self
            .gl
            .canvas()
            .and_then(|c| c.dyn_into::<web_sys::HtmlCanvasElement>().ok());
        if let Some(canvas) = canvas {
            if canvas.width() != physical_w {
                canvas.set_width(physical_w);
            }
            if canvas.height() != physical_h {
                canvas.set_height(physical_h);
            }
        }
    }
}
