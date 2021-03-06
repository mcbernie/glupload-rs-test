// This example demostrates how to output GL textures, within an
// EGL/X11 context provided by the application, and render those
// textures in the GL application.

// {videotestsrc} - { glsinkbin }

#[macro_use]
extern crate gstreamer as gst;
use gst::prelude::*;

extern crate gstreamer_app as gst_app;
extern crate gstreamer_gl as gst_gl;
use gst_gl::prelude::*;
extern crate gstreamer_video as gst_video;

extern crate glib;

use std::error::Error as StdError;
use std::ffi::CStr;
use std::mem;
use std::ptr;
use std::sync::mpsc;

extern crate failure;
use failure::Error;

#[macro_use]
extern crate failure_derive;

extern crate glutin;

mod common;

#[derive(Debug, Fail)]
#[fail(display = "Missing element {}", _0)]
struct MissingElement(&'static str);

#[derive(Debug, Fail)]
#[fail(
    display = "Received error from {}: {} (debug: {:?})",
    src, error, debug
)]
struct ErrorMessage {
    src: String,
    error: String,
    debug: Option<String>,
    #[cause]
    cause: glib::Error,
}

#[rustfmt::skip]
static VERTICES: [f32; 20] = [
     1.0,  1.0, 0.0, 1.0, 0.0,
    -1.0,  1.0, 0.0, 0.0, 0.0,
    -1.0, -1.0, 0.0, 0.0, 1.0,
     1.0, -1.0, 0.0, 1.0, 1.0,
];

static INDICES: [u16; 6] = [0, 1, 2, 0, 2, 3];

#[rustfmt::skip]
static IDENTITY: [f32; 16] = [
    1.0, 0.0, 0.0, 0.0,
    0.0, 1.0, 0.0, 0.0,
    0.0, 0.0, 1.0, 0.0,
    0.0, 0.0, 0.0, 1.0,
];

const VS_SRC: &[u8] = b"
uniform mat4 u_transformation;
attribute vec4 a_position;
attribute vec2 a_texcoord;
varying vec2 v_texcoord;

void main() {
    gl_Position = u_transformation * a_position;
    v_texcoord = a_texcoord;
}
\0";

const FS_SRC: &[u8] = b"
#ifdef GL_ES
precision mediump float;
#endif
varying vec2 v_texcoord;
uniform sampler2D tex;

void main() {
    gl_FragColor = texture2D(tex, v_texcoord);
}
\0";

#[allow(clippy::unreadable_literal)]
#[allow(clippy::unused_unit)]
#[allow(clippy::too_many_arguments)]
mod gl {
    pub use self::Gles2 as Gl;
    include!(concat!(env!("OUT_DIR"), "/test_gl_bindings.rs"));
}

struct Gl {
    gl: gl::Gl,
    program: gl::types::GLuint,
    attr_position: gl::types::GLint,
    attr_texture: gl::types::GLint,
    vao: Option<gl::types::GLuint>,
    vertex_buffer: gl::types::GLuint,
    vbo_indices: gl::types::GLuint,
}

impl Gl {
    fn draw_frame(&self, texture_id: gl::types::GLuint) {
        unsafe {
            // render
            self.gl.ClearColor(0.0, 0.0, 0.0, 1.0);
            self.gl.Clear(gl::COLOR_BUFFER_BIT);

            self.gl.BlendColor(0.0, 0.0, 0.0, 1.0);
            if self.gl.BlendFuncSeparate.is_loaded() {
                self.gl.BlendFuncSeparate(
                    gl::SRC_ALPHA,
                    gl::CONSTANT_COLOR,
                    gl::ONE,
                    gl::ONE_MINUS_SRC_ALPHA,
                );
            } else {
                self.gl.BlendFunc(gl::SRC_ALPHA, gl::CONSTANT_COLOR);
            }
            self.gl.BlendEquation(gl::FUNC_ADD);
            self.gl.Enable(gl::BLEND);

            self.gl.UseProgram(self.program);

            if self.gl.BindVertexArray.is_loaded() {
                self.gl.BindVertexArray(self.vao.unwrap());
            }

            {
                self.gl
                    .BindBuffer(gl::ELEMENT_ARRAY_BUFFER, self.vbo_indices);
                self.gl.BindBuffer(gl::ARRAY_BUFFER, self.vertex_buffer);

                // Load the vertex position
                self.gl.VertexAttribPointer(
                    self.attr_position as gl::types::GLuint,
                    3,
                    gl::FLOAT,
                    gl::FALSE,
                    (5 * mem::size_of::<f32>()) as gl::types::GLsizei,
                    ptr::null(),
                );

                // Load the texture coordinate
                self.gl.VertexAttribPointer(
                    self.attr_texture as gl::types::GLuint,
                    2,
                    gl::FLOAT,
                    gl::FALSE,
                    (5 * mem::size_of::<f32>()) as gl::types::GLsizei,
                    (3 * mem::size_of::<f32>()) as *const () as *const _,
                );

                self.gl.EnableVertexAttribArray(self.attr_position as _);
                self.gl.EnableVertexAttribArray(self.attr_texture as _);
            }

            self.gl.ActiveTexture(gl::TEXTURE0);
            self.gl.BindTexture(gl::TEXTURE_2D, texture_id);

            let location = self
                .gl
                .GetUniformLocation(self.program, b"tex\0".as_ptr() as *const _);
            self.gl.Uniform1i(location, 0);

            let location = self
                .gl
                .GetUniformLocation(self.program, b"u_transformation\0".as_ptr() as *const _);
            self.gl
                .UniformMatrix4fv(location, 1, gl::FALSE, IDENTITY.as_ptr() as *const _);

            self.gl
                .DrawElements(gl::TRIANGLES, 6, gl::UNSIGNED_SHORT, ptr::null());

            self.gl.BindTexture(gl::TEXTURE_2D, 0);
            self.gl.UseProgram(0);

            if self.gl.BindVertexArray.is_loaded() {
                self.gl.BindVertexArray(0);
            }

            {
                self.gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
                self.gl.BindBuffer(gl::ARRAY_BUFFER, 0);

                self.gl.DisableVertexAttribArray(self.attr_position as _);
                self.gl.DisableVertexAttribArray(self.attr_texture as _);
            }
        }
    }

    fn resize(&self, size: glutin::dpi::PhysicalSize) {
        unsafe {
            self.gl
                .Viewport(0, 0, size.width as i32, size.height as i32);
        }
    }
}

fn load(gl_context: &glutin::WindowedContext<glutin::PossiblyCurrent>) -> Gl {
    let gl = gl::Gl::load_with(|ptr| gl_context.get_proc_address(ptr) as *const _);

    let version = unsafe {
        let data = CStr::from_ptr(gl.GetString(gl::VERSION) as *const _)
            .to_bytes()
            .to_vec();
        String::from_utf8(data).unwrap()
    };

    println!("OpenGL version {}", version);

    let (program, attr_position, attr_texture, vao, vertex_buffer, vbo_indices) = unsafe {
        let vs = gl.CreateShader(gl::VERTEX_SHADER);
        gl.ShaderSource(vs, 1, [VS_SRC.as_ptr() as *const _].as_ptr(), ptr::null());
        gl.CompileShader(vs);

        let fs = gl.CreateShader(gl::FRAGMENT_SHADER);
        gl.ShaderSource(fs, 1, [FS_SRC.as_ptr() as *const _].as_ptr(), ptr::null());
        gl.CompileShader(fs);

        let program = gl.CreateProgram();
        gl.AttachShader(program, vs);
        gl.AttachShader(program, fs);
        gl.LinkProgram(program);

        {
            let mut success: gl::types::GLint = 1;
            gl.GetProgramiv(fs, gl::LINK_STATUS, &mut success);
            assert!(success != 0);
        }

        let attr_position = gl.GetAttribLocation(program, b"a_position\0".as_ptr() as *const _);
        let attr_texture = gl.GetAttribLocation(program, b"a_texcoord\0".as_ptr() as *const _);

        let vao = if gl.BindVertexArray.is_loaded() {
            let mut vao = mem::MaybeUninit::uninit();
            gl.GenVertexArrays(1, vao.as_mut_ptr());
            let vao = vao.assume_init();
            gl.BindVertexArray(vao);
            Some(vao)
        } else {
            None
        };

        let mut vertex_buffer = mem::MaybeUninit::uninit();
        gl.GenBuffers(1, vertex_buffer.as_mut_ptr());
        let vertex_buffer = vertex_buffer.assume_init();
        gl.BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);
        gl.BufferData(
            gl::ARRAY_BUFFER,
            (VERTICES.len() * mem::size_of::<f32>()) as gl::types::GLsizeiptr,
            VERTICES.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );

        let mut vbo_indices = mem::MaybeUninit::uninit();
        gl.GenBuffers(1, vbo_indices.as_mut_ptr());
        let vbo_indices = vbo_indices.assume_init();
        gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, vbo_indices);
        gl.BufferData(
            gl::ELEMENT_ARRAY_BUFFER,
            (INDICES.len() * mem::size_of::<u16>()) as gl::types::GLsizeiptr,
            INDICES.as_ptr() as *const _,
            gl::STATIC_DRAW,
        );

        if gl.BindVertexArray.is_loaded() {
            gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, vbo_indices);
            gl.BindBuffer(gl::ARRAY_BUFFER, vertex_buffer);

            // Load the vertex position
            gl.VertexAttribPointer(
                attr_position as gl::types::GLuint,
                3,
                gl::FLOAT,
                gl::FALSE,
                (5 * mem::size_of::<f32>()) as gl::types::GLsizei,
                ptr::null(),
            );

            // Load the texture coordinate
            gl.VertexAttribPointer(
                attr_texture as gl::types::GLuint,
                2,
                gl::FLOAT,
                gl::FALSE,
                (5 * mem::size_of::<f32>()) as gl::types::GLsizei,
                (3 * mem::size_of::<f32>()) as *const () as *const _,
            );

            gl.EnableVertexAttribArray(attr_position as _);
            gl.EnableVertexAttribArray(attr_texture as _);

            gl.BindVertexArray(0);
        }

        gl.BindBuffer(gl::ELEMENT_ARRAY_BUFFER, 0);
        gl.BindBuffer(gl::ARRAY_BUFFER, 0);

        (
            program,
            attr_position,
            attr_texture,
            vao,
            vertex_buffer,
            vbo_indices,
        )
    };

    Gl {
        gl,
        program,
        attr_position,
        attr_texture,
        vao,
        vertex_buffer,
        vbo_indices,
    }
}

struct App {
    pipeline: gst::Pipeline,
    appsink: gst_app::AppSink,
    glupload: gst::Element,
    bus: gst::Bus,
    events_loop: glutin::EventsLoop,
    windowed_context: glutin::WindowedContext<glutin::PossiblyCurrent>,
    shared_context: gst_gl::GLContext,
}

impl App {
    fn new() -> Result<App, Error> {
        gst::init()?;

        let (pipeline, appsink, glupload) = App::create_pipeline()?;
        let bus = pipeline
            .get_bus()
            .expect("Pipeline without bus. Shouldn't happen!");

        let events_loop = glutin::EventsLoop::new();
        let window = glutin::WindowBuilder::new().with_title("GL rendering");
        let windowed_context = glutin::ContextBuilder::new()
            .with_vsync(true)
            .build_windowed(window, &events_loop)?;

        let windowed_context = unsafe { windowed_context.make_current().map_err(|(_, err)| err)? };

        #[cfg(any(feature = "gl-x11", feature = "gl-wayland"))]
        let inner_window = windowed_context.window();

        let shared_context: gst_gl::GLContext;
        if cfg!(target_os = "linux") {
            use glutin::os::unix::RawHandle;
            #[cfg(any(feature = "gl-x11", feature = "gl-wayland"))]
            use glutin::os::unix::WindowExt;
            use glutin::os::ContextTraitExt;

            let api = App::map_gl_api(windowed_context.get_api());

            let (gl_context, gl_display, platform) = match unsafe { windowed_context.raw_handle() }
            {
                #[cfg(any(feature = "gl-egl", feature = "gl-wayland"))]
                RawHandle::Egl(egl_context) => {
                    #[cfg(feature = "gl-egl")]
                    let gl_display =
                        if let Some(display) = unsafe { windowed_context.get_egl_display() } {
                            unsafe { gst_gl::GLDisplayEGL::new_with_egl_display(display as usize) }
                                .unwrap()
                        } else {
                            panic!("EGL context without EGL display");
                        };

                    #[cfg(not(feature = "gl-egl"))]
                    let gl_display = if let Some(display) = inner_window.get_wayland_display() {
                        unsafe { gst_gl::GLDisplayWayland::new_with_display(display as usize) }
                            .unwrap()
                    } else {
                        panic!("Wayland window without Wayland display");
                    };

                    (
                        egl_context as usize,
                        gl_display.upcast::<gst_gl::GLDisplay>(),
                        gst_gl::GLPlatform::EGL,
                    )
                }
                #[cfg(feature = "gl-x11")]
                RawHandle::Glx(glx_context) => {
                    let gl_display = if let Some(display) = inner_window.get_xlib_display() {
                        unsafe { gst_gl::GLDisplayX11::new_with_display(display as usize) }.unwrap()
                    } else {
                        panic!("X11 window without X Display");
                    };

                    (
                        glx_context as usize,
                        gl_display.upcast::<gst_gl::GLDisplay>(),
                        gst_gl::GLPlatform::GLX,
                    )
                }
                #[allow(unreachable_patterns)]
                handler => panic!("Unsupported platform: {:?}.", handler),
            };

            shared_context =
                unsafe { gst_gl::GLContext::new_wrapped(&gl_display, gl_context, platform, api) }
                    .unwrap();

            shared_context
                .activate(true)
                .expect("Couldn't activate wrapped GL context");

            shared_context.fill_info()?;

            let gl_context = shared_context.clone();
            let events_proxy = events_loop.create_proxy();

            #[allow(clippy::single_match)]
            bus.set_sync_handler(move |_, msg| {
                match msg.view() {
                    gst::MessageView::NeedContext(ctxt) => {
                        let context_type = ctxt.get_context_type();
                        if context_type == *gst_gl::GL_DISPLAY_CONTEXT_TYPE {
                            if let Some(el) =
                                msg.get_src().map(|s| s.downcast::<gst::Element>().unwrap())
                            {
                                let context = gst::Context::new(context_type, true);
                                context.set_gl_display(&gl_display);
                                el.set_context(&context);
                            }
                        }
                        if context_type == "gst.gl.app_context" {
                            if let Some(el) =
                                msg.get_src().map(|s| s.downcast::<gst::Element>().unwrap())
                            {
                                let mut context = gst::Context::new(context_type, true);
                                {
                                    let context = context.get_mut().unwrap();
                                    let s = context.get_mut_structure();
                                    s.set_value("context", gl_context.to_send_value());
                                }
                                el.set_context(&context);
                            }
                        }
                    }
                    _ => (),
                }

                let _ = events_proxy.wakeup();

                gst::BusSyncReply::Pass
            });
        } else {
            panic!("This example only has Linux support");
        }

        Ok(App {
            pipeline,
            appsink,
            glupload,
            bus,
            events_loop,
            windowed_context,
            shared_context,
        })
    }

    fn setup(
        &self,
        events_loop: &glutin::EventsLoop,
    ) -> Result<mpsc::Receiver<gst::Sample>, Error> {
        let events_proxy = events_loop.create_proxy();
        let (sender, receiver) = mpsc::channel();
        self.appsink.set_callbacks(
            gst_app::AppSinkCallbacks::new()
                .new_sample(move |appsink| {
                    let sample = appsink.pull_sample().map_err(|_| gst::FlowError::Eos)?;

                    {
                        let _buffer = sample.get_buffer().ok_or_else(|| {
                            gst_element_error!(
                                appsink,
                                gst::ResourceError::Failed,
                                ("Failed to get buffer from appsink")
                            );

                            gst::FlowError::Error
                        })?;

                        let _info = sample
                            .get_caps()
                            .and_then(|caps| gst_video::VideoInfo::from_caps(caps).ok())
                            .ok_or_else(|| {
                                gst_element_error!(
                                    appsink,
                                    gst::ResourceError::Failed,
                                    ("Failed to get video info from sample")
                                );

                                gst::FlowError::Error
                            })?;
                    }

                    sender
                        .send(sample)
                        .map(|_| gst::FlowSuccess::Ok)
                        .map_err(|_| gst::FlowError::Error)?;

                    let _ = events_proxy.wakeup();

                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        self.pipeline.set_state(gst::State::Playing)?;

        Ok(receiver)
    }

    fn map_gl_api(api: glutin::Api) -> gst_gl::GLAPI {
        match api {
            glutin::Api::OpenGl => gst_gl::GLAPI::OPENGL3,
            glutin::Api::OpenGlEs => gst_gl::GLAPI::GLES2,
            _ => gst_gl::GLAPI::NONE,
        }
    }

    fn create_pipeline() -> Result<(gst::Pipeline, gst_app::AppSink, gst::Element), Error> {
        let pipeline = gst::Pipeline::new(None);
        
        let src = gst::ElementFactory::make("filesrc", None)
            .map_err(|_| MissingElement("filesrc"))?;
        src.set_property("location", &"resources/test2.mp4".to_string())?;

        let qtdemuxer =
            gst::ElementFactory::make("qtdemux", None).map_err(|_| MissingElement("qtdemux"))?;               

        pipeline.add_many(&[&src, &qtdemuxer])?;
        src.link(&qtdemuxer)?;

        let queue =
                gst::ElementFactory::make("queue", None).map_err(|_| MissingElement("queue"))?;
        let h264parse =
            gst::ElementFactory::make("h264parse", None).map_err(|_| MissingElement("h264parse"))?;

        let decoder = {
            if let Ok(d) = gst::ElementFactory::make("vaapidecodebin", None) {
                d
            } else {
                gst::ElementFactory::make("avdec_h264", None).map_err(|_| MissingElement("avdec_h264"))?
            }
        };
            
        let sink =
            gst::ElementFactory::make("glsinkbin", None).map_err(|_| MissingElement("glsinkbin"))?;

        

        let elements = &[&queue, &h264parse, &decoder, &sink];

        pipeline.add_many(elements)?;
        gst::Element::link_many(elements)?;

        //hmmm did i need it?
        for e in elements {
            e.sync_state_with_parent()?
        }

        let queue_weak = queue.downgrade();
        //let pipeline_weak = pipeline.downgrade();
        qtdemuxer.connect_pad_added(move |qtdemux, src_pad| {

            let queue = match queue_weak.upgrade() {
                Some(queue) => queue,
                None => return,
            };

            /*let pipeline = match pipeline_weak.upgrade() {
                Some(pipeline) => pipeline,
                None => return,
            };*/

            handle_demux_pad_added(qtdemux, src_pad, &queue);
        });

        qtdemuxer
            .sync_state_with_parent()
            .expect("Failed to build remux pipeline");

        let appsink = gst::ElementFactory::make("appsink", None)
            .map_err(|_| MissingElement("appsink"))?
            .dynamic_cast::<gst_app::AppSink>()
            .expect("Sink element is expected to be an appsink!");

        sink.set_property("sink", &appsink)?;

        appsink.set_property("enable-last-sample", &false.to_value())?;
        appsink.set_property("emit-signals", &false.to_value())?;
        appsink.set_property("max-buffers", &1u32.to_value())?;

        let caps = gst::Caps::builder("video/x-raw")
            .features(&[&gst_gl::CAPS_FEATURE_MEMORY_GL_MEMORY])
            .field("format", &gst_video::VideoFormat::Rgba.to_string())
            .field("texture-target", &"2D")
            .build();
        appsink.set_caps(Some(&caps));

        // get the glupload element to extract later the used context in it
        let mut iter = sink.dynamic_cast::<gst::Bin>().unwrap().iterate_elements();
        let glupload = loop {
            match iter.next() {
                Ok(Some(element)) => {
                    if "glupload" == element.get_factory().unwrap().get_name() {
                        break Some(element);
                    }
                }
                Err(gst::IteratorError::Resync) => iter.resync(),
                _ => break None,
            }
        };

        Ok((pipeline, appsink, glupload.unwrap()))
    }

    fn handle_messages(bus: &gst::Bus, pipeline: &gst::Pipeline) -> Result<(), Error> {
        use gst::MessageView;

        for msg in bus.iter() {
            match msg.view() {
                MessageView::Eos(..) => {
                    pipeline.set_state(gst::State::Ready);
                    pipeline.set_state(gst::State::Playing);
                    break
                },
                MessageView::Error(err) => {
                    return Err(ErrorMessage {
                        src: msg
                            .get_src()
                            .map(|s| String::from(s.get_path_string()))
                            .unwrap_or_else(|| String::from("None")),
                        error: err.get_error().description().into(),
                        debug: err.get_debug(),
                        cause: err.get_error(),
                    }
                    .into());
                }
                _ => (),
            }
        }

        Ok(())
    }

    fn into_context(self: App) -> glutin::WindowedContext<glutin::PossiblyCurrent> {
        self.windowed_context
    }
}

// This is the callback function called by the demuxer, when a new stream was detected.
fn handle_demux_pad_added(
    demuxer: &gst::Element,
    src_pad: &gst::Pad,
    videoqueue: &gst::Element,
) {
    //println!("what is the name of the src_pad:{}", src_pad.get_name().as_str());

    let (is_audio, is_video) = {
        let media_type = src_pad.get_current_caps().and_then(|caps| {
            caps.get_structure(0).map(|s| {
                let name = s.get_name();
                (name.starts_with("audio"), name.starts_with("video"))
            })
        });

        match media_type {
            None => {
                gst_element_warning!(
                    demuxer,
                    gst::CoreError::Negotiation,
                    ("Failed to get media type from pad {}", src_pad.get_name())
                );

                return;
            }
            Some(media_type) => media_type,
        }
    };


    // We create a closure here, calling it directly below it, because this greatly
    // improves readability for error-handling. Like this, we can simply use the
    // ?-operator within the closure, and handle the actual error down below where
    // we call the insert_sink(..) closure.
    let insert_sink = |is_audio, is_video| -> Result<(), Error> {
        // currently no audio
        if is_video {
            
            // Get the queue element's sink pad and link the decodebin's newly created
            // src pad for the video stream to it.
            let sink_pad = videoqueue.get_static_pad("sink").expect("videoqueue has no sinkpad");
            src_pad.link(&sink_pad)?;
        }

        Ok(())
    };

    // When adding and linking new elements in a callback fails, error information is often sparse.
    // GStreamer's built-in debugging can be hard to link back to the exact position within the code
    // that failed. Since callbacks are called from random threads within the pipeline, it can get hard
    // to get good error information. The macros used in the following can solve that. With the use
    // of those, one can send arbitrary rust types (using the pipeline's bus) into the mainloop.
    // What we send here is unpacked down below, in the iteration-code over sent bus-messages.
    // Because we are using the failure crate for error details here, we even get a backtrace for
    // where the error was constructed. (If RUST_BACKTRACE=1 is set)
    if let Err(err) = insert_sink(is_audio, is_video) {
        // The following sends a message of type Error on the bus, containing our detailed
        // error information.
        gst_element_error!(
            demuxer,
            gst::LibraryError::Failed,
            ("Failed to insert sink"),
            ["{}", err]
        );
    }
}

fn main_loop(mut app: App) -> Result<glutin::WindowedContext<glutin::PossiblyCurrent>, Error> {
    println!(
        "Pixel format of the window's GL context {:?}",
        app.windowed_context.get_pixel_format()
    );

    let gl = load(&app.windowed_context);

    let receiver = app.setup(&app.events_loop)?;

    let mut curr_frame: Option<gst_video::VideoFrame<gst_video::video_frame::Readable>> = None;
    let mut running = true;
    let mut gst_gl_context: Option<gst_gl::GLContext> = None;
    let events_loop = &mut app.events_loop;
    let windowed_context = &mut app.windowed_context;
    let bus = &app.bus;
    let pipeline = &app.pipeline;

    while running {
        #[allow(clippy::single_match)]
        events_loop.poll_events(|event| match event {
            glutin::Event::WindowEvent { event, .. } => match event {
                glutin::WindowEvent::CloseRequested => running = false,
                glutin::WindowEvent::Resized(logical_size) => {
                    let dpi_factor = windowed_context.window().get_hidpi_factor();
                    windowed_context.resize(logical_size.to_physical(dpi_factor));
                    gl.resize(logical_size.to_physical(dpi_factor));
                }
                _ => (),
            },
            _ => (),
        });

        // Handle all pending messages. Whenever there is a message we will
        // wake up the events loop above
        App::handle_messages(&bus, &pipeline)?;

        // get the last frame in channel
        if let Some(sample) = receiver.try_iter().last() {
            let buffer = sample.get_buffer_owned().unwrap();
            let info = sample
                .get_caps()
                .and_then(|caps| gst_video::VideoInfo::from_caps(caps).ok())
                .unwrap();

            {
                if gst_gl_context.is_none() {
                    gst_gl_context = app
                        .glupload
                        .get_property("context")
                        .unwrap()
                        .get::<gst_gl::GLContext>()
                        .unwrap();
                }

                let sync_meta = buffer.get_meta::<gst_gl::GLSyncMeta>().unwrap();
                sync_meta.set_sync_point(gst_gl_context.as_ref().unwrap());
            }

            if let Ok(frame) = gst_video::VideoFrame::from_buffer_readable_gl(buffer, &info) {
                curr_frame = Some(frame);
            }
        }

        if let Some(frame) = curr_frame.as_ref() {
            let sync_meta = frame.buffer().get_meta::<gst_gl::GLSyncMeta>().unwrap();
            sync_meta.wait(&app.shared_context);
            if let Some(texture) = frame.get_texture_id(0) {
                gl.draw_frame(texture as gl::types::GLuint);
            }
        }
        windowed_context.swap_buffers()?;
    }

    app.pipeline.send_event(gst::Event::new_eos().build());
    app.pipeline.set_state(gst::State::Null)?;

    Ok(app.into_context())
}

fn cleanup(
    _windowed_context: glutin::WindowedContext<glutin::PossiblyCurrent>,
) -> Result<(), failure::Error> {
    // To ensure that the context stays alive longer than the pipeline or any reference
    // inside GStreamer to the GL context, its display or anything else. See
    // https://gitlab.freedesktop.org/gstreamer/gstreamer-rs/issues/196
    //
    // We might do any window/GL specific cleanup here as needed.

    Ok(())
}

fn example_main() {
    match App::new().and_then(main_loop).and_then(cleanup) {
        Ok(r) => r,
        Err(e) => eprintln!("Error! {}", e),
    }
}

fn main() {
    common::run(example_main);
}
