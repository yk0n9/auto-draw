use std::{
    io::Cursor,
    sync::{Arc, LazyLock},
    thread,
    time::Duration,
};

use crossbeam::atomic::AtomicCell;
use eframe::{
    egui::{self, Image},
    App,
};
use enigo::{Enigo, Mouse, Settings};
use image::{imageops::FilterType, DynamicImage, GenericImageView};
use imageproc::{
    contours::{self, Contour},
    edges,
};
use nanoid::nanoid;
use parking_lot::RwLock;
use rfd::FileDialog;
use windows::Win32::UI::{
    Input::KeyboardAndMouse::{GetAsyncKeyState, VK_F1, VK_F2},
    WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN},
};

pub static STATE: AtomicCell<State> = AtomicCell::new(State::Stop);
pub static DRAWING: AtomicCell<bool> = AtomicCell::new(false);
pub static SCREEN: LazyLock<(i32, i32)> =
    LazyLock::new(|| unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) });

#[derive(Debug, Clone, Copy)]
pub enum State {
    Drawing,
    Stop,
}

#[derive(Debug, Clone)]
pub struct Panel {
    pub center: Arc<RwLock<(i32, i32)>>,
    pub area: u32,
    pub canny_value: u32,
    pub canny_image: Arc<RwLock<Option<Img>>>,
    pub resized_img: Arc<RwLock<Option<DynamicImage>>>,
    pub raw_img: Arc<RwLock<Option<DynamicImage>>>,
    pub lines: Arc<RwLock<Option<Vec<Contour<i32>>>>>,
}

#[derive(Debug, Clone)]
pub struct Img {
    id: String,
    buf: Vec<u8>,
}

impl Default for Panel {
    fn default() -> Self {
        Self {
            center: Arc::new(RwLock::new((0, 0))),
            area: 70,
            canny_value: 25,
            canny_image: Arc::new(RwLock::new(None)),
            resized_img: Arc::new(RwLock::new(None)),
            raw_img: Arc::new(RwLock::new(None)),
            lines: Arc::new(RwLock::new(None)),
        }
    }
}

impl Panel {
    fn open_image(&self) {
        let image_center = self.center.clone();
        let area = self.area;
        let canny_value = self.canny_value;
        let canny_image = self.canny_image.clone();
        let lines = self.lines.clone();
        let resized_img = self.resized_img.clone();
        let raw_img = self.raw_img.clone();
        rayon::spawn(move || {
            let Some(path) = FileDialog::new()
                .add_filter("Image", &["jpg", "jpeg", "png"])
                .pick_file()
            else {
                return;
            };

            let Ok(mut image) = image::open(&path) else {
                return;
            };
            raw_img.write().replace(image.clone());

            let dim = image.dimensions();

            let r = (
                (SCREEN.0 as f32 * (area as f32 / 100.0)) as i32,
                (SCREEN.1 as f32 * (area as f32 / 100.0)) as i32,
            );

            let rect = if (dim.1 as f32 / dim.0 as f32) < (2.0 / 3.0) {
                (r.0, r.0)
            } else {
                (r.1, r.1)
            };

            if dim.0 > rect.0 as _ || dim.1 > rect.1 as _ {
                image = image.resize(rect.0 as _, rect.1 as _, FilterType::Lanczos3);
            }
            let center = (
                (SCREEN.0 - image.width() as i32) / 2,
                (SCREEN.1 - image.height() as i32) / 2,
            );
            *image_center.write() = center;

            let gray = image.to_luma8();
            resized_img.write().replace(image);

            let canny = edges::canny(&gray, canny_value as f32, 3.0 * canny_value as f32);
            let mut data = Cursor::new(vec![]);
            canny.write_to(&mut data, image::ImageFormat::Png).ok();
            canny_image.write().replace(Img {
                id: nanoid!(),
                buf: data.into_inner(),
            });

            let mut contours = contours::find_contours(&canny);
            contours.iter_mut().for_each(|contour| {
                contour.points.iter_mut().for_each(|point| {
                    point.x += center.0;
                    point.y += center.1;
                });
            });
            lines.write().replace(contours);
        });
    }

    fn resize(&self, mut image: DynamicImage) -> (i32, i32) {
        let dim = image.dimensions();

        let r = (
            (SCREEN.0 as f32 * (self.area as f32 / 100.0)) as i32,
            (SCREEN.1 as f32 * (self.area as f32 / 100.0)) as i32,
        );

        let rect = if (dim.1 as f32 / dim.0 as f32) < (2.0 / 3.0) {
            (r.0, r.0)
        } else {
            (r.1, r.1)
        };

        if dim.0 > rect.0 as _ || dim.1 > rect.1 as _ {
            image = image.resize(rect.0 as _, rect.1 as _, FilterType::Lanczos3);
        }
        let center = (
            (SCREEN.0 - image.width() as i32) / 2,
            (SCREEN.1 - image.height() as i32) / 2,
        );

        self.resized_img.write().replace(image);
        center
    }

    fn reload(&self, area: bool) {
        if area {
            let raw_img = self.raw_img.read();
            let Some(image) = raw_img.as_ref() else {
                return;
            };
            *self.center.write() = self.resize(image.clone());
        }

        let resized_img = self.resized_img.read();
        let Some(resized_img) = resized_img.as_ref() else {
            return;
        };
        let center = *self.center.read();
        let image = resized_img.to_luma8();
        let canny = edges::canny(
            &image,
            self.canny_value as f32,
            3.0 * self.canny_value as f32,
        );
        let mut data = Cursor::new(vec![]);
        canny.write_to(&mut data, image::ImageFormat::Png).ok();
        self.canny_image.write().replace(Img {
            id: nanoid!(),
            buf: data.into_inner(),
        });

        let mut contours = contours::find_contours(&canny);
        contours.iter_mut().for_each(|contour| {
            contour.points.iter_mut().for_each(|point| {
                point.x += center.0;
                point.y += center.1;
            });
        });
        self.lines.write().replace(contours);
    }

    fn draw(&self) {
        let contours = self.lines.clone();
        rayon::spawn(move || {
            STATE.store(State::Drawing);
            DRAWING.store(true);
            let contours = contours.read();
            let Some(contours) = contours.as_ref() else {
                STATE.store(State::Stop);
                return;
            };

            let mut enigo = Enigo::new(&Settings::default()).unwrap();

            for contour in contours.iter() {
                if let State::Stop = STATE.load() {
                    enigo
                        .button(enigo::Button::Left, enigo::Direction::Release)
                        .ok();
                    break;
                }
                if contour.points.is_empty() {
                    continue;
                }

                enigo
                    .move_mouse(
                        contour.points[0].x,
                        contour.points[0].y,
                        enigo::Coordinate::Abs,
                    )
                    .ok();
                enigo
                    .button(enigo::Button::Left, enigo::Direction::Press)
                    .ok();
                for point in &contour.points[1..] {
                    if let State::Stop = STATE.load() {
                        break;
                    }
                    enigo
                        .move_mouse(point.x, point.y, enigo::Coordinate::Abs)
                        .ok();
                    thread::sleep(Duration::from_micros(100));
                }
                enigo
                    .button(enigo::Button::Left, enigo::Direction::Release)
                    .ok();
                thread::sleep(Duration::from_millis(100));
            }
            STATE.store(State::Stop);
            DRAWING.store(false);
        });
    }
}

impl App for Panel {
    fn update(&mut self, ctx: &eframe::egui::Context, _frame: &mut eframe::Frame) {
        ctx.request_repaint();
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Select Image").clicked() {
                    ctx.forget_all_images();
                    self.open_image();
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut self.canny_value)
                            .range(1..=u32::MAX)
                            .prefix("low threshold: "),
                    )
                    .changed()
                {
                    ctx.forget_all_images();
                    self.reload(false);
                }
                if ui
                    .add(
                        egui::DragValue::new(&mut self.area)
                            .range(0..=100)
                            .prefix("Drawing area: ")
                            .custom_formatter(|n, _| format!("{n}%")),
                    )
                    .changed()
                {
                    ctx.forget_all_images();
                    self.reload(true);
                }
            });
            ui.separator();

            ui.label("Press F1 to start draw");
            ui.label("Press F2 to stop draw");
            ui.separator();

            if let Some(image) = self.canny_image.read().as_ref() {
                ui.add(Image::from_bytes(image.id.to_string(), image.buf.to_vec()));
            }

            if is_pressed(VK_F1.0) {
                match STATE.load() {
                    State::Stop => {
                        if !DRAWING.load() {
                            self.draw();
                        }
                    }
                    State::Drawing => {}
                }
            }
            if is_pressed(VK_F2.0) {
                STATE.store(State::Stop);
            }
        });
    }
}

pub fn is_pressed(vk: u16) -> bool {
    let status = unsafe { GetAsyncKeyState(vk as i32) as u32 };
    status >> 31 == 1
}
